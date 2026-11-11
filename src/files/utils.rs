use crate::files::types::{FileMetadata, ObjectType};
use blake3::Hash;
use std::fs::File;
use std::fs::{self, OpenOptions, symlink_metadata};
use std::io::Result;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const IO_BUFFER_SIZE: usize = 64 * 1024;
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn object_header(obj_type: ObjectType, byte_len: u64) -> Vec<u8> {
    format!("{} {}\0", obj_type.header_prefix(), byte_len).into_bytes()
}

pub fn time_format(time_: SystemTime) -> (u64, u32) {
    let duration = time_
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime is set before UNIX EPOCH");

    let secs: u64 = duration.as_secs();
    let subsec_nanos: u32 = duration.subsec_nanos();
    (secs, subsec_nanos)
}
pub fn get_file_metadata(path: &Path) -> FileMetadata {
    let metadata = symlink_metadata(path).unwrap();

    let atime = metadata.accessed().unwrap();
    let mtime = metadata.modified().unwrap();

    let size = metadata.len();

    FileMetadata {
        atime,
        mtime,
        size: Some(size),
    }
}

/// Compute the Git-style single-pass BLAKE3 object hash (`"<type> <len>\0" || content`).
pub fn hash(path: &Path, size: Option<u64>, obj_type: ObjectType) -> Hash {
    // in future size will be used to chose rayon, mmap or not
    let byte_len = match size {
        Some(len) => len,
        None => fs::metadata(path)
            .expect("Failed to read file metadata")
            .len(),
    };

    let mut hasher = blake3::Hasher::new();
    hasher.update(&object_header(obj_type, byte_len));
    hasher.update_mmap_rayon(path).expect("Failed to hash file");

    hasher.finalize()
}

/// Write a Git-style uncompressed object atomically under `.git-med/{objects|meta_objects}/{dir}/{filename}`.
pub fn make_object(path: &Path, hash: Hash, obj_type: ObjectType) -> Result<()> {
    make_object_in(path, hash, obj_type, Path::new("."))
}

// in .git-med/{objects|meta_objects}/{dir}/{filename}
// write header `<type> <byte_length>\0` then stream a copy of the file from path
fn make_object_in(
    path: &Path,
    hash: Hash,
    obj_type: ObjectType,
    repo_root: &Path,
) -> Result<()> {
    let hex = hash.to_hex();
    let dir = &hex[..2];
    let filename = &hex[2..];

    let object_dir = repo_root.join(".git-med").join(obj_type.dir_name()).join(dir);
    let object_path = object_dir.join(filename);

    if object_path.exists() {
        return Ok(());
    }

    let source = File::open(path)?;
    let byte_len = source.metadata()?.len();
    let header = object_header(obj_type, byte_len);

    fs::create_dir_all(&object_dir)?;

    let tmp_seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = object_dir.join(format!(
        ".{}.tmp.{}.{}",
        filename,
        std::process::id(),
        tmp_seq
    ));

    if let Err(err) = write_object_atomic(source, &header, byte_len, &tmp_path, &object_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    Ok(())
}

fn write_object_atomic(
    source: File,
    header: &[u8],
    expected_len: u64,
    tmp_path: &Path,
    object_path: &Path,
) -> Result<()> {
    let tmp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(tmp_path)?;

    let mut reader = BufReader::with_capacity(IO_BUFFER_SIZE, source);
    let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, tmp_file);

    writer.write_all(header)?;
    let copied = io::copy(&mut reader, &mut writer)?;
    if copied != expected_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "file size changed while writing object: expected {} bytes, copied {}",
                expected_len, copied
            ),
        ));
    }

    writer.flush()?;
    writer
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()?;

    fs::rename(tmp_path, object_path)?;
    Ok(())
}

// vibe coded test. Not well reviewed but should work. Use with caution.
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "git_med_utils_{}_{}_{}",
                label,
                std::process::id(),
                seq
            ));
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn expected_object_path(repo_root: &Path, digest: Hash, obj_type: ObjectType) -> PathBuf {
        let hex = digest.to_hex();
        repo_root
            .join(".git-med")
            .join(obj_type.dir_name())
            .join(&hex[..2])
            .join(&hex[2..])
    }

    #[test]
    fn test_hash_matches_git_style_header_and_content() {
        let tmp = TempDir::new("hash");
        let file_path = tmp.path.join("sample.bin");
        let payload = b"medical imaging payload";
        fs::write(&file_path, payload).unwrap();

        for obj_type in [ObjectType::Blob, ObjectType::Meta] {
            let digest_auto = hash(&file_path, None, obj_type);
            let digest_with_size = hash(&file_path, Some(payload.len() as u64), obj_type);

            let mut expected_bytes =
                format!("{} {}\0", obj_type.header_prefix(), payload.len()).into_bytes();
            expected_bytes.extend_from_slice(payload);
            let expected_digest = blake3::hash(&expected_bytes);

            assert_eq!(digest_auto, expected_digest);
            assert_eq!(digest_with_size, expected_digest);
        }

        assert_ne!(
            hash(&file_path, None, ObjectType::Blob),
            hash(&file_path, None, ObjectType::Meta)
        );
    }

    #[test]
    fn test_make_object_writes_header_and_payload() {
        let tmp = TempDir::new("write");
        let src_path = tmp.path.join("slice_001.dcm");
        let payload = b"DICOM_TEST_BYTES_12345";
        fs::write(&src_path, payload).unwrap();

        for obj_type in [ObjectType::Blob, ObjectType::Meta] {
            let repo_root = tmp.path.join(obj_type.header_prefix());
            fs::create_dir_all(&repo_root).unwrap();

            let digest = hash(&src_path, None, obj_type);
            make_object_in(&src_path, digest, obj_type, &repo_root).unwrap();

            let obj_path = expected_object_path(&repo_root, digest, obj_type);
            assert!(obj_path.exists());

            let other_type = match obj_type {
                ObjectType::Blob => ObjectType::Meta,
                ObjectType::Meta => ObjectType::Blob,
            };
            assert!(!repo_root.join(".git-med").join(other_type.dir_name()).exists());

            let obj_bytes = fs::read(&obj_path).unwrap();
            let mut expected =
                format!("{} {}\0", obj_type.header_prefix(), payload.len()).into_bytes();
            expected.extend_from_slice(payload);
            assert_eq!(obj_bytes, expected);
            assert_eq!(blake3::hash(&obj_bytes), digest);

            let entries: Vec<_> = fs::read_dir(obj_path.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(entries.len(), 1);
        }
    }

    #[test]
    fn test_make_object_empty_file() {
        let tmp = TempDir::new("empty");
        let repo_root = tmp.path.join("repo");
        let src_path = tmp.path.join("empty.bin");
        fs::write(&src_path, b"").unwrap();

        let digest = hash(&src_path, None, ObjectType::Blob);
        make_object_in(&src_path, digest, ObjectType::Blob, &repo_root).unwrap();

        let obj_path = expected_object_path(&repo_root, digest, ObjectType::Blob);
        let obj_bytes = fs::read(&obj_path).unwrap();
        assert_eq!(obj_bytes, b"blob 0\0");
        assert_eq!(blake3::hash(&obj_bytes), digest);
    }

    #[test]
    fn test_make_object_large_file_mmap_and_streaming() {
        let tmp = TempDir::new("large");
        let repo_root = tmp.path.join("repo");
        let src_path = tmp.path.join("volume.nii");

        // Exceeds both blake3's 16 KiB mmap threshold and IO_BUFFER_SIZE (64 KiB).
        let payload: Vec<u8> = (0..200_000usize).map(|i| (i % 251) as u8).collect();
        fs::write(&src_path, &payload).unwrap();

        let digest = hash(&src_path, Some(payload.len() as u64), ObjectType::Blob);
        make_object_in(&src_path, digest, ObjectType::Blob, &repo_root).unwrap();

        let obj_path = expected_object_path(&repo_root, digest, ObjectType::Blob);
        let obj_bytes = fs::read(&obj_path).unwrap();
        let expected_header = format!("blob {}\0", payload.len()).into_bytes();
        assert_eq!(&obj_bytes[..expected_header.len()], &expected_header[..]);
        assert_eq!(&obj_bytes[expected_header.len()..], &payload[..]);
        assert_eq!(blake3::hash(&obj_bytes), digest);
    }

    #[test]
    fn test_make_object_skips_if_object_exists() {
        let tmp = TempDir::new("dedup");
        let repo_root = tmp.path.join("repo");
        let src_path = tmp.path.join("scan.dcm");
        fs::write(&src_path, b"payload").unwrap();

        let digest = hash(&src_path, None, ObjectType::Meta);
        let obj_path = expected_object_path(&repo_root, digest, ObjectType::Meta);
        fs::create_dir_all(obj_path.parent().unwrap()).unwrap();
        fs::write(&obj_path, b"sentinel").unwrap();

        make_object_in(&src_path, digest, ObjectType::Meta, &repo_root).unwrap();
        assert_eq!(fs::read(&obj_path).unwrap(), b"sentinel");
    }

    #[test]
    fn test_make_object_missing_source_errors_without_creating_objects() {
        let tmp = TempDir::new("missing");
        let repo_root = tmp.path.join("repo");
        let missing_src = tmp.path.join("does_not_exist.dcm");
        let dummy_digest = blake3::hash(b"blob 0\0");

        let err = make_object_in(&missing_src, dummy_digest, ObjectType::Blob, &repo_root)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(!repo_root.join(".git-med").exists());
    }
}

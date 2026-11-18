use std::io::Write;
use std::path::Path;
use flate2::write::ZlibEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};

use crate::files::hash::{hash_bytes, hash_raw};
use crate::files::types::ObjectType;
use crate::files::utils::make_object_from_bytes_in;
use crate::remote::types::RemoteError;

/// Unpack all base objects from a Git Packfile v2 byte stream directly into
/// `<repo_root>/.git-med/objects/` as loose Git-med objects.
///
/// Returns the number of objects unpacked.
/// Delta objects (`OBJ_OFS_DELTA` and `OBJ_REF_DELTA`) are rejected with `RemoteError::Protocol`.
pub fn unpack_packfile_into(pack_bytes: &[u8], repo_root: &Path) -> Result<usize, RemoteError> {
    if pack_bytes.len() < 12 {
        return Err(RemoteError::Protocol(
            "packfile payload is too short (< 12 bytes)".into(),
        ));
    }

    // 1. Verify 4-byte magic "PACK"
    if &pack_bytes[0..4] != b"PACK" {
        return Err(RemoteError::Protocol(format!(
            "invalid packfile magic: expected 'PACK', got {:?}",
            &pack_bytes[0..4]
        )));
    }

    // 2. Verify 4-byte version (must be 2)
    let version = u32::from_be_bytes(
        pack_bytes[4..8]
            .try_into()
            .map_err(|_| RemoteError::Protocol("failed to read packfile version".into()))?,
    );
    if version != 2 {
        return Err(RemoteError::Protocol(format!(
            "unsupported packfile version: expected 2, got {}",
            version
        )));
    }

    // 3. Number of objects
    let num_objects = u32::from_be_bytes(
        pack_bytes[8..12]
            .try_into()
            .map_err(|_| RemoteError::Protocol("failed to read object count".into()))?,
    ) as usize;

    let mut offset = 12;
    let mut unpacked_count = 0;

    for _ in 0..num_objects {
        if offset >= pack_bytes.len() {
            return Err(RemoteError::Protocol(
                "unexpected end of packfile data while reading object header".into(),
            ));
        }

        // 4. Decode LEB128 object header (type + uncompressed size)
        let first_byte = pack_bytes[offset];
        offset += 1;

        let obj_type_raw = (first_byte >> 4) & 0x07;
        let mut size = (first_byte & 0x0F) as usize;
        let mut shift = 4;

        let mut has_continuation = (first_byte & 0x80) != 0;
        while has_continuation {
            if offset >= pack_bytes.len() {
                return Err(RemoteError::Protocol(
                    "unexpected end of packfile data while decoding variable-length size".into(),
                ));
            }
            let next_byte = pack_bytes[offset];
            offset += 1;

            size |= ((next_byte & 0x7F) as usize) << shift;
            shift += 7;
            has_continuation = (next_byte & 0x80) != 0;
        }

        // Map object type
        let obj_type = match obj_type_raw {
            1 => ObjectType::Commit,
            2 => ObjectType::Tree,
            3 => ObjectType::Blob,
            6 | 7 => {
                return Err(RemoteError::Protocol(
                    "delta objects (OBJ_OFS_DELTA / OBJ_REF_DELTA) are not supported; packfile must contain base objects only".into(),
                ));
            }
            4 => {
                return Err(RemoteError::Protocol(
                    "OBJ_TAG packfile objects are not currently supported".into(),
                ));
            }
            other => {
                return Err(RemoteError::Protocol(format!(
                    "unknown packfile object type: {}",
                    other
                )));
            }
        };

        // 5. Decompress object payload via flate2
        let mut decompress = Decompress::new(true);
        let mut decompressed = vec![0u8; size];
        let mut total_out = 0;
        let mut total_in = 0;

        while total_out < size {
            let in_slice = &pack_bytes[offset + total_in..];
            let out_slice = &mut decompressed[total_out..];

            let before_in = decompress.total_in();
            let before_out = decompress.total_out();

            let status = decompress.decompress(
                in_slice,
                out_slice,
                FlushDecompress::None,
            )?;

            let consumed_in = (decompress.total_in() - before_in) as usize;
            let produced_out = (decompress.total_out() - before_out) as usize;

            total_in += consumed_in;
            total_out += produced_out;

            if status == Status::StreamEnd {
                break;
            }

            if consumed_in == 0 && produced_out == 0 {
                return Err(RemoteError::Protocol(
                    "stalled decompression in packfile object stream".into(),
                ));
            }
        }

        offset += decompress.total_in() as usize;

        if decompressed.len() != size {
            return Err(RemoteError::Protocol(format!(
                "decompressed object size mismatch: expected {} bytes, got {}",
                size,
                decompressed.len()
            )));
        }

        // 6. Compute object hash and write directly into .git-med/objects/
        let digest = hash_bytes(&decompressed, obj_type);
        make_object_from_bytes_in(&decompressed, digest, obj_type, repo_root)?;

        unpacked_count += 1;
    }

    Ok(unpacked_count)
}

/// Generate a Git Packfile v2 binary byte stream from a slice of objects.
///
/// Header: 4 bytes "PACK", 4 bytes version (2), 4 bytes object count (big-endian).
/// Each object: variable-length LEB128 type and size header, followed by zlib-compressed payload.
/// Trailer: abstract digest computed over all preceding packfile bytes.
pub fn create_packfile_bytes(objects: &[(ObjectType, &[u8])]) -> Result<Vec<u8>, RemoteError> {
    if objects.is_empty() {
        return Ok(Vec::new());
    }

    let mut buf = Vec::new();

    // 1. Packfile header (12 bytes)
    buf.extend_from_slice(b"PACK");
    buf.extend_from_slice(&2u32.to_be_bytes());
    buf.extend_from_slice(&(objects.len() as u32).to_be_bytes());

    // 2. Encode and compress each object
    for (obj_type, body) in objects {
        let type_id: u8 = match obj_type {
            ObjectType::Commit => 1,
            ObjectType::Tree => 2,
            ObjectType::Blob | ObjectType::Meta => 3,
        };

        // Variable-length LEB128 header:
        // Byte 0: MSB continuation bit, 3 bits type, 4 bits size
        let mut size = body.len();
        let type_bits = (type_id & 0x07) << 4;
        let size_bits = (size & 0x0F) as u8;
        size >>= 4;

        let continuation = if size > 0 { 0x80 } else { 0x00 };
        buf.push(continuation | type_bits | size_bits);

        while size > 0 {
            let next_byte = (size & 0x7F) as u8;
            size >>= 7;
            let cont = if size > 0 { 0x80 } else { 0x00 };
            buf.push(cont | next_byte);
        }

        // Deflate object payload
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(body)?;
        let compressed = encoder.finish()?;
        buf.extend_from_slice(&compressed);
    }

    // 3. Trailer checksum (computed over all preceding bytes in packfile)
    let checksum = hash_raw(&buf);
    buf.extend_from_slice(checksum.as_bytes());

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::init::init_at;
    use std::fs;
    use std::path::PathBuf;

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "git_med_packfile_test_{}_{}",
                name,
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create repo dir");
            init_at(&path);
            Self { path }
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_single_object_packfile_roundtrip() {
        let repo = TempRepo::new("single");
        let obj = (ObjectType::Blob, b"hello world".as_slice());
        let pack = create_packfile_bytes(&[obj]).unwrap();
        let count = unpack_packfile_into(&pack, &repo.path).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_multiple_objects_packfile_roundtrip() {
        let repo = TempRepo::new("multiple");
        let obj1 = (ObjectType::Blob, b"hello world".as_slice());
        let obj2 = (ObjectType::Blob, b"second object data here".as_slice());
        let pack = create_packfile_bytes(&[obj1, obj2]).unwrap();
        let count = unpack_packfile_into(&pack, &repo.path).unwrap();
        assert_eq!(count, 2);
    }
}

use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use regex::Regex;

use crate::files::hash::{hash_bytes, hash_raw, Hash};
use crate::files::types::ObjectType;
use crate::files::utils::make_object_from_bytes_in;

/// LFS pointer object format representing a large file stored outside the Git object database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfsPointer {
    pub origin: String,
    pub parent: String,
    pub local_path: String,
    pub remote_path: String,
    pub hash: Hash,
}

impl LfsPointer {
    /// Create a new LFS pointer with default self attributes.
    pub fn new(remote_path: String, hash: Hash) -> Self {
        Self {
            origin: "self".to_string(),
            parent: "self".to_string(),
            local_path: "self".to_string(),
            remote_path,
            hash,
        }
    }

    /// Serialize the LFS pointer to plain text.
    pub fn serialize(&self) -> Vec<u8> {
        format!(
            "Origin {}\nParent {}\nLocal Path {}\nRemote Path {}\nHash {}\n",
            self.origin,
            self.parent,
            self.local_path,
            self.remote_path,
            self.hash.to_hex()
        )
        .into_bytes()
    }

    /// Parse an LFS pointer from raw bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| format!("invalid utf-8 in lfs pointer: {}", e))?;
        let mut origin = None;
        let mut parent = None;
        let mut local_path = None;
        let mut remote_path = None;
        let mut hash = None;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(val) = trimmed.strip_prefix("Origin ") {
                origin = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("Parent ") {
                parent = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("Local Path ") {
                local_path = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("Remote Path ") {
                remote_path = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("Hash ") {
                hash = Some(
                    Hash::from_hex(val.trim())
                        .map_err(|e| format!("invalid hash in lfs pointer: {}", e))?,
                );
            }
        }

        Ok(Self {
            origin: origin.unwrap_or_else(|| "self".to_string()),
            parent: parent.unwrap_or_else(|| "self".to_string()),
            local_path: local_path.unwrap_or_else(|| "self".to_string()),
            remote_path: remote_path
                .ok_or_else(|| "missing Remote Path in lfs pointer".to_string())?,
            hash: hash.ok_or_else(|| "missing Hash in lfs pointer".to_string())?,
        })
    }
}

/// Directory path for raw LFS objects: `<repo_root>/.git-med/lfs/objects/`
pub fn lfs_raw_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".git-med/lfs/objects")
}

/// Absolute path on disk for a specific raw LFS object: `<repo_root>/.git-med/lfs/objects/{xx}/{yy}`
pub fn lfs_raw_object_path(repo_root: &Path, raw_hash: &Hash) -> PathBuf {
    let hex = raw_hash.to_hex();
    lfs_raw_dir(repo_root).join(&hex[..2]).join(&hex[2..])
}

/// Check if a raw LFS object exists locally.
pub fn has_lfs_raw(repo_root: &Path, raw_hash: &Hash) -> bool {
    lfs_raw_object_path(repo_root, raw_hash).is_file()
}

/// Stage a raw LFS file into `.git-med/lfs/objects/{xx}/{yy}`.
pub fn stage_lfs_file(repo_root: &Path, src_path: &Path, raw_hash: &Hash) -> Result<PathBuf, String> {
    let dest_path = lfs_raw_object_path(repo_root, raw_hash);
    if dest_path.is_file() {
        return Ok(dest_path);
    }
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create lfs object directory: {}", e))?;
    }
    fs::copy(src_path, &dest_path)
        .map_err(|e| format!("failed to stage lfs file {}: {}", src_path.display(), e))?;
    Ok(dest_path)
}

/// Stage raw LFS bytes into `.git-med/lfs/objects/{xx}/{yy}`.
pub fn stage_lfs_bytes(repo_root: &Path, bytes: &[u8], raw_hash: &Hash) -> Result<PathBuf, String> {
    let dest_path = lfs_raw_object_path(repo_root, raw_hash);
    if dest_path.is_file() {
        return Ok(dest_path);
    }
    if let Some(parent) = dest_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create lfs object directory: {}", e))?;
    }
    fs::write(&dest_path, bytes)
        .map_err(|e| format!("failed to stage lfs bytes to {}: {}", dest_path.display(), e))?;
    Ok(dest_path)
}

/// Read the effective file content for writing to the working tree.
/// If the object is ObjectType::Lfs, reads the staged raw bytes from `.git-med/lfs/objects/`.
/// Otherwise returns the object body bytes.
pub fn read_file_content_for_write(
    repo_root: &Path,
    hash: &Hash,
) -> Result<Vec<u8>, String> {
    let (obj_type, body) = crate::files::utils::read_object_bytes_in(repo_root, hash)?;
    match obj_type {
        ObjectType::Lfs => {
            let pointer = LfsPointer::parse(&body)?;
            let raw_path = if pointer.local_path != "self" && Path::new(&pointer.local_path).is_file() {
                PathBuf::from(&pointer.local_path)
            } else {
                lfs_raw_object_path(repo_root, &pointer.hash)
            };
            if !raw_path.is_file() {
                return Err(format!(
                    "missing raw LFS object {} for pointer {}",
                    pointer.hash.to_hex(),
                    hash.to_hex()
                ));
            }
            fs::read(&raw_path).map_err(|e| {
                format!(
                    "failed to read raw LFS object {}: {}",
                    raw_path.display(),
                    e
                )
            })
        }
        _ => Ok(body),
    }
}

/// Compile a glob pattern string into a Regex.
fn compile_pattern(line: &str) -> Option<Regex> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let anchored = trimmed.starts_with('/');
    let pat = trimmed.trim_start_matches('/').trim_end_matches('/');
    if pat.is_empty() {
        return None;
    }

    let mut re_str = String::from(if anchored { "^" } else { "(^|/)" });
    let chars: Vec<char> = pat.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' {
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                if i + 2 < chars.len() && chars[i + 2] == '/' {
                    re_str.push_str("(.*/)?");
                    i += 3;
                } else {
                    re_str.push_str(".*");
                    i += 2;
                }
            } else {
                re_str.push_str("[^/]*");
                i += 1;
            }
        } else if chars[i] == '?' {
            re_str.push_str("[^/]");
            i += 1;
        } else {
            let mut buf = [0u8; 4];
            re_str.push_str(&regex::escape(chars[i].encode_utf8(&mut buf)));
            i += 1;
        }
    }
    re_str.push_str("(/|$)");
    Regex::new(&re_str).ok()
}

pub const DEFAULT_LFS_PATTERNS: &str = "*.dcm\n*.nii\n*.nii.gz\n";

/// Load tracking patterns from `<repo_root>/.git-med/lfs_track`.
/// If the file does not exist, defaults to DEFAULT_LFS_PATTERNS (*.dcm, *.nii, *.nii.gz).
pub fn load_lfs_track_patterns(repo_root: &Path) -> Vec<Regex> {
    let track_path = repo_root.join(".git-med/lfs_track");
    let content = if track_path.is_file() {
        match fs::read_to_string(&track_path) {
            Ok(c) => c,
            Err(_) => DEFAULT_LFS_PATTERNS.to_string(),
        }
    } else {
        DEFAULT_LFS_PATTERNS.to_string()
    };
    content
        .lines()
        .filter_map(compile_pattern)
        .collect()
}

/// Check if a relative path matches any pattern in `<repo_root>/.git-med/lfs_track`.
pub fn is_lfs_tracked(repo_root: &Path, rel_path: &Path) -> bool {
    let patterns = load_lfs_track_patterns(repo_root);
    if patterns.is_empty() {
        return false;
    }
    let Some(path_str) = rel_path.to_str() else {
        return false;
    };
    patterns.iter().any(|re| re.is_match(path_str))
}

/// Derive the repository specifier (e.g. `owner/repo` or `repo`) for constructing remote S3 paths.
pub fn derive_repo_spec(repo_root: &Path) -> String {
    if let Ok(Some(remote)) = crate::core::remote::get_remote_in(repo_root, "origin") {
        let trimmed = remote.url.trim().trim_end_matches('/');
        if let Some(idx) = trimmed.rfind("://") {
            let after_scheme = &trimmed[idx + 3..];
            if let Some(path_idx) = after_scheme.find('/') {
                let path = &after_scheme[path_idx + 1..];
                let clean = path.strip_suffix(".git").unwrap_or(path).trim_matches('/');
                if !clean.is_empty() {
                    return clean.to_string();
                }
            }
        }
    }

    if let Ok(abs) = fs::canonicalize(repo_root) {
        if let Some(name) = abs.file_name().and_then(|n| n.to_str()) {
            return name.to_string();
        }
    }
    "repo".to_string()
}

/// Compute raw BLAKE3 hash of a file on disk.
pub fn compute_raw_file_hash(path: &Path) -> Result<Hash, String> {
    let file = File::open(path).map_err(|e| format!("failed to open file {}: {}", path.display(), e))?;
    let mut reader = BufReader::new(file);
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).map_err(|e| format!("failed to read file {}: {}", path.display(), e))?;
    Ok(hash_raw(&buf))
}

/// Create an LFS pointer object and store it under `.git-med/objects/`.
/// Returns the Hash of the created LFS pointer object (used as `node.self_hash`).
pub fn create_and_store_lfs_pointer(
    repo_root: &Path,
    raw_hash: &Hash,
) -> Result<Hash, String> {
    let repo_spec = derive_repo_spec(repo_root);
    let remote_path = format!("{}/lfs/{}", repo_spec, raw_hash.to_hex());
    let pointer = LfsPointer::new(remote_path, *raw_hash);
    let pointer_bytes = pointer.serialize();

    let pointer_hash = hash_bytes(&pointer_bytes, ObjectType::Lfs);
    make_object_from_bytes_in(&pointer_bytes, pointer_hash, ObjectType::Lfs, repo_root)
        .map_err(|e| format!("failed to store lfs pointer object: {}", e))?;

    Ok(pointer_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lfs_pointer_serialize_and_parse() {
        let raw_hash = hash_raw(b"dummy large medical file content");
        let remote_path = format!("test-repo/lfs/{}", raw_hash.to_hex());
        let pointer = LfsPointer::new(remote_path.clone(), raw_hash);

        let bytes = pointer.serialize();
        let parsed = LfsPointer::parse(&bytes).expect("parse lfs pointer");

        assert_eq!(parsed.origin, "self");
        assert_eq!(parsed.parent, "self");
        assert_eq!(parsed.local_path, "self");
        assert_eq!(parsed.remote_path, remote_path);
        assert_eq!(parsed.hash, raw_hash);
    }

    #[test]
    fn test_lfs_track_patterns_and_matching() {
        let temp_dir = std::env::temp_dir().join(format!("git_med_lfs_track_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let git_med = temp_dir.join(".git-med");
        fs::create_dir_all(&git_med).unwrap();

        fs::write(git_med.join("lfs_track"), "*.dcm\n*.nii.gz\n").unwrap();

        assert!(is_lfs_tracked(&temp_dir, Path::new("sub/dir/image.dcm")));
        assert!(is_lfs_tracked(&temp_dir, Path::new("volume.nii.gz")));
        assert!(!is_lfs_tracked(&temp_dir, Path::new("meta.json")));
        assert!(!is_lfs_tracked(&temp_dir, Path::new("README.md")));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_stage_lfs_and_read_content_for_write() {
        let temp_dir = std::env::temp_dir().join(format!("git_med_lfs_stage_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        crate::core::init::init_at(&temp_dir);

        let sample_data = b"very large medical image binary payload";
        let raw_hash = hash_raw(sample_data);

        // Stage raw bytes
        stage_lfs_bytes(&temp_dir, sample_data, &raw_hash).expect("stage bytes");
        assert!(has_lfs_raw(&temp_dir, &raw_hash));

        // Create LFS pointer object
        let pointer_hash = create_and_store_lfs_pointer(&temp_dir, &raw_hash).expect("create pointer");

        // Verify read_file_content_for_write resolves the pointer to raw bytes
        let resolved = read_file_content_for_write(&temp_dir, &pointer_hash).expect("read content");
        assert_eq!(resolved, sample_data);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

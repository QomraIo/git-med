use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufReader, Read, Result as IoResult};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::files::types::ObjectType;

// ============================================================================
// HASH ALGORITHM CONFIGURATION: Change this one line to switch backends
// ============================================================================
pub type ActiveAlgorithm = Blake3Algorithm;
// e.g., to switch to SHA-256:
// pub type ActiveAlgorithm = Blake3Algorithm;

/// Construct the Git-style object header bytes: `"<type> <len>\0"`.
pub fn object_header(obj_type: ObjectType, byte_len: u64) -> Vec<u8> {
    format!("{} {}\0", obj_type.header_prefix(), byte_len).into_bytes()
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashError {
    InvalidLength { expected: usize, actual: usize },
    InvalidHex(String),
}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLength { expected, actual } => {
                write!(f, "invalid hash length: expected {}, got {}", expected, actual)
            }
            Self::InvalidHex(err) => write!(f, "invalid hex: {}", err),
        }
    }
}

impl std::error::Error for HashError {}

// ============================================================================
// Universal Hash Value Type
// ============================================================================

/// Flexible, stack-allocated content-addressed digest supporting up to 64-byte hashes
/// (e.g. SHA-1 (20B), BLAKE3 (32B), SHA-256 (32B), SHA-512 (64B)) with `Copy` semantics.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hash {
    bytes: [u8; 64],
    len: u8,
}

impl Hash {
    /// Create a `Hash` from a raw byte slice (must be <= 64 bytes).
    pub fn from_bytes(slice: &[u8]) -> Result<Self, HashError> {
        if slice.len() > 64 {
            return Err(HashError::InvalidLength {
                expected: 64,
                actual: slice.len(),
            });
        }
        let mut bytes = [0u8; 64];
        bytes[..slice.len()].copy_from_slice(slice);
        Ok(Self {
            bytes,
            len: slice.len() as u8,
        })
    }

    /// Infallible creation from fixed 32-byte array.
    pub fn from_bytes32(arr: [u8; 32]) -> Self {
        let mut bytes = [0u8; 64];
        bytes[..32].copy_from_slice(&arr);
        Self { bytes, len: 32 }
    }

    /// Return the digest byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Digest length in bytes.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns `true` if length is 0.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Formats the digest as a lowercase hex string.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(self.len as usize * 2);
        for &b in self.as_bytes() {
            let _ = write!(&mut s, "{:02x}", b);
        }
        s
    }

    /// Parse a lowercase or uppercase hex string into a `Hash`.
    pub fn from_hex(hex: &str) -> Result<Self, HashError> {
        let trimmed = hex.trim();
        if trimmed.len() % 2 != 0 {
            return Err(HashError::InvalidHex(format!(
                "odd length hex string: '{}'",
                trimmed
            )));
        }
        let byte_len = trimmed.len() / 2;
        if byte_len > 64 {
            return Err(HashError::InvalidLength {
                expected: 64,
                actual: byte_len,
            });
        }
        let mut bytes = [0u8; 64];
        for i in 0..byte_len {
            bytes[i] = u8::from_str_radix(&trimmed[i * 2..i * 2 + 2], 16)
                .map_err(|e| HashError::InvalidHex(format!("{}: '{}'", e, trimmed)))?;
        }
        Ok(Self {
            bytes,
            len: byte_len as u8,
        })
    }
}

impl Default for Hash {
    fn default() -> Self {
        Self {
            bytes: [0u8; 64],
            len: ActiveAlgorithm::DIGEST_LEN as u8,
        }
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hash({})", self.to_hex())
    }
}

impl From<blake3::Hash> for Hash {
    fn from(b3: blake3::Hash) -> Self {
        Self::from_bytes32(*b3.as_bytes())
    }
}

impl TryFrom<Hash> for blake3::Hash {
    type Error = HashError;

    fn try_from(h: Hash) -> Result<Self, Self::Error> {
        if h.len() != 32 {
            return Err(HashError::InvalidLength {
                expected: 32,
                actual: h.len(),
            });
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(h.as_bytes());
        Ok(blake3::Hash::from_bytes(arr))
    }
}

// ============================================================================
// Hashing Algorithm Abstraction Trait
// ============================================================================

pub trait HashAlgorithm: 'static + Send + Sync {
    const NAME: &'static str;
    const DIGEST_LEN: usize;

    /// Hash arbitrary raw bytes without Git-med object header (for tests and mocking).
    fn hash_raw(bytes: &[u8]) -> Hash;

    /// Hash in-memory payload prepended with Git-style header (`"<type> <len>\0"`).
    fn hash_object_bytes(bytes: &[u8], obj_type: ObjectType) -> Hash;

    /// Hash disk file prepended with Git-style header (supports streaming / mmap / rayon).
    fn hash_file(path: &Path, size: Option<u64>, obj_type: ObjectType) -> IoResult<Hash>;
}

// ============================================================================
// Concrete Algorithm: BLAKE3 (Default)
// ============================================================================

pub struct Blake3Algorithm;

impl Blake3Algorithm {
    pub const NAME: &'static str = "blake3";
    pub const DIGEST_LEN: usize = 32;
}

impl HashAlgorithm for Blake3Algorithm {
    const NAME: &'static str = Self::NAME;
    const DIGEST_LEN: usize = Self::DIGEST_LEN;

    fn hash_raw(bytes: &[u8]) -> Hash {
        blake3::hash(bytes).into()
    }

    fn hash_object_bytes(bytes: &[u8], obj_type: ObjectType) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&object_header(obj_type, bytes.len() as u64));
        hasher.update(bytes);
        hasher.finalize().into()
    }

    fn hash_file(path: &Path, size: Option<u64>, obj_type: ObjectType) -> IoResult<Hash> {
        let byte_len = match size {
            Some(len) => len,
            None => std::fs::metadata(path)?.len(),
        };
        let mut hasher = blake3::Hasher::new();
        hasher.update(&object_header(obj_type, byte_len));
        hasher.update_mmap_rayon(path)?;
        Ok(hasher.finalize().into())
    }
}

// ============================================================================
// Concrete Algorithm: SHA-256
// ============================================================================

pub struct Sha256Algorithm;

impl Sha256Algorithm {
    pub const NAME: &'static str = "sha256";
    pub const DIGEST_LEN: usize = 32;
}

impl HashAlgorithm for Sha256Algorithm {
    const NAME: &'static str = Self::NAME;
    const DIGEST_LEN: usize = Self::DIGEST_LEN;

    fn hash_raw(bytes: &[u8]) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Hash::from_bytes(&hasher.finalize()).expect("sha256 fits in 64 bytes")
    }

    fn hash_object_bytes(bytes: &[u8], obj_type: ObjectType) -> Hash {
        let mut hasher = Sha256::new();
        hasher.update(&object_header(obj_type, bytes.len() as u64));
        hasher.update(bytes);
        Hash::from_bytes(&hasher.finalize()).expect("sha256 fits in 64 bytes")
    }

    fn hash_file(path: &Path, size: Option<u64>, obj_type: ObjectType) -> IoResult<Hash> {
        let byte_len = match size {
            Some(len) => len,
            None => std::fs::metadata(path)?.len(),
        };
        let mut hasher = Sha256::new();
        hasher.update(&object_header(obj_type, byte_len));

        let file = File::open(path)?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(Hash::from_bytes(&hasher.finalize()).expect("sha256 fits in 64 bytes"))
    }
}

// ============================================================================
// Global Forwarding Functions
// ============================================================================

/// Compute the Git-style single-pass object hash (`"<type> <len>\0" || file content`).
pub fn hash(path: &Path, size: Option<u64>, obj_type: ObjectType) -> Hash {
    ActiveAlgorithm::hash_file(path, size, obj_type).expect("Failed to hash file")
}

/// Compute the Git-style single-pass object hash from in-memory bytes (`"<type> <len>\0" || bytes`).
pub fn hash_bytes(bytes: &[u8], obj_type: ObjectType) -> Hash {
    ActiveAlgorithm::hash_object_bytes(bytes, obj_type)
}

/// Compute raw hash from arbitrary in-memory bytes without object header (useful for tests and mock nodes).
pub fn hash_raw(bytes: &[u8]) -> Hash {
    ActiveAlgorithm::hash_raw(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_roundtrip_hex_and_bytes() {
        let data = b"hello git-med abstract hashing";
        let h1 = hash_raw(data);
        assert_eq!(h1.len(), 32);

        let hex = h1.to_hex();
        assert_eq!(hex.len(), 64);

        let h2 = Hash::from_hex(&hex).expect("parse valid hex");
        assert_eq!(h1, h2);
        assert_eq!(h1.as_bytes(), h2.as_bytes());

        let h3 = Hash::from_bytes(h1.as_bytes()).expect("parse bytes");
        assert_eq!(h1, h3);
    }

    #[test]
    fn test_blake3_and_sha256_both_work() {
        let payload = b"content to compare algorithms";
        let b3 = Blake3Algorithm::hash_raw(payload);
        let s256 = Sha256Algorithm::hash_raw(payload);

        assert_eq!(b3.len(), 32);
        assert_eq!(s256.len(), 32);
        assert_ne!(b3, s256, "different algorithms must produce different hashes");
    }

    #[test]
    fn test_hash_object_includes_header() {
        let payload = b"blob data";
        let obj_h = Blake3Algorithm::hash_object_bytes(payload, ObjectType::Blob);
        let raw_h = Blake3Algorithm::hash_raw(payload);
        assert_ne!(obj_h, raw_h);
    }
}

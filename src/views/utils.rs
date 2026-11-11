use std::fs::symlink_metadata;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{os::unix::ffi::OsStrExt, path::Path};

use crate::core::tree::NodeRef;
use crate::files::types::FileMetadata;
use crate::files::utils::hash;

pub fn get_name(opt_path: &Option<&Path>) -> Option<[u8; 16]> {
    opt_path
        .and_then(|p| p.file_name())
        .map(|name| bytes2buf128(name.as_bytes()))
}

pub fn string2buf128(text: &str) -> [u8; 16] {
    let bytes = text.as_bytes();
    bytes2buf128(bytes)
}

pub fn bytes2buf128(bytes: &[u8]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let len = bytes.len().min(16);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

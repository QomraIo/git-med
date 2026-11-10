use std::fs::symlink_metadata;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{os::unix::ffi::OsStrExt, path::Path};

use crate::views::types::FileMetadata;

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

fn time_format(time_: SystemTime) -> (u64, u32) {
    let duration = time_
        .duration_since(UNIX_EPOCH)
        .expect("SystemTime is set before UNIX EPOCH");

    let secs: u64 = duration.as_secs();
    let subsec_nanos: u32 = duration.subsec_nanos();
    (secs, subsec_nanos)
}
fn get_times(path: &Path) -> FileMetadata {
    let metadata = symlink_metadata(path).unwrap();

    let atime = metadata.accessed().unwrap();
    let mtime = metadata.modified().unwrap();

    let size = metadata.len();

    FileMetadata { atime, mtime, size }
}

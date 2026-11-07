use std::fs;
use std::os::unix::fs::MetadataExt;

fn metadata(path: &Path) -> std::io::Result<()> {
    // Executes the lstat() syscall (does not follow symlinks)
    let meta = fs::symlink_metadata(path)?;

    // Cast to u32 to match the Git index binary format
    let ctime_sec = meta.ctime() as u32;
    let ctime_nsec = meta.ctime_nsec() as u32;

    let mtime_sec = meta.mtime() as u32;
    let mtime_nsec = meta.mtime_nsec() as u32;

    // Git stores size as a 32-bit unsigned integer (truncated if > 4GB)
    let size = meta.size() as u32;
}

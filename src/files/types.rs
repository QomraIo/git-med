use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

/// Kind of content-addressed object stored under `.git-med/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Blob,
    Meta,
    Tree,
    Commit,
}

impl ObjectType {
    /// Returns the object header prefix (`"blob"`, `"meta"`, `"tree"`, or `"commit"`).
    pub fn header_prefix(self) -> &'static str {
        match self {
            Self::Blob => "blob",
            Self::Meta => "meta",
            Self::Tree => "tree",
            Self::Commit => "commit",
        }
    }

    /// Returns the object store directory name under `.git-med/`.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Blob | Self::Tree | Self::Commit => "objects",
            Self::Meta => "meta_objects",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FileMetadata {
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub size: Option<u64>,
}

impl Ord for FileMetadata {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.atime, self.mtime).cmp(&(other.atime, other.mtime))
    }
}

impl PartialOrd for FileMetadata {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for FileMetadata {
    fn eq(&self, other: &Self) -> bool {
        (self.atime, self.mtime) == (other.atime, other.mtime)
    }
}

impl Eq for FileMetadata {}

impl Default for FileMetadata {
    fn default() -> Self {
        Self {
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            size: None,
        }
    }
}

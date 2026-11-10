use std::cmp::Ordering;
use std::fs::FileType;
use std::time::SystemTime;

pub struct ViewAttributes<'a> {
    pub subject: &'a str,
    pub study: &'a str,
    pub series: &'a str,
    pub instance: &'a str,

    pub source: &'a str,
    pub task: &'a str,
}

pub struct ViewAttributesIndex {
    pub subject: Option<[u8; 16]>,
    pub study: Option<[u8; 16]>,
    pub series: Option<[u8; 16]>,
    pub instance: Option<[u8; 16]>,

    pub source: Option<[u8; 16]>,
    pub task: Option<[u8; 16]>,
}

#[derive(Debug, Copy, Clone)]
pub struct FileMetadata {
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub size: u64,
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

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::files::types::FileMetadata;

// Type aliases make the signatures much cleaner and easier to read
pub type NodeRef = Rc<RefCell<Node>>;
pub type WeakNodeRef = Weak<RefCell<Node>>;

pub enum Children {
    Attributes(BTreeMap<[u8; 16], NodeRef>), // attribute has other attributes
    Files(BTreeMap<[u8; 16], NodeRef>),      // attribute has files
    None,                                    // file has no sub
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Children::Attributes(map) => f
                .debug_struct("Attributes")
                .field("count", &map.len())
                .finish(),
            Children::Files(map) => {
                f.debug_struct("Files").field("count", &map.len()).finish()
            }
            Children::None => write!(f, "None"),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub type NodeMetadata = FileMetadata;

impl Default for NodeMetadata {
    fn default() -> Self {
        Self {
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            size: None,
        }
    }
}

pub struct Node {
    pub id: [u8; 16],

    pub children: Children,
    pub parent: Option<WeakNodeRef>, // root has no parent

    pub visited: bool,
    pub valid_child_hash: bool,
    pub valid_self_hash: bool,

    pub node_metadata: NodeMetadata,
    pub meta_metadata: NodeMetadata,

    pub draft_meta_metadata: NodeMetadata,
    pub draft_node_metadata: NodeMetadata,

    pub self_hash: String,
    pub meta_hash: String,
    pub child_hash: Option<String>, //  leaf node (file) has no child
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("children", &self.children)
            .field("visited", &self.visited)
            .field("valid_child_hash", &self.valid_child_hash)
            .field("valid_self_hash", &self.valid_self_hash)
            .field("self_hash", &self.self_hash)
            .field("meta_hash", &self.meta_hash)
            .field("child_hash", &self.child_hash)
            .finish()
    }
}

pub enum ChildType {
    ID([u8; 16]),
    REF(NodeRef),
}

impl Node {
    /// Create a new detached node
    pub fn new(id: [u8; 16]) -> NodeRef {
        Rc::new(RefCell::new(Node {
            id,
            children: Children::None,
            parent: None,
            visited: false,
            valid_child_hash: true,
            valid_self_hash: true,
            node_metadata: NodeMetadata::default(),
            meta_metadata: NodeMetadata::default(),
            draft_meta_metadata: NodeMetadata::default(),
            draft_node_metadata: NodeMetadata::default(),
            self_hash: String::new(),
            meta_hash: String::new(),
            child_hash: None,
        }))
    }

    /// Access parent safely by upgrading the Weak pointer to an Rc
    pub fn parent(&self) -> Option<NodeRef> {
        self.parent.as_ref().and_then(|weak| weak.upgrade())
    }

    pub fn insert(&mut self, parent_ref: &NodeRef, child: ChildType) -> Result<NodeRef, ()> {
        let child = match child {
            ChildType::ID(id) => Node::new(id),
            ChildType::REF(child) => child,
        };

        // 1. Set child's parent to a Weak pointer of this parent
        child.borrow_mut().parent = Some(Rc::downgrade(parent_ref));

        // 2. Insert strong reference into parent's children map
        match &mut self.children {
            Children::Attributes(map) | Children::Files(map) => {
                map.insert(child.borrow().id, Rc::clone(&child));
                Ok(child)
            }
            Children::None => Err(()),
        }
    }

    /// Save this node and its entire subtree to a file at `path`
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), TreeError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let tmp_file_name = format!(
            ".{}.tmp.{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        );
        let tmp_path = path.with_file_name(tmp_file_name);
        {
            let file = File::create(&tmp_path)?;
            let mut writer = BufWriter::new(file);
            write_header(&mut writer)?;
            serialize_node_internal(self, &mut writer)?;
            writer.flush()?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Load a tree from a file at `path` and return its root node
    pub fn load<P: AsRef<Path>>(path: P) -> Result<NodeRef, TreeError> {
        let tree = Tree::load(path)?;
        Ok(tree.root)
    }

    /// Write this node and its subtree without file headers
    pub fn write_subtree<W: Write>(&self, writer: &mut W) -> Result<(), TreeError> {
        serialize_node_internal(self, writer)
    }

    /// Load a subtree from a stream without file headers
    pub fn load_subtree<R: Read>(
        reader: &mut R,
        parent: Option<&NodeRef>,
    ) -> Result<NodeRef, TreeError> {
        deserialize_node(reader, parent)
    }
}

pub fn get_or_insert_child(parent_ref: &NodeRef, id: [u8; 16]) -> Result<NodeRef, ()> {
    let mut parent = parent_ref.borrow_mut();
    match &parent.children {
        Children::Attributes(map) | Children::Files(map) => match map.get(&id) {
            Some(node) => Ok(Rc::clone(node)),
            None => parent.insert(parent_ref, ChildType::ID(id)),
        },
        Children::None => Err(()),
    }
}

// ============================================================================
// Tree Wrapper & High-Level Persistence
// ============================================================================

#[derive(Clone)]
pub struct Tree {
    pub root: NodeRef,
}

impl Tree {
    /// Create a new tree with a given root ID
    pub fn new(root_id: [u8; 16]) -> Self {
        Self {
            root: Node::new(root_id),
        }
    }

    /// Wrap an existing root node in a Tree
    pub fn from_root(root: NodeRef) -> Self {
        Self { root }
    }

    /// Save the entire tree to a file at `path` atomically using a temporary file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), TreeError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let tmp_file_name = format!(
            ".{}.tmp.{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        );
        let tmp_path = path.with_file_name(tmp_file_name);
        {
            let file = File::create(&tmp_path)?;
            let mut writer = BufWriter::new(file);
            self.write_to(&mut writer)?;
            writer.flush()?;
        }
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Load a tree from a file at `path`
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, TreeError> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        Self::read_from(&mut reader)
    }

    /// Write binary tree data to any Write stream
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<(), TreeError> {
        write_header(writer)?;
        serialize_node(&self.root, writer)?;
        Ok(())
    }

    /// Read binary tree data from any Read stream
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, TreeError> {
        read_header(reader)?;
        let root = deserialize_node(reader, None)?;
        Ok(Self { root })
    }
}

// ============================================================================
// Serialization Error Handling
// ============================================================================

#[derive(Debug)]
pub enum TreeError {
    Io(std::io::Error),
    InvalidMagic,
    UnsupportedVersion(u32),
    CorruptedData(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::Io(e) => write!(f, "I/O error: {}", e),
            TreeError::InvalidMagic => write!(f, "Invalid file format signature (magic bytes)"),
            TreeError::UnsupportedVersion(v) => write!(f, "Unsupported tree format version: {}", v),
            TreeError::CorruptedData(msg) => write!(f, "Corrupted tree data: {}", msg),
        }
    }
}

impl std::error::Error for TreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TreeError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for TreeError {
    fn from(err: std::io::Error) -> Self {
        TreeError::Io(err)
    }
}

// ============================================================================
// Binary Protocol Implementation
// ============================================================================

pub const TREE_MAGIC: &[u8; 8] = b"GMEDTR01";
pub const TREE_VERSION: u32 = 1;

fn write_header<W: Write>(writer: &mut W) -> Result<(), TreeError> {
    writer.write_all(TREE_MAGIC)?;
    writer.write_all(&TREE_VERSION.to_be_bytes())?;
    Ok(())
}

fn read_header<R: Read>(reader: &mut R) -> Result<(), TreeError> {
    let mut magic_buf = [0u8; 8];
    reader.read_exact(&mut magic_buf)?;
    if &magic_buf != TREE_MAGIC {
        return Err(TreeError::InvalidMagic);
    }

    let mut version_buf = [0u8; 4];
    reader.read_exact(&mut version_buf)?;
    let version = u32::from_be_bytes(version_buf);
    if version != TREE_VERSION {
        return Err(TreeError::UnsupportedVersion(version));
    }

    Ok(())
}

fn system_time_to_secs_nanos(time: SystemTime) -> (u64, u32) {
    match time.duration_since(UNIX_EPOCH) {
        Ok(dur) => (dur.as_secs(), dur.subsec_nanos()),
        Err(_) => (0, 0),
    }
}

fn secs_nanos_to_system_time(secs: u64, nanos: u32) -> SystemTime {
    UNIX_EPOCH + Duration::new(secs, nanos)
}

fn write_file_metadata<W: Write>(meta: &FileMetadata, writer: &mut W) -> std::io::Result<()> {
    let (a_sec, a_nsec) = system_time_to_secs_nanos(meta.atime);
    writer.write_all(&a_sec.to_be_bytes())?;
    writer.write_all(&a_nsec.to_be_bytes())?;

    let (m_sec, m_nsec) = system_time_to_secs_nanos(meta.mtime);
    writer.write_all(&m_sec.to_be_bytes())?;
    writer.write_all(&m_nsec.to_be_bytes())?;

    match meta.size {
        Some(sz) => {
            writer.write_all(&[1u8])?;
            writer.write_all(&sz.to_be_bytes())?;
        }
        None => {
            writer.write_all(&[0u8])?;
        }
    }
    Ok(())
}

fn read_file_metadata<R: Read>(reader: &mut R) -> Result<FileMetadata, TreeError> {
    let mut u64_buf = [0u8; 8];
    let mut u32_buf = [0u8; 4];
    let mut u8_buf = [0u8; 1];

    reader.read_exact(&mut u64_buf)?;
    let a_sec = u64::from_be_bytes(u64_buf);
    reader.read_exact(&mut u32_buf)?;
    let a_nsec = u32::from_be_bytes(u32_buf);

    reader.read_exact(&mut u64_buf)?;
    let m_sec = u64::from_be_bytes(u64_buf);
    reader.read_exact(&mut u32_buf)?;
    let m_nsec = u32::from_be_bytes(u32_buf);

    reader.read_exact(&mut u8_buf)?;
    let size = if u8_buf[0] == 1 {
        reader.read_exact(&mut u64_buf)?;
        Some(u64::from_be_bytes(u64_buf))
    } else {
        None
    };

    Ok(FileMetadata {
        atime: secs_nanos_to_system_time(a_sec, a_nsec),
        mtime: secs_nanos_to_system_time(m_sec, m_nsec),
        size,
    })
}

fn write_string<W: Write>(s: &str, writer: &mut W) -> std::io::Result<()> {
    let bytes = s.as_bytes();
    let len = bytes.len() as u16;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(bytes)?;
    Ok(())
}

fn read_string<R: Read>(reader: &mut R) -> Result<String, TreeError> {
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf)?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut str_buf = vec![0u8; len];
    reader.read_exact(&mut str_buf)?;
    String::from_utf8(str_buf)
        .map_err(|e| TreeError::CorruptedData(format!("Invalid UTF-8 string: {}", e)))
}

fn serialize_node<W: Write>(node_ref: &NodeRef, writer: &mut W) -> Result<(), TreeError> {
    let node = node_ref.borrow();
    serialize_node_internal(&node, writer)
}

fn serialize_node_internal<W: Write>(node: &Node, writer: &mut W) -> Result<(), TreeError> {
    // 1. Write id (16 bytes)
    writer.write_all(&node.id)?;

    // 2. Write flags (1 byte)
    let mut flags = 0u8;
    if node.visited {
        flags |= 1 << 0;
    }
    if node.valid_child_hash {
        flags |= 1 << 1;
    }
    if node.valid_self_hash {
        flags |= 1 << 2;
    }
    if node.child_hash.is_some() {
        flags |= 1 << 3;
    }
    writer.write_all(&[flags])?;

    // 3. Write hashes
    write_string(&node.self_hash, writer)?;
    write_string(&node.meta_hash, writer)?;
    if let Some(ref ch) = node.child_hash {
        write_string(ch, writer)?;
    }

    // 4. Write metadata blocks
    write_file_metadata(&node.node_metadata, writer)?;
    write_file_metadata(&node.meta_metadata, writer)?;
    write_file_metadata(&node.draft_node_metadata, writer)?;
    write_file_metadata(&node.draft_meta_metadata, writer)?;

    // 5. Write children
    match &node.children {
        Children::None => {
            writer.write_all(&[0u8])?;
        }
        Children::Attributes(map) => {
            writer.write_all(&[1u8])?;
            let count = map.len() as u32;
            writer.write_all(&count.to_be_bytes())?;
            for (_child_id, child_ref) in map {
                serialize_node(child_ref, writer)?;
            }
        }
        Children::Files(map) => {
            writer.write_all(&[2u8])?;
            let count = map.len() as u32;
            writer.write_all(&count.to_be_bytes())?;
            for (_child_id, child_ref) in map {
                serialize_node(child_ref, writer)?;
            }
        }
    }

    Ok(())
}

fn deserialize_node<R: Read>(
    reader: &mut R,
    parent: Option<&NodeRef>,
) -> Result<NodeRef, TreeError> {
    // 1. Read id
    let mut id = [0u8; 16];
    reader.read_exact(&mut id)?;

    // 2. Read flags
    let mut flags_buf = [0u8; 1];
    reader.read_exact(&mut flags_buf)?;
    let flags = flags_buf[0];
    let visited = (flags & (1 << 0)) != 0;
    let valid_child_hash = (flags & (1 << 1)) != 0;
    let valid_self_hash = (flags & (1 << 2)) != 0;
    let has_child_hash = (flags & (1 << 3)) != 0;

    // 3. Read hashes
    let self_hash = read_string(reader)?;
    let meta_hash = read_string(reader)?;
    let child_hash = if has_child_hash {
        Some(read_string(reader)?)
    } else {
        None
    };

    // 4. Read metadata blocks
    let node_metadata = read_file_metadata(reader)?;
    let meta_metadata = read_file_metadata(reader)?;
    let draft_node_metadata = read_file_metadata(reader)?;
    let draft_meta_metadata = read_file_metadata(reader)?;

    // Create current node
    let node_ref = Node::new(id);
    if let Some(p) = parent {
        node_ref.borrow_mut().parent = Some(Rc::downgrade(p));
    }

    // 5. Read children
    let mut disc_buf = [0u8; 1];
    reader.read_exact(&mut disc_buf)?;
    let children = match disc_buf[0] {
        0 => Children::None,
        1 => {
            let mut count_buf = [0u8; 4];
            reader.read_exact(&mut count_buf)?;
            let count = u32::from_be_bytes(count_buf) as usize;
            let mut map = BTreeMap::new();
            for _ in 0..count {
                let child = deserialize_node(reader, Some(&node_ref))?;
                let child_id = child.borrow().id;
                map.insert(child_id, child);
            }
            Children::Attributes(map)
        }
        2 => {
            let mut count_buf = [0u8; 4];
            reader.read_exact(&mut count_buf)?;
            let count = u32::from_be_bytes(count_buf) as usize;
            let mut map = BTreeMap::new();
            for _ in 0..count {
                let child = deserialize_node(reader, Some(&node_ref))?;
                let child_id = child.borrow().id;
                map.insert(child_id, child);
            }
            Children::Files(map)
        }
        other => {
            return Err(TreeError::CorruptedData(format!(
                "Invalid children discriminant: {}",
                other
            )));
        }
    };

    {
        let mut node = node_ref.borrow_mut();
        node.children = children;
        node.visited = visited;
        node.valid_child_hash = valid_child_hash;
        node.valid_self_hash = valid_self_hash;
        node.self_hash = self_hash;
        node.meta_hash = meta_hash;
        node.child_hash = child_hash;
        node.node_metadata = node_metadata;
        node.meta_metadata = meta_metadata;
        node.draft_node_metadata = draft_node_metadata;
        node.draft_meta_metadata = draft_meta_metadata;
    }

    Ok(node_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_node_save_and_load_roundtrip() {
        let node_id = [1u8; 16];
        let node = Node::new(node_id);
        {
            let mut n = node.borrow_mut();
            n.self_hash = "self_hash_123".to_string();
            n.meta_hash = "meta_hash_456".to_string();
            n.child_hash = Some("child_hash_789".to_string());
            n.visited = true;
            n.valid_self_hash = false;
            n.node_metadata.size = Some(4096);
        }

        let mut buffer = Vec::new();
        let tree = Tree::from_root(Rc::clone(&node));
        tree.write_to(&mut buffer).expect("Serialization failed");

        let loaded_tree = Tree::read_from(&mut &buffer[..]).expect("Deserialization failed");
        let loaded_node = loaded_tree.root.borrow();

        assert_eq!(loaded_node.id, node_id);
        assert_eq!(loaded_node.self_hash, "self_hash_123");
        assert_eq!(loaded_node.meta_hash, "meta_hash_456");
        assert_eq!(loaded_node.child_hash.as_deref(), Some("child_hash_789"));
        assert!(loaded_node.visited);
        assert!(!loaded_node.valid_self_hash);
        assert_eq!(loaded_node.node_metadata.size, Some(4096));
        assert!(matches!(loaded_node.children, Children::None));
        assert!(loaded_node.parent.is_none());
    }

    #[test]
    fn test_tree_hierarchy_and_parent_reconstruction() {
        let root = Node::new([0u8; 16]);
        root.borrow_mut().children = Children::Attributes(BTreeMap::new());

        let task_id = [10u8; 16];
        let task_node = root
            .borrow_mut()
            .insert(&root, ChildType::ID(task_id))
            .expect("Failed to insert task");
        task_node.borrow_mut().children = Children::Files(BTreeMap::new());

        let file_id = [20u8; 16];
        let _file_node = task_node
            .borrow_mut()
            .insert(&task_node, ChildType::ID(file_id))
            .expect("Failed to insert file");

        let tree = Tree::from_root(Rc::clone(&root));
        let mut buffer = Vec::new();
        tree.write_to(&mut buffer).expect("Write to buffer failed");

        let restored_tree = Tree::read_from(&mut &buffer[..]).expect("Read from buffer failed");
        let restored_root = restored_tree.root;

        assert_eq!(restored_root.borrow().id, [0u8; 16]);
        assert!(restored_root.borrow().parent().is_none());

        let restored_task = match &restored_root.borrow().children {
            Children::Attributes(map) => Rc::clone(map.get(&task_id).expect("task node missing")),
            _ => panic!("Expected Children::Attributes on root"),
        };

        // Verify parent link from task to root
        let task_parent = restored_task.borrow().parent().expect("task has no parent");
        assert_eq!(task_parent.borrow().id, [0u8; 16]);

        let restored_file = match &restored_task.borrow().children {
            Children::Files(map) => Rc::clone(map.get(&file_id).expect("file node missing")),
            _ => panic!("Expected Children::Files on task"),
        };

        // Verify parent link from file to task
        let file_parent = restored_file.borrow().parent().expect("file has no parent");
        assert_eq!(file_parent.borrow().id, task_id);
    }

    #[test]
    fn test_file_save_and_load() {
        let root = Node::new([99u8; 16]);
        let temp_dir = std::env::temp_dir().join(format!("git_med_test_{}", std::process::id()));
        let file_path = temp_dir.join("test_tree.bin");

        let tree = Tree::from_root(root);
        tree.save(&file_path).expect("Failed to save tree to file");

        let loaded = Tree::load(&file_path).expect("Failed to load tree from file");
        assert_eq!(loaded.root.borrow().id, [99u8; 16]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_invalid_magic() {
        let invalid_data = b"NOTVALID";
        let result = Tree::read_from(&mut &invalid_data[..]);
        assert!(matches!(result, Err(TreeError::InvalidMagic)));
    }
}

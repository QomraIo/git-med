// save, load and tests are vibed. I'm thinking about changing the logic soon

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::files::hash::{ActiveAlgorithm, Hash};

use crate::files::commit::{save_tree, save_tree_in};
use crate::files::types::FileMetadata;

// Type aliases make the signatures much cleaner and easier to read
pub type NodeRef = Rc<RefCell<Node>>;
pub type WeakNodeRef = Weak<RefCell<Node>>;

/// Selector for initializing a node's child container without allocating upfront.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildrenType {
    Attributes,
    Files,
    None,
}

pub enum Children {
    Attributes(BTreeMap<String, NodeRef>), // attribute has other attributes
    Files(BTreeMap<String, NodeRef>),      // attribute has files
    None,                                  // file has no sub
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Children::Attributes(map) => f
                .debug_struct("Attributes")
                .field("count", &map.len())
                .finish(),
            Children::Files(map) => f.debug_struct("Files").field("count", &map.len()).finish(),
            Children::None => write!(f, "None"),
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub type NodeMetadata = FileMetadata;

pub struct Node {
    pub id: String,

    pub children: Children,
    pub meta: BTreeMap<String, NodeRef>,
    pub parent: Option<WeakNodeRef>, // root has no parent

    pub visited: bool,
    pub valid_child_hash: bool,
    pub valid_self_hash: bool,

    pub node_metadata: NodeMetadata,
    pub meta_metadata: NodeMetadata,

    pub draft_meta_metadata: Option<NodeMetadata>, // some views need to make a draft to collect info from multiple files
    pub draft_node_metadata: Option<NodeMetadata>,

    pub self_hash: Option<Hash>,
    pub meta_hash: Option<Hash>,
    pub child_hash: Option<Hash>, //  leaf node (file) has no child
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("children", &self.children)
            .field("meta_count", &self.meta.len())
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
    Id(String),
    Ref(NodeRef),
}

impl Node {
    /// Create a new detached node with the specified child container type.
    pub fn new(id: impl Into<String>, children_type: ChildrenType) -> NodeRef {
        let children = match children_type {
            ChildrenType::Attributes => Children::Attributes(BTreeMap::new()),
            ChildrenType::Files => Children::Files(BTreeMap::new()),
            ChildrenType::None => Children::None,
        };
        Rc::new(RefCell::new(Node {
            id: id.into(),
            children,
            meta: BTreeMap::new(),
            parent: None,
            visited: false,
            valid_child_hash: false,
            valid_self_hash: false,
            node_metadata: NodeMetadata::default(),
            meta_metadata: NodeMetadata::default(),
            draft_meta_metadata: None,
            draft_node_metadata: None,
            self_hash: None,
            meta_hash: None,
            child_hash: None,
        }))
    }

    /// Access parent safely by upgrading the Weak pointer to an Rc
    pub fn parent(&self) -> Option<NodeRef> {
        self.parent.as_ref().and_then(|weak| weak.upgrade())
    }

    /// Mark `valid_child_hash` as `false` for all ancestor nodes up to the root
    pub fn invalidate_ancestors_hash(&self) {
        let mut current = self.parent();
        while let Some(parent_ref) = current {
            let mut parent = parent_ref.borrow_mut();
            parent.valid_child_hash = false;
            current = parent.parent();
        }
    }

    /// Insert a child node (or create one from an ID) under `parent_ref`.
    pub fn insert(parent_ref: &NodeRef, child: ChildType) -> Result<NodeRef, ()> {
        if let ChildType::Ref(ref child_ref) = child {
            // Prevent self-insertion or ancestor cycles before mutating links.
            let mut ancestor = Some(Rc::clone(parent_ref));
            while let Some(curr) = ancestor {
                if Rc::ptr_eq(&curr, child_ref) {
                    return Err(());
                }
                ancestor = curr.borrow().parent();
            }
        }

        let mut parent = parent_ref.borrow_mut();
        let map = match &mut parent.children {
            Children::Attributes(map) | Children::Files(map) => map,
            Children::None => return Err(()),
        };

        let child_ref = match child {
            ChildType::Id(id) => Node::new(id, ChildrenType::None),
            ChildType::Ref(node_ref) => node_ref,
        };

        let child_id = {
            let mut child_node = child_ref.borrow_mut();
            child_node.parent = Some(Rc::downgrade(parent_ref));
            child_node.id.clone()
        };

        if let Some(old_child) = map.insert(child_id, Rc::clone(&child_ref)) {
            if !Rc::ptr_eq(&old_child, &child_ref) {
                old_child.borrow_mut().parent = None;
            }
        }

        Ok(child_ref)
    }

    /// Save this node and its entire subtree directly to `path`.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), TreeError> {
        save_to_file(path, |writer| {
            write_header(writer)?;
            serialize_node_internal(self, writer, 0)
        })
    }

    /// Load a tree from a file at `path` and return its root node
    pub fn load<P: AsRef<Path>>(path: P) -> Result<NodeRef, TreeError> {
        let tree = Tree::load(path)?;
        Ok(tree.root)
    }

    /// Write this node and its subtree without file headers
    pub fn write_subtree<W: Write>(&self, writer: &mut W) -> Result<(), TreeError> {
        serialize_node_internal(self, writer, 0)
    }

    /// Load a subtree from a stream without file headers
    pub fn load_subtree<R: Read>(
        reader: &mut R,
        parent: Option<&NodeRef>,
    ) -> Result<NodeRef, TreeError> {
        deserialize_node(reader, parent, 0)
    }

    /// Reset `visited` to `false` across this node, its `meta` entries, and its subtree.
    pub fn reset_visited(&mut self) {
        self.visited = false;
        for meta_ref in self.meta.values() {
            meta_ref.borrow_mut().reset_visited();
        }
        match &self.children {
            Children::Attributes(map) | Children::Files(map) => {
                for child_ref in map.values() {
                    child_ref.borrow_mut().reset_visited();
                }
            }
            Children::None => {}
        }
    }

    /// Recursively prune unvisited entries from `meta` and `children`.
    /// Returns `true` if any entry in `meta` or `children` (at any depth) was removed.
    pub fn prune_unvisited(&mut self) -> bool {
        let mut meta_pruned = false;
        self.meta.retain(|_, meta_ref| {
            let keep = meta_ref.borrow().visited;
            if !keep {
                meta_ref.borrow_mut().parent = None;
                meta_pruned = true;
            }
            keep
        });
        if meta_pruned {
            self.valid_self_hash = false;
        }

        let mut child_pruned = false;
        match &mut self.children {
            Children::Attributes(map) | Children::Files(map) => {
                map.retain(|_, child_ref| {
                    let (keep, descendant_changed) = {
                        let mut child = child_ref.borrow_mut();
                        let changed = child.prune_unvisited();
                        (child.visited, changed)
                    };
                    if descendant_changed {
                        child_pruned = true;
                    }
                    if !keep {
                        child_ref.borrow_mut().parent = None;
                        child_pruned = true;
                    }
                    keep
                });
            }
            Children::None => {}
        }
        if child_pruned {
            self.valid_child_hash = false;
        }

        child_pruned || meta_pruned
    }
}

fn get_or_insert_in_map(
    parent_ref: &NodeRef,
    map: &mut BTreeMap<String, NodeRef>,
    id: &str,
    children_type: ChildrenType,
) -> NodeRef {
    if let Some(node) = map.get(id) {
        return Rc::clone(node);
    }
    let child = Node::new(id, children_type);
    child.borrow_mut().parent = Some(Rc::downgrade(parent_ref));
    map.insert(id.to_string(), Rc::clone(&child));
    child
}

/// Look up a child by `id` under `parent_ref`, or insert a new node with `children_type`.
pub fn get_or_insert_child(
    parent_ref: &NodeRef,
    id: &str,
    children_type: ChildrenType,
) -> Result<NodeRef, ()> {
    let mut parent = parent_ref.borrow_mut();
    match &mut parent.children {
        Children::Attributes(map) | Children::Files(map) => {
            Ok(get_or_insert_in_map(parent_ref, map, id, children_type))
        }
        Children::None => Err(()),
    }
}

/// Look up a metadata file node by `id` in `parent_ref.meta`, or insert a leaf node if absent.
pub fn get_or_insert_meta(parent_ref: &NodeRef, id: &str) -> NodeRef {
    let mut parent = parent_ref.borrow_mut();
    get_or_insert_in_map(parent_ref, &mut parent.meta, id, ChildrenType::None)
}

fn save_to_file<P, F>(path: P, write_fn: F) -> Result<(), TreeError>
where
    P: AsRef<Path>,
    F: FnOnce(&mut BufWriter<File>) -> Result<(), TreeError>,
{
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    // Write directly to the target path without temp files or rename.
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    write_fn(&mut writer)?;
    writer.flush()?;
    writer
        .into_inner()
        .map_err(std::io::IntoInnerError::into_error)?
        .sync_all()?;
    Ok(())
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
    pub fn new(root_id: impl Into<String>) -> Self {
        Self {
            root: Node::new(root_id.into(), ChildrenType::Attributes),
        }
    }

    /// Wrap an existing root node in a Tree
    pub fn from_root(root: NodeRef) -> Self {
        Self { root }
    }

    /// Save the entire tree directly to `path`.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), TreeError> {
        save_to_file(path, |writer| self.write_to(writer))
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
        serialize_node(&self.root, writer, 0)?;
        Ok(())
    }

    /// Read binary tree data from any Read stream
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self, TreeError> {
        read_header(reader)?;
        let root = deserialize_node(reader, None, 0)?;
        let mut trailing = [0u8; 1];
        loop {
            match reader.read(&mut trailing) {
                Ok(0) => break,
                Ok(_) => {
                    return Err(TreeError::CorruptedData(
                        "unexpected trailing bytes after tree data".to_string(),
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(Self { root })
    }

    /// Reset `visited` to `false` across every node and metadata entry in the tree.
    pub fn reset_visited(&self) {
        self.root.borrow_mut().reset_visited();
    }

    /// Prune all unvisited `meta` and `children` nodes from the tree.
    pub fn prune_unvisited(&self) -> bool {
        self.root.borrow_mut().prune_unvisited()
    }

    pub fn hash(&self) -> Hash {
        save_tree(self.root.clone()).unwrap()
    }

    pub fn hash_in(&self, repo_root: &Path) -> Result<Hash, String> {
        save_tree_in(self.root.clone(), repo_root)
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
const MAX_TREE_DEPTH: usize = 128;

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

// Pre-1970 timestamps clamp to UNIX_EPOCH (0, 0).
fn system_time_to_secs_nanos(time: SystemTime) -> (u64, u32) {
    match time.duration_since(UNIX_EPOCH) {
        Ok(dur) => (dur.as_secs(), dur.subsec_nanos()),
        Err(_) => (0, 0),
    }
}

fn secs_nanos_to_system_time(secs: u64, nanos: u32) -> Result<SystemTime, TreeError> {
    if nanos >= 1_000_000_000 {
        return Err(TreeError::CorruptedData(format!(
            "invalid timestamp nanoseconds: {}",
            nanos
        )));
    }
    UNIX_EPOCH
        .checked_add(Duration::new(secs, nanos))
        .ok_or_else(|| {
            TreeError::CorruptedData(format!(
                "timestamp out of range: secs={}, nanos={}",
                secs, nanos
            ))
        })
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
    let size = match u8_buf[0] {
        0 => None,
        1 => {
            reader.read_exact(&mut u64_buf)?;
            Some(u64::from_be_bytes(u64_buf))
        }
        other => {
            return Err(TreeError::CorruptedData(format!(
                "invalid size tag: {}",
                other
            )));
        }
    };

    Ok(FileMetadata {
        atime: secs_nanos_to_system_time(a_sec, a_nsec)?,
        mtime: secs_nanos_to_system_time(m_sec, m_nsec)?,
        size,
    })
}

const FLAG_VISITED: u8 = 1 << 0;
const FLAG_VALID_CHILD_HASH: u8 = 1 << 1;
const FLAG_VALID_SELF_HASH: u8 = 1 << 2;
const FLAG_HAS_CHILD_HASH: u8 = 1 << 3;
const FLAG_HAS_SELF_HASH: u8 = 1 << 4;
const FLAG_HAS_META_HASH: u8 = 1 << 5;
const FLAG_HAS_DRAFT_NODE_META: u8 = 1 << 6;
const FLAG_HAS_DRAFT_META_META: u8 = 1 << 7;

fn write_opt_hash<W: Write>(hash: &Option<Hash>, writer: &mut W) -> std::io::Result<()> {
    if let Some(h) = hash {
        writer.write_all(h.as_bytes())?;
    }
    Ok(())
}

fn read_opt_hash<R: Read>(present: bool, reader: &mut R) -> Result<Option<Hash>, TreeError> {
    if !present {
        return Ok(None);
    }
    let mut buf = vec![0u8; ActiveAlgorithm::DIGEST_LEN];
    reader.read_exact(&mut buf)?;
    let hash = Hash::from_bytes(&buf).map_err(|e| TreeError::CorruptedData(e.to_string()))?;
    Ok(Some(hash))
}

fn write_opt_file_metadata<W: Write>(
    meta: &Option<FileMetadata>,
    writer: &mut W,
) -> std::io::Result<()> {
    if let Some(m) = meta {
        write_file_metadata(m, writer)?;
    }
    Ok(())
}

fn read_opt_file_metadata<R: Read>(
    present: bool,
    reader: &mut R,
) -> Result<Option<FileMetadata>, TreeError> {
    if present {
        Ok(Some(read_file_metadata(reader)?))
    } else {
        Ok(None)
    }
}

fn write_children_map<W: Write>(
    disc: u8,
    map: &BTreeMap<String, NodeRef>,
    writer: &mut W,
    depth: usize,
) -> Result<(), TreeError> {
    writer.write_all(&[disc])?;
    let count = u32::try_from(map.len()).map_err(|_| {
        TreeError::CorruptedData(format!("children count exceeds u32::MAX: {}", map.len()))
    })?;
    writer.write_all(&count.to_be_bytes())?;
    for child_ref in map.values() {
        serialize_node(child_ref, writer, depth + 1)?;
    }
    Ok(())
}

fn read_children_map<R: Read>(
    reader: &mut R,
    parent: &NodeRef,
    depth: usize,
) -> Result<BTreeMap<String, NodeRef>, TreeError> {
    let mut count_buf = [0u8; 4];
    reader.read_exact(&mut count_buf)?;
    let count = u32::from_be_bytes(count_buf) as usize;
    let mut map = BTreeMap::new();
    for _ in 0..count {
        let child = deserialize_node(reader, Some(parent), depth + 1)?;
        let child_id = child.borrow().id.clone();
        if map.insert(child_id.clone(), child).is_some() {
            return Err(TreeError::CorruptedData(format!(
                "duplicate child id in serialized node: {}",
                child_id
            )));
        }
    }
    Ok(map)
}

fn serialize_node<W: Write>(
    node_ref: &NodeRef,
    writer: &mut W,
    depth: usize,
) -> Result<(), TreeError> {
    let node = node_ref.borrow();
    serialize_node_internal(&node, writer, depth)
}

fn serialize_node_internal<W: Write>(
    node: &Node,
    writer: &mut W,
    depth: usize,
) -> Result<(), TreeError> {
    if depth > MAX_TREE_DEPTH {
        return Err(TreeError::CorruptedData(format!(
            "tree depth exceeds maximum of {}",
            MAX_TREE_DEPTH
        )));
    }

    // 1. Write id (u16 length + UTF-8 bytes)
    let id_bytes = node.id.as_bytes();
    let id_len = u16::try_from(id_bytes.len()).map_err(|_| {
        TreeError::CorruptedData(format!("node id length exceeds u16::MAX: {}", id_bytes.len()))
    })?;
    writer.write_all(&id_len.to_be_bytes())?;
    writer.write_all(id_bytes)?;

    // 2. Write flags (1 byte)
    let mut flags = 0u8;
    if node.visited {
        flags |= FLAG_VISITED;
    }
    if node.valid_child_hash {
        flags |= FLAG_VALID_CHILD_HASH;
    }
    if node.valid_self_hash {
        flags |= FLAG_VALID_SELF_HASH;
    }
    if node.child_hash.is_some() {
        flags |= FLAG_HAS_CHILD_HASH;
    }
    if node.self_hash.is_some() {
        flags |= FLAG_HAS_SELF_HASH;
    }
    if node.meta_hash.is_some() {
        flags |= FLAG_HAS_META_HASH;
    }
    if node.draft_node_metadata.is_some() {
        flags |= FLAG_HAS_DRAFT_NODE_META;
    }
    if node.draft_meta_metadata.is_some() {
        flags |= FLAG_HAS_DRAFT_META_META;
    }
    writer.write_all(&[flags])?;

    // 3. Write hashes
    write_opt_hash(&node.self_hash, writer)?;
    write_opt_hash(&node.meta_hash, writer)?;
    write_opt_hash(&node.child_hash, writer)?;

    // 4. Write metadata blocks
    write_file_metadata(&node.node_metadata, writer)?;
    write_file_metadata(&node.meta_metadata, writer)?;
    write_opt_file_metadata(&node.draft_node_metadata, writer)?;
    write_opt_file_metadata(&node.draft_meta_metadata, writer)?;

    // 5. Write children
    match &node.children {
        Children::None => {
            writer.write_all(&[0u8])?;
        }
        Children::Attributes(map) => {
            write_children_map(1u8, map, writer, depth)?;
        }
        Children::Files(map) => {
            write_children_map(2u8, map, writer, depth)?;
        }
    }

    // 6. Write meta map
    if node.meta.is_empty() {
        writer.write_all(&[0u8])?;
    } else {
        write_children_map(1u8, &node.meta, writer, depth)?;
    }

    Ok(())
}

fn deserialize_node<R: Read>(
    reader: &mut R,
    parent: Option<&NodeRef>,
    depth: usize,
) -> Result<NodeRef, TreeError> {
    if depth > MAX_TREE_DEPTH {
        return Err(TreeError::CorruptedData(format!(
            "tree depth exceeds maximum of {}",
            MAX_TREE_DEPTH
        )));
    }

    // 1. Read id (u16 length + UTF-8 bytes)
    let mut id_len_buf = [0u8; 2];
    reader.read_exact(&mut id_len_buf)?;
    let id_len = u16::from_be_bytes(id_len_buf) as usize;
    let mut id_buf = vec![0u8; id_len];
    reader.read_exact(&mut id_buf)?;
    let id = String::from_utf8(id_buf).map_err(|e| {
        TreeError::CorruptedData(format!("invalid UTF-8 in node id: {}", e))
    })?;

    // 2. Read flags
    let mut flags_buf = [0u8; 1];
    reader.read_exact(&mut flags_buf)?;
    let flags = flags_buf[0];
    let visited = (flags & FLAG_VISITED) != 0;
    let valid_child_hash = (flags & FLAG_VALID_CHILD_HASH) != 0;
    let valid_self_hash = (flags & FLAG_VALID_SELF_HASH) != 0;
    let has_child_hash = (flags & FLAG_HAS_CHILD_HASH) != 0;
    let has_self_hash = (flags & FLAG_HAS_SELF_HASH) != 0;
    let has_meta_hash = (flags & FLAG_HAS_META_HASH) != 0;
    let has_draft_node_meta = (flags & FLAG_HAS_DRAFT_NODE_META) != 0;
    let has_draft_meta_meta = (flags & FLAG_HAS_DRAFT_META_META) != 0;

    // 3. Read hashes
    let self_hash = read_opt_hash(has_self_hash, reader)?;
    let meta_hash = read_opt_hash(has_meta_hash, reader)?;
    let child_hash = read_opt_hash(has_child_hash, reader)?;

    // 4. Read metadata blocks
    let node_metadata = read_file_metadata(reader)?;
    let meta_metadata = read_file_metadata(reader)?;
    let draft_node_metadata = read_opt_file_metadata(has_draft_node_meta, reader)?;
    let draft_meta_metadata = read_opt_file_metadata(has_draft_meta_meta, reader)?;

    // Create current node
    let node_ref = Node::new(id, ChildrenType::None);
    if let Some(p) = parent {
        node_ref.borrow_mut().parent = Some(Rc::downgrade(p));
    }

    // 5. Read children
    let mut disc_buf = [0u8; 1];
    reader.read_exact(&mut disc_buf)?;
    let children = match disc_buf[0] {
        0 => Children::None,
        1 => Children::Attributes(read_children_map(reader, &node_ref, depth)?),
        2 => Children::Files(read_children_map(reader, &node_ref, depth)?),
        other => {
            return Err(TreeError::CorruptedData(format!(
                "Invalid children discriminant: {}",
                other
            )));
        }
    };

    // 6. Read meta map
    let mut meta_disc_buf = [0u8; 1];
    reader.read_exact(&mut meta_disc_buf)?;
    let meta = match meta_disc_buf[0] {
        0 => BTreeMap::new(),
        1 => read_children_map(reader, &node_ref, depth)?,
        other => {
            return Err(TreeError::CorruptedData(format!(
                "Invalid meta discriminant: {}",
                other
            )));
        }
    };

    {
        let mut node = node_ref.borrow_mut();
        node.children = children;
        node.meta = meta;
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

pub fn load_or_fail_tree(index_path: &Path) -> Result<Tree, String> {
    if index_path.exists() {
        let meta = fs::metadata(index_path)
            .map_err(|e| format!("failed to stat {}: {}", index_path.display(), e))?;
        if meta.len() > 0 {
            return Tree::load(index_path)
                .map_err(|e| format!("failed to load {}: {}", index_path.display(), e));
        }
    }
    Err(format!("No tree found at {}", index_path.display()))
}

pub fn load_or_init_tree(index_path: &Path) -> Result<Tree, String> {
    load_or_fail_tree(index_path).or_else(|_| Ok(Tree::new("")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::hash::hash_raw;
    use std::error::Error;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestTempDir {
        path: PathBuf,
    }

    impl TestTempDir {
        fn new(prefix: &str) -> Self {
            let seq = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "git_med_{}_{}_{}",
                prefix,
                std::process::id(),
                seq
            ));
            Self { path }
        }
    }

    impl Drop for TestTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_node_new_defaults_and_tree_new() {
        let node = Node::new("3", ChildrenType::None);
        {
            let n = node.borrow();
            assert!(!n.visited);
            assert!(!n.valid_child_hash);
            assert!(!n.valid_self_hash);
            assert!(n.self_hash.is_none());
            assert!(n.meta_hash.is_none());
            assert!(n.child_hash.is_none());
            assert!(n.draft_node_metadata.is_none());
            assert!(n.draft_meta_metadata.is_none());
            assert_eq!(n.node_metadata.atime, UNIX_EPOCH);
            assert_eq!(n.node_metadata.mtime, UNIX_EPOCH);
            assert_eq!(n.node_metadata.size, None);
            assert_eq!(n.meta_metadata.atime, UNIX_EPOCH);
            assert_eq!(n.meta_metadata.mtime, UNIX_EPOCH);
            assert_eq!(n.meta_metadata.size, None);
            assert!(n.parent().is_none());
            assert!(matches!(n.children, Children::None));
        }

        let files_node = Node::new("5", ChildrenType::Files);
        assert!(matches!(files_node.borrow().children, Children::Files(ref m) if m.is_empty()));

        let tree = Tree::new("4");
        let root = tree.root.borrow();
        assert_eq!(root.id, "4");
        assert!(matches!(root.children, Children::Attributes(ref m) if m.is_empty()));
    }

    #[test]
    fn test_single_node_stream_roundtrip() {
        let node_id = "1";
        let self_hash = hash_raw(b"self_hash_123");
        let meta_hash = hash_raw(b"meta_hash_456");
        let child_hash = hash_raw(b"child_hash_789");

        let node = Node::new(node_id, ChildrenType::None);
        {
            let mut n = node.borrow_mut();
            n.self_hash = Some(self_hash);
            n.meta_hash = Some(meta_hash);
            n.child_hash = Some(child_hash);
            n.visited = true;
            n.valid_child_hash = true;
            n.valid_self_hash = false;
            n.node_metadata.size = Some(4096);
            n.meta_metadata.size = Some(128);
            n.draft_node_metadata = Some(NodeMetadata {
                size: Some(8192),
                ..NodeMetadata::default()
            });
            n.draft_meta_metadata = Some(NodeMetadata {
                size: Some(256),
                ..NodeMetadata::default()
            });
        }

        let mut buffer = Vec::new();
        let tree = Tree::from_root(Rc::clone(&node));
        tree.write_to(&mut buffer).expect("Serialization failed");

        let loaded_tree = Tree::read_from(&mut &buffer[..]).expect("Deserialization failed");
        let loaded_node = loaded_tree.root.borrow();

        assert_eq!(loaded_node.id, node_id);
        assert_eq!(loaded_node.self_hash, Some(self_hash));
        assert_eq!(loaded_node.meta_hash, Some(meta_hash));
        assert_eq!(loaded_node.child_hash, Some(child_hash));
        assert!(loaded_node.visited);
        assert!(loaded_node.valid_child_hash);
        assert!(!loaded_node.valid_self_hash);
        assert_eq!(loaded_node.node_metadata.size, Some(4096));
        assert_eq!(loaded_node.meta_metadata.size, Some(128));
        assert_eq!(
            loaded_node
                .draft_node_metadata
                .as_ref()
                .expect("draft_node_metadata missing")
                .size,
            Some(8192)
        );
        assert_eq!(
            loaded_node
                .draft_meta_metadata
                .as_ref()
                .expect("draft_meta_metadata missing")
                .size,
            Some(256)
        );
        assert!(matches!(loaded_node.children, Children::None));
        assert!(loaded_node.parent.is_none());
    }

    #[test]
    fn test_tree_hierarchy_and_parent_reconstruction() {
        let root = Node::new("0", ChildrenType::Attributes);

        let task_id = "10";
        let task_node = Node::insert(&root, ChildType::Id(task_id.to_string())).expect("Failed to insert task");
        task_node.borrow_mut().children = Children::Files(BTreeMap::new());

        let file_id = "20";
        let _file_node =
            Node::insert(&task_node, ChildType::Id(file_id.to_string())).expect("Failed to insert file");

        let tree = Tree::from_root(Rc::clone(&root));
        let mut buffer = Vec::new();
        tree.write_to(&mut buffer).expect("Write to buffer failed");

        let restored_tree = Tree::read_from(&mut &buffer[..]).expect("Read from buffer failed");
        let restored_root = restored_tree.root;

        assert_eq!(restored_root.borrow().id, "0");
        assert!(restored_root.borrow().parent().is_none());

        let restored_task = match &restored_root.borrow().children {
            Children::Attributes(map) => Rc::clone(map.get(task_id).expect("task node missing")),
            _ => panic!("Expected Children::Attributes on root"),
        };

        let task_parent = restored_task.borrow().parent().expect("task has no parent");
        assert_eq!(task_parent.borrow().id, "0");

        let restored_file = match &restored_task.borrow().children {
            Children::Files(map) => Rc::clone(map.get(file_id).expect("file node missing")),
            _ => panic!("Expected Children::Files on task"),
        };

        let file_parent = restored_file.borrow().parent().expect("file has no parent");
        assert_eq!(file_parent.borrow().id, task_id);
        assert!(matches!(restored_file.borrow().children, Children::None));
    }

    #[test]
    fn test_file_save_load_and_direct_overwrite() {
        let temp_dir = TestTempDir::new("tree_file");
        let nested_dir = temp_dir.path.join("nested");
        let file_path = nested_dir.join("test_tree.bin");

        let root = Node::new("99", ChildrenType::Attributes);
        Node::insert(&root, ChildType::Id("1".to_string())).unwrap();
        Node::insert(&root, ChildType::Id("2".to_string())).unwrap();

        let tree = Tree::from_root(root);
        tree.save(&file_path).expect("Failed to save tree to file");
        let first_len = std::fs::metadata(&file_path).unwrap().len();

        let loaded = Tree::load(&file_path).expect("Failed to load tree from file");
        assert_eq!(loaded.root.borrow().id, "99");

        // Overwrite with a smaller tree directly and confirm truncation + no temp files.
        let smaller = Tree::from_root(Node::new("77", ChildrenType::None));
        smaller.save(&file_path).expect("Failed to overwrite tree");
        let second_len = std::fs::metadata(&file_path).unwrap().len();
        assert!(second_len < first_len);

        let reloaded = Tree::load(&file_path).expect("Failed to reload overwritten tree");
        assert_eq!(reloaded.root.borrow().id, "77");
        assert!(matches!(reloaded.root.borrow().children, Children::None));

        let entries: Vec<_> = std::fs::read_dir(&nested_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("test_tree.bin")]);
    }

    #[test]
    fn test_load_missing_file_returns_io_error() {
        let temp_dir = TestTempDir::new("missing");
        let missing = temp_dir.path.join("does_not_exist.bin");
        assert!(matches!(Tree::load(&missing), Err(TreeError::Io(_))));
        assert!(matches!(Node::load(&missing), Err(TreeError::Io(_))));
    }

    #[test]
    fn test_invalid_magic() {
        let invalid_data = b"NOTVALID";
        assert!(matches!(
            Tree::read_from(&mut &invalid_data[..]),
            Err(TreeError::InvalidMagic)
        ));

        let mut wrong_magic_with_version = b"BADMAGIC".to_vec();
        wrong_magic_with_version.extend_from_slice(&TREE_VERSION.to_be_bytes());
        assert!(matches!(
            Tree::read_from(&mut &wrong_magic_with_version[..]),
            Err(TreeError::InvalidMagic)
        ));

        let short_magic = b"GMED";
        assert!(matches!(
            Tree::read_from(&mut &short_magic[..]),
            Err(TreeError::Io(_))
        ));
    }

    #[test]
    fn test_metadata_timestamp_roundtrip() {
        let node = Node::new("5", ChildrenType::None);
        let test_time = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
        {
            let mut n = node.borrow_mut();
            n.node_metadata.atime = test_time;
            n.node_metadata.mtime = test_time;
            n.meta_metadata.atime = UNIX_EPOCH + Duration::new(1, 0);
            n.meta_metadata.mtime = UNIX_EPOCH + Duration::new(2, 999_999_999);
            n.meta_metadata.size = Some(42);
            n.draft_node_metadata = Some(NodeMetadata {
                atime: UNIX_EPOCH + Duration::new(100, 500),
                size: Some(10),
                ..NodeMetadata::default()
            });
            n.draft_meta_metadata = Some(NodeMetadata {
                mtime: UNIX_EPOCH + Duration::new(999, 0),
                size: Some(20),
                ..NodeMetadata::default()
            });
        }

        let mut buf = Vec::new();
        Tree::from_root(Rc::clone(&node))
            .write_to(&mut buf)
            .unwrap();
        let loaded = Tree::read_from(&mut &buf[..]).unwrap();
        let l = loaded.root.borrow();

        assert_eq!(l.node_metadata.atime, test_time);
        assert_eq!(l.node_metadata.mtime, test_time);
        assert_eq!(l.meta_metadata.atime, UNIX_EPOCH + Duration::new(1, 0));
        assert_eq!(
            l.meta_metadata.mtime,
            UNIX_EPOCH + Duration::new(2, 999_999_999)
        );
        assert_eq!(l.meta_metadata.size, Some(42));
        assert_eq!(
            l.draft_node_metadata
                .as_ref()
                .expect("draft_node_metadata missing")
                .atime,
            UNIX_EPOCH + Duration::new(100, 500)
        );
        assert_eq!(l.draft_node_metadata.as_ref().unwrap().size, Some(10));
        assert_eq!(
            l.draft_meta_metadata
                .as_ref()
                .expect("draft_meta_metadata missing")
                .mtime,
            UNIX_EPOCH + Duration::new(999, 0)
        );
        assert_eq!(l.draft_meta_metadata.as_ref().unwrap().size, Some(20));

        if let Some(pre_epoch) = UNIX_EPOCH.checked_sub(Duration::from_secs(10)) {
            let pre_node = Node::new("6", ChildrenType::None);
            pre_node.borrow_mut().node_metadata.atime = pre_epoch;
            let mut pre_buf = Vec::new();
            Tree::from_root(pre_node).write_to(&mut pre_buf).unwrap();
            let pre_loaded = Tree::read_from(&mut &pre_buf[..]).unwrap();
            assert_eq!(pre_loaded.root.borrow().node_metadata.atime, UNIX_EPOCH);
        }
    }

    #[test]
    fn test_child_hash_none_and_flags_roundtrip() {
        let node = Node::new("6", ChildrenType::None);
        {
            let mut n = node.borrow_mut();
            n.child_hash = None;
            n.valid_child_hash = false;
            n.valid_self_hash = true;
            n.visited = false;
        }

        let mut buf = Vec::new();
        Tree::from_root(Rc::clone(&node))
            .write_to(&mut buf)
            .unwrap();
        let loaded = Tree::read_from(&mut &buf[..]).unwrap();
        let l = loaded.root.borrow();

        assert!(l.self_hash.is_none());
        assert!(l.meta_hash.is_none());
        assert!(l.child_hash.is_none());
        assert!(l.draft_node_metadata.is_none());
        assert!(l.draft_meta_metadata.is_none());
        assert!(!l.valid_child_hash);
        assert!(l.valid_self_hash);
        assert!(!l.visited);
    }

    #[test]
    fn test_empty_attributes_and_files_roundtrip() {
        let attr_node = Node::new("7", ChildrenType::Attributes);
        let mut attr_buf = Vec::new();
        Tree::from_root(attr_node).write_to(&mut attr_buf).unwrap();
        let loaded_attr = Tree::read_from(&mut &attr_buf[..]).unwrap();
        assert!(matches!(
            &loaded_attr.root.borrow().children,
            Children::Attributes(map) if map.is_empty()
        ));

        let files_node = Node::new("8", ChildrenType::Files);
        let mut files_buf = Vec::new();
        Tree::from_root(files_node)
            .write_to(&mut files_buf)
            .unwrap();
        let loaded_files = Tree::read_from(&mut &files_buf[..]).unwrap();
        assert!(matches!(
            &loaded_files.root.borrow().children,
            Children::Files(map) if map.is_empty()
        ));
    }

    #[test]
    fn test_multiple_siblings_ordering() {
        let root = Node::new("0", ChildrenType::Attributes);

        let unsorted_ids: Vec<&str> = vec!["5", "1", "3", "2", "4"];
        for id in &unsorted_ids {
            Node::insert(&root, ChildType::Id((*id).to_string())).unwrap();
        }
        let expected_sorted: Vec<String> = vec!["1", "2", "3", "4", "5"].into_iter().map(String::from).collect();

        let mut buf = Vec::new();
        Tree::from_root(Rc::clone(&root))
            .write_to(&mut buf)
            .unwrap();
        let loaded = Tree::read_from(&mut &buf[..]).unwrap();

        match &loaded.root.borrow().children {
            Children::Attributes(map) => {
                let loaded_ids: Vec<String> = map.keys().cloned().collect();
                assert_eq!(loaded_ids, expected_sorted);
            }
            _ => panic!("Expected Attributes"),
        }
    }

    #[test]
    fn test_unsupported_version() {
        for version in [0u32, 99u32] {
            let mut data = Vec::new();
            data.extend_from_slice(TREE_MAGIC);
            data.extend_from_slice(&version.to_be_bytes());
            let result = Tree::read_from(&mut &data[..]);
            assert!(matches!(
                result,
                Err(TreeError::UnsupportedVersion(v)) if v == version
            ));
        }
    }

    #[test]
    fn test_truncated_file() {
        let root = Node::new("0", ChildrenType::Attributes);
        let child = Node::insert(&root, ChildType::Id("1".to_string())).unwrap();
        child.borrow_mut().self_hash = Some(hash_raw(b"child"));

        let mut buf = Vec::new();
        Tree::from_root(root).write_to(&mut buf).unwrap();

        for len in 0..buf.len() {
            let result = Tree::read_from(&mut &buf[..len]);
            assert!(
                matches!(result, Err(TreeError::Io(_))),
                "expected Io error at truncation length {}",
                len
            );
        }
    }

    #[test]
    fn test_trailing_bytes_rejected_by_tree_read_from() {
        let node = Node::new("0", ChildrenType::None);
        let mut buf = Vec::new();
        Tree::from_root(node).write_to(&mut buf).unwrap();
        buf.push(0x00);

        let result = Tree::read_from(&mut &buf[..]);
        assert!(matches!(result, Err(TreeError::CorruptedData(_))));
    }

    #[test]
    fn test_invalid_children_discriminant() {
        let node = Node::new("0", ChildrenType::None);
        let mut buf = Vec::new();
        Tree::from_root(node).write_to(&mut buf).unwrap();

        let children_disc = buf.len() - 2;
        assert_eq!(buf[children_disc], 0u8);
        let mut bad_children = buf.clone();
        bad_children[children_disc] = 0xFF;
        assert!(matches!(
            Tree::read_from(&mut &bad_children[..]),
            Err(TreeError::CorruptedData(_))
        ));

        let meta_disc = buf.len() - 1;
        assert_eq!(buf[meta_disc], 0u8);
        buf[meta_disc] = 0xFF;
        assert!(matches!(
            Tree::read_from(&mut &buf[..]),
            Err(TreeError::CorruptedData(_))
        ));
    }

    #[test]
    fn test_invalid_size_tag() {
        let node = Node::new("0", ChildrenType::None);
        let mut buf = Vec::new();
        Tree::from_root(node).write_to(&mut buf).unwrap();

        // 12 (header) + 2 (id len) + 1 (id "0") + 1 (flags) + 24 (node_metadata atime+mtime)
        let size_tag_offset = 12 + 2 + 1 + 1 + 24;
        assert_eq!(buf[size_tag_offset], 0u8);
        buf[size_tag_offset] = 0x05;

        let result = Tree::read_from(&mut &buf[..]);
        assert!(matches!(result, Err(TreeError::CorruptedData(_))));
    }

    #[test]
    fn test_invalid_and_overflow_timestamps() {
        let node = Node::new("0", ChildrenType::None);
        let mut base_buf = Vec::new();
        Tree::from_root(node).write_to(&mut base_buf).unwrap();

        // 12 (header) + 2 (id len) + 1 (id "0") + 1 (flags) = start of node_metadata.atime (secs: 8, nanos: 4)
        let atime_secs_offset = 12 + 2 + 1 + 1;
        let atime_nanos_offset = atime_secs_offset + 8;

        let mut bad_nanos = base_buf.clone();
        bad_nanos[atime_nanos_offset..atime_nanos_offset + 4]
            .copy_from_slice(&1_000_000_000u32.to_be_bytes());
        assert!(matches!(
            Tree::read_from(&mut &bad_nanos[..]),
            Err(TreeError::CorruptedData(_))
        ));

        let mut overflow_secs = base_buf;
        overflow_secs[atime_secs_offset..atime_secs_offset + 8]
            .copy_from_slice(&u64::MAX.to_be_bytes());
        assert!(matches!(
            Tree::read_from(&mut &overflow_secs[..]),
            Err(TreeError::CorruptedData(_))
        ));
    }

    #[test]
    fn test_duplicate_child_id_in_stream_rejected() {
        let child_id = "42";
        let child = Node::new(child_id, ChildrenType::None);
        let mut child_bytes = Vec::new();
        child.borrow().write_subtree(&mut child_bytes).unwrap();

        let mut stream = Vec::new();
        stream.extend_from_slice(TREE_MAGIC);
        stream.extend_from_slice(&TREE_VERSION.to_be_bytes());
        // Root node: id len (2) + id ("0") + flags + two empty FileMetadata + children(disc 1, count 2) + meta(0)
        let root_id = "0";
        let root_bytes = root_id.as_bytes();
        stream.extend_from_slice(&(root_bytes.len() as u16).to_be_bytes());
        stream.extend_from_slice(root_bytes);
        stream.push(0u8);
        write_file_metadata(&NodeMetadata::default(), &mut stream).unwrap();
        write_file_metadata(&NodeMetadata::default(), &mut stream).unwrap();
        stream.push(1u8);
        stream.extend_from_slice(&2u32.to_be_bytes());
        stream.extend_from_slice(&child_bytes);
        stream.extend_from_slice(&child_bytes);
        stream.push(0u8);

        let result = Tree::read_from(&mut &stream[..]);
        assert!(matches!(result, Err(TreeError::CorruptedData(_))));
    }

    #[test]
    fn test_excessive_tree_depth_rejected() {
        let root = Node::new("0", ChildrenType::Attributes);
        let mut current = Rc::clone(&root);
        for d in 1..=(MAX_TREE_DEPTH + 1) {
            let next = Node::new(d.to_string(), ChildrenType::Attributes);
            Node::insert(&current, ChildType::Ref(Rc::clone(&next))).unwrap();
            current = next;
        }

        let mut buf = Vec::new();
        let ser_res = Tree::from_root(root).write_to(&mut buf);
        assert!(matches!(ser_res, Err(TreeError::CorruptedData(_))));

        // Craft a raw serialized stream nested MAX_TREE_DEPTH + 2 levels deep.
        let mut deep_stream = Vec::new();
        deep_stream.extend_from_slice(TREE_MAGIC);
        deep_stream.extend_from_slice(&TREE_VERSION.to_be_bytes());
        for d in 0..=(MAX_TREE_DEPTH + 1) {
            let id_str = d.to_string();
            let id_bytes = id_str.as_bytes();
            deep_stream.extend_from_slice(&(id_bytes.len() as u16).to_be_bytes());
            deep_stream.extend_from_slice(id_bytes);
            deep_stream.push(0u8);
            write_file_metadata(&NodeMetadata::default(), &mut deep_stream).unwrap();
            write_file_metadata(&NodeMetadata::default(), &mut deep_stream).unwrap();
            if d <= MAX_TREE_DEPTH {
                deep_stream.push(1u8);
                deep_stream.extend_from_slice(&1u32.to_be_bytes());
            } else {
                deep_stream.push(0u8);
                deep_stream.push(0u8);
            }
        }
        let de_res = Tree::read_from(&mut &deep_stream[..]);
        assert!(matches!(de_res, Err(TreeError::CorruptedData(_))));
    }

    #[test]
    fn test_write_subtree_and_load_subtree() {
        let node = Node::new("42", ChildrenType::None);
        let expected_hash = hash_raw(b"subtree_hash");
        node.borrow_mut().self_hash = Some(expected_hash);

        let mut buf = Vec::new();
        node.borrow().write_subtree(&mut buf).unwrap();

        let loaded = Node::load_subtree(&mut &buf[..], None).unwrap();
        assert_eq!(loaded.borrow().id, "42");
        assert_eq!(loaded.borrow().self_hash, Some(expected_hash));
        assert!(loaded.borrow().parent().is_none());
    }

    #[test]
    fn test_load_subtree_with_parent() {
        let parent = Node::new("1", ChildrenType::Attributes);
        let child = Node::new("2", ChildrenType::None);
        let expected_hash = hash_raw(b"child_hash");
        child.borrow_mut().self_hash = Some(expected_hash);

        let mut buf = Vec::new();
        child.borrow().write_subtree(&mut buf).unwrap();

        let loaded = Node::load_subtree(&mut &buf[..], Some(&parent)).unwrap();
        assert_eq!(loaded.borrow().id, "2");
        assert_eq!(loaded.borrow().self_hash, Some(expected_hash));
        let loaded_parent = loaded.borrow().parent().expect("should have parent");
        assert_eq!(loaded_parent.borrow().id, "1");
    }

    #[test]
    fn test_multi_node_subtree_and_sequential_stream() {
        let sub1 = Node::new("10", ChildrenType::Files);
        Node::insert(&sub1, ChildType::Id("11".to_string())).unwrap();

        let sub2 = Node::new("20", ChildrenType::None);

        let mut stream = Vec::new();
        sub1.borrow().write_subtree(&mut stream).unwrap();
        sub2.borrow().write_subtree(&mut stream).unwrap();

        let mut cursor = &stream[..];
        let loaded1 = Node::load_subtree(&mut cursor, None).unwrap();
        let loaded2 = Node::load_subtree(&mut cursor, None).unwrap();
        assert!(cursor.is_empty());

        match &loaded1.borrow().children {
            Children::Files(map) => {
                let child = map.get("11").expect("missing subtree child");
                assert_eq!(child.borrow().parent().unwrap().borrow().id, "10");
            }
            _ => panic!("Expected Children::Files"),
        }
        assert_eq!(loaded2.borrow().id, "20");
    }

    #[test]
    fn test_node_save_and_load() {
        let temp_dir = TestTempDir::new("node_file");
        let file_path = temp_dir.path.join("node_tree.bin");

        let node = Node::new("88", ChildrenType::None);
        let expected_hash = hash_raw(b"node_save_test");
        node.borrow_mut().self_hash = Some(expected_hash);

        node.borrow().save(&file_path).expect("Node::save failed");
        let loaded = Node::load(&file_path).expect("Node::load failed");
        assert_eq!(loaded.borrow().self_hash, Some(expected_hash));

        // Direct overwrite on the same path.
        let replacement = Node::new("89", ChildrenType::None);
        replacement
            .borrow()
            .save(&file_path)
            .expect("Node::save overwrite failed");
        let reloaded = Node::load(&file_path).expect("Node::load overwrite failed");
        assert_eq!(reloaded.borrow().id, "89");
        assert!(reloaded.borrow().self_hash.is_none());
    }

    #[test]
    fn test_node_insert_ref_errors_and_replacement() {
        let root = Node::new("0", ChildrenType::Attributes);
        let parent = Node::new("1", ChildrenType::Attributes);
        let leaf = Node::new("2", ChildrenType::None);

        Node::insert(&root, ChildType::Ref(Rc::clone(&parent))).unwrap();
        Node::insert(&parent, ChildType::Ref(Rc::clone(&leaf))).unwrap();
        assert_eq!(leaf.borrow().parent().unwrap().borrow().id, "1");

        // Inserting into Children::None must fail without mutating leaf's parent.
        let other_leaf = Node::new("3", ChildrenType::None);
        assert!(Node::insert(&other_leaf, ChildType::Ref(Rc::clone(&leaf))).is_err());
        assert_eq!(leaf.borrow().parent().unwrap().borrow().id, "1");

        // Self-insertion and ancestor cycle insertion must fail without panicking.
        assert!(Node::insert(&parent, ChildType::Ref(Rc::clone(&parent))).is_err());
        assert!(Node::insert(&leaf, ChildType::Ref(Rc::clone(&root))).is_err());
        assert!(Node::insert(&parent, ChildType::Ref(Rc::clone(&root))).is_err());

        // Re-inserting the same NodeRef keeps its parent intact.
        Node::insert(&parent, ChildType::Ref(Rc::clone(&leaf))).unwrap();
        assert_eq!(leaf.borrow().parent().unwrap().borrow().id, "1");

        // Replacing with a new NodeRef of the same ID clears the displaced node's parent.
        let replacement = Node::new("2", ChildrenType::None);
        Node::insert(&parent, ChildType::Ref(Rc::clone(&replacement))).unwrap();
        assert!(leaf.borrow().parent().is_none());
        assert_eq!(
            replacement.borrow().parent().unwrap().borrow().id,
            "1"
        );
    }

    #[test]
    fn test_get_or_insert_child() {
        let root = Node::new("0", ChildrenType::Attributes);
        let child_id = "7";

        let created = get_or_insert_child(&root, child_id, ChildrenType::Files).unwrap();
        assert!(matches!(created.borrow().children, Children::Files(_)));
        assert_eq!(created.borrow().parent().unwrap().borrow().id, "0");
        created.borrow_mut().visited = true;

        let fetched = get_or_insert_child(&root, child_id, ChildrenType::None).unwrap();
        assert!(Rc::ptr_eq(&created, &fetched));
        assert!(fetched.borrow().visited);

        let leaf = get_or_insert_child(&created, "8", ChildrenType::None).unwrap();
        assert!(get_or_insert_child(&leaf, "9", ChildrenType::None).is_err());
    }

    #[test]
    fn test_invalidate_ancestors_hash() {
        let root = Node::new("0", ChildrenType::Attributes);
        let task = get_or_insert_child(&root, "1", ChildrenType::Attributes).unwrap();
        let subject = get_or_insert_child(&task, "2", ChildrenType::Files).unwrap();
        let leaf = get_or_insert_child(&subject, "3", ChildrenType::None).unwrap();
        let sibling_task = get_or_insert_child(&root, "4", ChildrenType::Files).unwrap();

        for node in [&root, &task, &subject, &leaf, &sibling_task] {
            let mut n = node.borrow_mut();
            n.valid_child_hash = true;
            n.valid_self_hash = true;
        }

        leaf.borrow().invalidate_ancestors_hash();

        assert!(!root.borrow().valid_child_hash);
        assert!(!task.borrow().valid_child_hash);
        assert!(!subject.borrow().valid_child_hash);
        assert!(leaf.borrow().valid_child_hash);
        assert!(sibling_task.borrow().valid_child_hash);

        for node in [&root, &task, &subject, &leaf, &sibling_task] {
            assert!(node.borrow().valid_self_hash);
        }

        // Calling on root with no parent is a clean no-op.
        root.borrow_mut().valid_child_hash = true;
        root.borrow().invalidate_ancestors_hash();
        assert!(root.borrow().valid_child_hash);
    }

    #[test]
    fn test_tree_error_traits() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let tree_io: TreeError = io_err.into();
        assert!(tree_io.source().is_some());
        assert!(tree_io.to_string().contains("I/O error"));

        let magic_err = TreeError::InvalidMagic;
        assert!(magic_err.source().is_none());
        assert!(magic_err.to_string().contains("magic"));

        let ver_err = TreeError::UnsupportedVersion(7);
        assert!(ver_err.source().is_none());
        assert!(ver_err.to_string().contains('7'));

        let corrupt_err = TreeError::CorruptedData("bad tag".to_string());
        assert!(corrupt_err.source().is_none());
        assert!(corrupt_err.to_string().contains("bad tag"));
    }

    #[test]
    fn test_meta_btree_roundtrip_and_parent_links() {
        let root = Node::new("0", ChildrenType::Attributes);
        let readme_id = "1";
        let meta_json_id = "2";

        let readme = get_or_insert_meta(&root, readme_id);
        let readme_hash = hash_raw(b"readme");
        readme.borrow_mut().self_hash = Some(readme_hash);
        readme.borrow_mut().visited = true;

        let meta_json = get_or_insert_meta(&root, meta_json_id);
        let meta_hash = hash_raw(b"meta_json");
        meta_json.borrow_mut().self_hash = Some(meta_hash);

        // Calling get_or_insert_meta again returns the same NodeRef.
        let same_readme = get_or_insert_meta(&root, readme_id);
        assert!(Rc::ptr_eq(&readme, &same_readme));

        let mut buf = Vec::new();
        Tree::from_root(root).write_to(&mut buf).unwrap();
        let loaded = Tree::read_from(&mut &buf[..]).unwrap();

        let root_ref = loaded.root.borrow();
        assert_eq!(root_ref.meta.len(), 2);
        let loaded_readme = root_ref.meta.get(readme_id).unwrap();
        assert_eq!(loaded_readme.borrow().self_hash, Some(readme_hash));
        assert!(loaded_readme.borrow().visited);
        assert_eq!(
            loaded_readme.borrow().parent().unwrap().borrow().id,
            "0"
        );
        assert_eq!(
            root_ref.meta.get(meta_json_id).unwrap().borrow().self_hash,
            Some(meta_hash)
        );
    }

    #[test]
    fn test_reset_visited_and_prune_unvisited() {
        let tree = Tree::new("0");
        let task = get_or_insert_child(&tree.root, "1", ChildrenType::Files).unwrap();
        let kept_file = get_or_insert_child(&task, "2", ChildrenType::None).unwrap();
        let deleted_file = get_or_insert_child(&task, "3", ChildrenType::None).unwrap();
        let kept_meta = get_or_insert_meta(&tree.root, "4");
        let deleted_meta = get_or_insert_meta(&task, "5");

        for n in [
            &tree.root,
            &task,
            &kept_file,
            &deleted_file,
            &kept_meta,
            &deleted_meta,
        ] {
            let mut b = n.borrow_mut();
            b.visited = true;
            b.valid_child_hash = true;
            b.valid_self_hash = true;
        }

        tree.reset_visited();
        for n in [
            &tree.root,
            &task,
            &kept_file,
            &deleted_file,
            &kept_meta,
            &deleted_meta,
        ] {
            assert!(!n.borrow().visited);
        }

        // Re-visit only kept nodes.
        tree.root.borrow_mut().visited = true;
        task.borrow_mut().visited = true;
        kept_file.borrow_mut().visited = true;
        kept_meta.borrow_mut().visited = true;

        assert!(tree.prune_unvisited());

        assert!(tree.root.borrow().meta.contains_key("4"));
        assert!(!task.borrow().meta.contains_key("5"));
        assert!(deleted_meta.borrow().parent().is_none());
        assert!(!task.borrow().valid_self_hash);
        assert!(!task.borrow().valid_child_hash);
        assert!(!tree.root.borrow().valid_child_hash);

        match &task.borrow().children {
            Children::Files(map) => {
                assert!(map.contains_key("2"));
                assert!(!map.contains_key("3"));
            }
            _ => panic!("Expected Children::Files"),
        }
        assert!(deleted_file.borrow().parent().is_none());
    }
}

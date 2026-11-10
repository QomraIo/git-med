use regex::bytes;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};
use std::time::SystemTime;

// Type aliases make the signatures much cleaner and easier to read
pub type NodeRef = Rc<RefCell<Node>>;
pub type WeakNodeRef = Weak<RefCell<Node>>;

pub enum Children {
    Attributes(BTreeMap<[u8; 16], NodeRef>), // attribute has other attributes
    Files(BTreeMap<[u8; 16], NodeRef>),      // attribute has files
    None,                                    // file has no sub
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeMetadata {
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub size: Option<u64>,
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

enum childType {
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

    pub fn insert(&mut self, parent_ref: &NodeRef, child: childType) -> Result<NodeRef, ()> {
        let child = match child {
            childType::ID(id) => Node::new(id),
            childType::REF(child) => child,
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
}

pub fn get_or_insert_child(parent_ref: &NodeRef, id: [u8; 16]) -> Result<(NodeRef), ()> {
    let mut parent = parent_ref.borrow_mut();
    match &parent.children {
        Children::Attributes(map) | Children::Files(map) => match map.get(&id) {
            Some(node) => Ok(Rc::clone(node)),
            None => parent.insert(parent_ref, childType::ID(id)),
        },
        Children::None => Err(()),
    }
}

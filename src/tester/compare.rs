use std::fmt;

use crate::core::tree::{Children, NodeRef, Tree};
use crate::files::hash::Hash;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeDiffKind {
    IdMismatch { id_a: String, id_b: String },
    ContainerTypeMismatch { type_a: &'static str, type_b: &'static str },
    ChildMissingInB { child_id: String },
    ChildMissingInA { child_id: String },
    MetaKeyMissingInB { key: String },
    MetaKeyMissingInA { key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeShapeDiff {
    pub path: String,
    pub kind: ShapeDiffKind,
}

impl fmt::Display for TreeShapeDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Shape mismatch at '{}': {:?}", self.path, self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeHashDiff {
    pub path: String,
    pub hash_type: &'static str,
    pub hash_a: Option<Hash>,
    pub hash_b: Option<Hash>,
}

impl fmt::Display for TreeHashDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Hash mismatch at '{}' ({}): A = {:?}, B = {:?}",
            self.path, self.hash_type, self.hash_a, self.hash_b
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeDiff {
    Shape(TreeShapeDiff),
    Hash(TreeHashDiff),
    MetadataMismatch {
        path: String,
        details: String,
    },
}

impl fmt::Display for TreeDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeDiff::Shape(s) => write!(f, "{}", s),
            TreeDiff::Hash(h) => write!(f, "{}", h),
            TreeDiff::MetadataMismatch { path, details } => {
                write!(f, "Metadata mismatch at '{}': {}", path, details)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TreeCompareOptions {
    pub check_shape: bool,
    pub check_hashes: bool,
    pub check_metadata_content: bool,
    pub ignore_timestamps: bool,
}

impl Default for TreeCompareOptions {
    fn default() -> Self {
        Self {
            check_shape: true,
            check_hashes: true,
            check_metadata_content: true,
            ignore_timestamps: true,
        }
    }
}

/// Recursively compare the structural shape of two trees.
#[allow(dead_code)]
pub fn compare_tree_shapes(tree_a: &Tree, tree_b: &Tree) -> Result<(), TreeShapeDiff> {
    compare_node_shape(&tree_a.root, &tree_b.root, "")
}

fn compare_node_shape(node_a: &NodeRef, node_b: &NodeRef, path: &str) -> Result<(), TreeShapeDiff> {
    let a = node_a.borrow();
    let b = node_b.borrow();

    let current_path = if path.is_empty() {
        a.id.clone()
    } else {
        format!("{}/{}", path, a.id)
    };

    if a.id != b.id {
        return Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::IdMismatch {
                id_a: a.id.clone(),
                id_b: b.id.clone(),
            },
        });
    }

    // Compare metadata keys
    for key in a.meta.keys() {
        if !b.meta.contains_key(key) {
            return Err(TreeShapeDiff {
                path: current_path.clone(),
                kind: ShapeDiffKind::MetaKeyMissingInB { key: key.clone() },
            });
        }
    }
    for key in b.meta.keys() {
        if !a.meta.contains_key(key) {
            return Err(TreeShapeDiff {
                path: current_path.clone(),
                kind: ShapeDiffKind::MetaKeyMissingInA { key: key.clone() },
            });
        }
    }

    // Compare children container and items
    match (&a.children, &b.children) {
        (Children::None, Children::None) => Ok(()),
        (Children::None, Children::Attributes(_)) => Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::ContainerTypeMismatch {
                type_a: "None",
                type_b: "Attributes",
            },
        }),
        (Children::None, Children::Files(_)) => Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::ContainerTypeMismatch {
                type_a: "None",
                type_b: "Files",
            },
        }),
        (Children::Attributes(_), Children::None) => Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::ContainerTypeMismatch {
                type_a: "Attributes",
                type_b: "None",
            },
        }),
        (Children::Files(_), Children::None) => Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::ContainerTypeMismatch {
                type_a: "Files",
                type_b: "None",
            },
        }),
        (Children::Attributes(_map_a), Children::Files(_)) => Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::ContainerTypeMismatch {
                type_a: "Attributes",
                type_b: "Files",
            },
        }),
        (Children::Files(_), Children::Attributes(_)) => Err(TreeShapeDiff {
            path: current_path,
            kind: ShapeDiffKind::ContainerTypeMismatch {
                type_a: "Files",
                type_b: "Attributes",
            },
        }),
        (Children::Attributes(map_a), Children::Attributes(map_b))
        | (Children::Files(map_a), Children::Files(map_b)) => {
            for (key, child_a) in map_a {
                if let Some(child_b) = map_b.get(key) {
                    compare_node_shape(child_a, child_b, &current_path)?;
                } else {
                    return Err(TreeShapeDiff {
                        path: current_path,
                        kind: ShapeDiffKind::ChildMissingInB {
                            child_id: key.clone(),
                        },
                    });
                }
            }
            for key in map_b.keys() {
                if !map_a.contains_key(key) {
                    return Err(TreeShapeDiff {
                        path: current_path,
                        kind: ShapeDiffKind::ChildMissingInA {
                            child_id: key.clone(),
                        },
                    });
                }
            }
            Ok(())
        }
    }
}

/// Recursively compare stored hashes (`self_hash`, `child_hash`, `meta_hash`) of two trees.
#[allow(dead_code)]
pub fn compare_tree_hashes(tree_a: &Tree, tree_b: &Tree) -> Result<(), TreeHashDiff> {
    compare_node_hashes(&tree_a.root, &tree_b.root, "")
}

fn compare_node_hashes(
    node_a: &NodeRef,
    node_b: &NodeRef,
    path: &str,
) -> Result<(), TreeHashDiff> {
    let a = node_a.borrow();
    let b = node_b.borrow();

    let current_path = if path.is_empty() {
        a.id.clone()
    } else {
        format!("{}/{}", path, a.id)
    };

    if a.self_hash != b.self_hash {
        return Err(TreeHashDiff {
            path: current_path,
            hash_type: "self_hash",
            hash_a: a.self_hash,
            hash_b: b.self_hash,
        });
    }

    if a.child_hash != b.child_hash {
        return Err(TreeHashDiff {
            path: current_path,
            hash_type: "child_hash",
            hash_a: a.child_hash,
            hash_b: b.child_hash,
        });
    }

    if a.meta_hash.is_some() && b.meta_hash.is_some() && a.meta_hash != b.meta_hash {
        return Err(TreeHashDiff {
            path: current_path.clone(),
            hash_type: "meta_hash",
            hash_a: a.meta_hash,
            hash_b: b.meta_hash,
        });
    }

    for (key, meta_a) in &a.meta {
        if let Some(meta_b) = b.meta.get(key) {
            let h_a = meta_a.borrow().self_hash;
            let h_b = meta_b.borrow().self_hash;
            if h_a != h_b {
                return Err(TreeHashDiff {
                    path: format!("{}/meta:{}", current_path, key),
                    hash_type: "meta_self_hash",
                    hash_a: h_a,
                    hash_b: h_b,
                });
            }
        }
    }

    let map_a = match &a.children {
        Children::Attributes(m) | Children::Files(m) => Some(m),
        Children::None => None,
    };
    let map_b = match &b.children {
        Children::Attributes(m) | Children::Files(m) => Some(m),
        Children::None => None,
    };

    if let (Some(ma), Some(mb)) = (map_a, map_b) {
        for (key, child_a) in ma {
            if let Some(child_b) = mb.get(key) {
                compare_node_hashes(child_a, child_b, &current_path)?;
            }
        }
    }

    Ok(())
}

/// Perform deep equality comparison between two trees according to options.
#[allow(dead_code)]
pub fn compare_trees_equal(
    tree_a: &Tree,
    tree_b: &Tree,
    options: &TreeCompareOptions,
) -> Result<(), TreeDiff> {
    if options.check_shape {
        if let Err(diff) = compare_tree_shapes(tree_a, tree_b) {
            return Err(TreeDiff::Shape(diff));
        }
    }

    if options.check_hashes {
        if let Err(diff) = compare_tree_hashes(tree_a, tree_b) {
            return Err(TreeDiff::Hash(diff));
        }
    }

    if options.check_metadata_content {
        compare_node_metadata(&tree_a.root, &tree_b.root, "", options.ignore_timestamps)?;
    }

    Ok(())
}

fn compare_node_metadata(
    node_a: &NodeRef,
    node_b: &NodeRef,
    path: &str,
    ignore_timestamps: bool,
) -> Result<(), TreeDiff> {
    let a = node_a.borrow();
    let b = node_b.borrow();

    let current_path = if path.is_empty() {
        a.id.clone()
    } else {
        format!("{}/{}", path, a.id)
    };

    if !ignore_timestamps {
        if a.node_metadata != b.node_metadata {
            return Err(TreeDiff::MetadataMismatch {
                path: current_path,
                details: format!(
                    "node_metadata mismatch: {:?} vs {:?}",
                    a.node_metadata, b.node_metadata
                ),
            });
        }
        if a.meta_metadata != b.meta_metadata {
            return Err(TreeDiff::MetadataMismatch {
                path: current_path,
                details: format!(
                    "meta_metadata mismatch: {:?} vs {:?}",
                    a.meta_metadata, b.meta_metadata
                ),
            });
        }
    } else {
        if a.node_metadata.size != b.node_metadata.size {
            return Err(TreeDiff::MetadataMismatch {
                path: current_path,
                details: format!(
                    "node size mismatch: {:?} vs {:?}",
                    a.node_metadata.size, b.node_metadata.size
                ),
            });
        }
        if a.meta_metadata.size != b.meta_metadata.size {
            return Err(TreeDiff::MetadataMismatch {
                path: current_path,
                details: format!(
                    "meta size mismatch: {:?} vs {:?}",
                    a.meta_metadata.size, b.meta_metadata.size
                ),
            });
        }
    }

    let map_a = match &a.children {
        Children::Attributes(m) | Children::Files(m) => Some(m),
        Children::None => None,
    };
    let map_b = match &b.children {
        Children::Attributes(m) | Children::Files(m) => Some(m),
        Children::None => None,
    };

    if let (Some(ma), Some(mb)) = (map_a, map_b) {
        for (key, child_a) in ma {
            if let Some(child_b) = mb.get(key) {
                compare_node_metadata(child_a, child_b, &current_path, ignore_timestamps)?;
            }
        }
    }

    Ok(())
}

/// Assert that two trees share identical hierarchical shape.
#[allow(dead_code)]
pub fn assert_tree_shape_eq(tree_a: &Tree, tree_b: &Tree) {
    if let Err(diff) = compare_tree_shapes(tree_a, tree_b) {
        panic!("Tree shape equality assertion failed: {}", diff);
    }
}

/// Assert that two trees share identical stored hashes.
#[allow(dead_code)]
pub fn assert_tree_hashes_eq(tree_a: &Tree, tree_b: &Tree) {
    if let Err(diff) = compare_tree_hashes(tree_a, tree_b) {
        panic!("Tree hashes equality assertion failed: {}", diff);
    }
}

/// Assert that two trees are deeply equal.
#[allow(dead_code)]
pub fn assert_trees_equal(tree_a: &Tree, tree_b: &Tree) {
    let options = TreeCompareOptions::default();
    if let Err(diff) = compare_trees_equal(tree_a, tree_b, &options) {
        panic!("Tree deep equality assertion failed: {}", diff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tree::{ChildrenType, Node};
    use crate::files::hash::hash_raw;

    #[test]
    fn test_compare_identical_trees() {
        let tree1 = Tree::new("root");
        let tree2 = Tree::new("root");

        assert_tree_shape_eq(&tree1, &tree2);
        assert_tree_hashes_eq(&tree1, &tree2);
        assert_trees_equal(&tree1, &tree2);
    }

    #[test]
    fn test_compare_shape_diff_missing_child() {
        let tree1 = Tree::new("root");
        let tree2 = Tree::new("root");

        let child = Node::new("sub_01", ChildrenType::Files);
        if let Children::Attributes(ref mut map) = tree1.root.borrow_mut().children {
            map.insert("sub_01".to_string(), child);
        }

        let res = compare_tree_shapes(&tree1, &tree2);
        assert!(res.is_err());
        let diff = res.unwrap_err();
        assert_eq!(
            diff.kind,
            ShapeDiffKind::ChildMissingInB {
                child_id: "sub_01".to_string()
            }
        );
    }

    #[test]
    fn test_compare_hash_diff() {
        let tree1 = Tree::new("root");
        let tree2 = Tree::new("root");

        let hash_a = hash_raw(b"data A");
        let hash_b = hash_raw(b"data B");

        tree1.root.borrow_mut().self_hash = Some(hash_a);
        tree2.root.borrow_mut().self_hash = Some(hash_b);

        let res = compare_tree_hashes(&tree1, &tree2);
        assert!(res.is_err());
        let diff = res.unwrap_err();
        assert_eq!(diff.hash_type, "self_hash");
        assert_eq!(diff.hash_a, Some(hash_a));
        assert_eq!(diff.hash_b, Some(hash_b));
    }
}
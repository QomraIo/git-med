use crate::core::tree::{Children, Node, NodeRef};
use crate::files::types::ObjectType;
use crate::files::utils::{hash_bytes, make_object_from_bytes_in};
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;

pub fn needs_hash(node: NodeRef) -> VecDeque<NodeRef> {
    let mut fringe: VecDeque<NodeRef> = VecDeque::new();
    fringe.push_back(node);

    let mut needs_hash_vec: VecDeque<NodeRef> = VecDeque::new();

    while let Some(current) = fringe.pop_front() {
        let (needs_rehash, child_nodes) = {
            let n = current.borrow();
            let needs = !n.valid_self_hash || !n.valid_child_hash;
            let children: Vec<NodeRef> = match &n.children {
                Children::Attributes(map) | Children::Files(map) => {
                    map.values().cloned().collect()
                }
                Children::None => Vec::new(),
            };
            (needs, children)
        };

        if needs_rehash {
            needs_hash_vec.push_back(Rc::clone(&current));
        }

        for child in child_nodes {
            let (child_needs, is_tree_node) = {
                let c = child.borrow();
                let needs = !c.valid_self_hash || !c.valid_child_hash;
                let is_tree = matches!(c.children, Children::Attributes(_) | Children::Files(_));
                (needs, is_tree)
            };

            if is_tree_node && child_needs {
                fringe.push_back(Rc::clone(&child));
            }
        }
    }

    needs_hash_vec
}

fn format_tree_body(node: &Node) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();

    // 1. Metadata hashes (sorted by BTreeMap key order)
    for (_key, meta_node) in &node.meta {
        let m_hash = meta_node
            .borrow()
            .self_hash
            .ok_or_else(|| format!("meta node {:?} missing self_hash", meta_node.borrow().id))?;
        body.extend_from_slice(m_hash.to_hex().as_bytes());
        body.push(b'\n');
    }

    // 2. Section separator
    body.extend_from_slice(b"\0\0\n");

    // 3. Child hashes (sorted by BTreeMap key order)
    match &node.children {
        Children::Attributes(map) | Children::Files(map) => {
            for (_key, child_node) in map {
                let c_hash = child_node.borrow().self_hash.ok_or_else(|| {
                    format!("child node {:?} missing self_hash", child_node.borrow().id)
                })?;
                body.extend_from_slice(c_hash.to_hex().as_bytes());
                body.push(b'\n');
            }
        }
        Children::None => {}
    }

    Ok(body)
}

pub fn save_tree_in(root: NodeRef, repo_root: &Path) -> Result<blake3::Hash, String> {
    let mut pending = needs_hash(Rc::clone(&root));

    // If nothing needed rehashing, return root's existing hash if available
    if pending.is_empty() {
        return root
            .borrow()
            .self_hash
            .ok_or_else(|| "Root has no self_hash and was not scheduled for hashing".to_string());
    }

    while let Some(node_ref) = pending.pop_back() {
        let body = {
            let node = node_ref.borrow();
            format_tree_body(&node)?
        };

        let digest = hash_bytes(&body, ObjectType::Tree);

        make_object_from_bytes_in(&body, digest, ObjectType::Tree, repo_root)
            .map_err(|e| format!("failed to write tree object: {}", e))?;

        let mut node = node_ref.borrow_mut();
        node.self_hash = Some(digest);
        node.valid_self_hash = true;
        node.valid_child_hash = true;
    }

    root.borrow()
        .self_hash
        .ok_or_else(|| "Root missing self_hash after save_tree".to_string())
}

pub fn save_tree(root: NodeRef) -> Result<blake3::Hash, String> {
    save_tree_in(root, Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tree::{ChildrenType, Node};
    use std::fs;
    use std::path::PathBuf;

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "git_med_commit_test_{}_{}_{}",
                name,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join(".git-med/objects")).expect("create objects dir");
            Self { path }
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_needs_hash_order() {
        let root = Node::new([0u8; 16], ChildrenType::Attributes);
        let task = Node::new([1u8; 16], ChildrenType::Attributes);
        let series = Node::new([2u8; 16], ChildrenType::Files);

        Node::insert(&root, crate::core::tree::ChildType::Ref(Rc::clone(&task))).unwrap();
        Node::insert(&task, crate::core::tree::ChildType::Ref(Rc::clone(&series))).unwrap();

        let queue = needs_hash(Rc::clone(&root));
        // BFS order should be root, then task, then series
        assert_eq!(queue.len(), 3);
        assert_eq!(queue[0].borrow().id, [0u8; 16]);
        assert_eq!(queue[1].borrow().id, [1u8; 16]);
        assert_eq!(queue[2].borrow().id, [2u8; 16]);
    }

    #[test]
    fn test_save_tree_bottom_up() {
        let repo = TempRepo::new("save_tree");
        let root = Node::new([0u8; 16], ChildrenType::Attributes);
        let series = Node::new([1u8; 16], ChildrenType::Files);
        let leaf_file = Node::new([2u8; 16], ChildrenType::None);

        let file_blob_hash = blake3::hash(b"test file content");
        leaf_file.borrow_mut().self_hash = Some(file_blob_hash);
        leaf_file.borrow_mut().valid_self_hash = true;

        let meta_file = Node::new([3u8; 16], ChildrenType::None);
        let meta_hash = blake3::hash(b"test meta content");
        meta_file.borrow_mut().self_hash = Some(meta_hash);
        meta_file.borrow_mut().valid_self_hash = true;

        Node::insert(&root, crate::core::tree::ChildType::Ref(Rc::clone(&series))).unwrap();
        Node::insert(&series, crate::core::tree::ChildType::Ref(Rc::clone(&leaf_file))).unwrap();
        series.borrow_mut().meta.insert([3u8; 16], Rc::clone(&meta_file));

        let root_hash = save_tree_in(Rc::clone(&root), &repo.path).expect("save_tree_in should succeed");

        assert_eq!(root.borrow().self_hash, Some(root_hash));
        assert!(root.borrow().valid_self_hash);
        assert!(root.borrow().valid_child_hash);

        let series_hash = series.borrow().self_hash.expect("series should have self_hash");
        assert!(series.borrow().valid_self_hash);
        assert!(series.borrow().valid_child_hash);

        // Verify that object files were written to disk
        let root_hex = root_hash.to_hex();
        let root_obj_path = repo
            .path
            .join(".git-med/objects")
            .join(&root_hex[..2])
            .join(&root_hex[2..]);
        assert!(root_obj_path.is_file(), "root object file must exist");

        let series_hex = series_hash.to_hex();
        let series_obj_path = repo
            .path
            .join(".git-med/objects")
            .join(&series_hex[..2])
            .join(&series_hex[2..]);
        assert!(series_obj_path.is_file(), "series object file must exist");

        // Verify the series object contents
        let series_bytes = fs::read(&series_obj_path).expect("read series obj");
        let expected_series_body = format!("{}\n\0\0\n{}\n", meta_hash.to_hex(), file_blob_hash.to_hex());
        let expected_series_header = format!("tree {}\0", expected_series_body.len());
        let mut expected_full = Vec::new();
        expected_full.extend_from_slice(expected_series_header.as_bytes());
        expected_full.extend_from_slice(expected_series_body.as_bytes());
        assert_eq!(series_bytes, expected_full);
    }

    #[test]
    fn test_save_tree_skips_valid_subtree() {
        let repo = TempRepo::new("skip_valid");
        let root = Node::new([0u8; 16], ChildrenType::Attributes);
        let valid_sub = Node::new([1u8; 16], ChildrenType::Files);
        let invalid_sub = Node::new([2u8; 16], ChildrenType::Files);

        let precomputed_hash = blake3::hash(b"already valid");
        valid_sub.borrow_mut().self_hash = Some(precomputed_hash);
        valid_sub.borrow_mut().valid_self_hash = true;
        valid_sub.borrow_mut().valid_child_hash = true;

        Node::insert(&root, crate::core::tree::ChildType::Ref(Rc::clone(&valid_sub))).unwrap();
        Node::insert(&root, crate::core::tree::ChildType::Ref(Rc::clone(&invalid_sub))).unwrap();

        let queue = needs_hash(Rc::clone(&root));
        // Only root and invalid_sub should be in the queue
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].borrow().id, [0u8; 16]);
        assert_eq!(queue[1].borrow().id, [2u8; 16]);

        let root_hash = save_tree_in(Rc::clone(&root), &repo.path).expect("save_tree_in should succeed");

        // valid_sub should still have its original hash
        assert_eq!(valid_sub.borrow().self_hash, Some(precomputed_hash));

        // invalid_sub should now have a valid hash
        assert!(invalid_sub.borrow().self_hash.is_some());
        assert!(invalid_sub.borrow().valid_self_hash);
        assert!(invalid_sub.borrow().valid_child_hash);

        assert_eq!(root.borrow().self_hash, Some(root_hash));
    }

    #[test]
    fn test_sorted_meta_and_children_order() {
        let root = Node::new([0u8; 16], ChildrenType::Attributes);

        let meta_b = Node::new([20u8; 16], ChildrenType::None);
        let meta_b_hash = blake3::hash(b"meta B");
        meta_b.borrow_mut().self_hash = Some(meta_b_hash);

        let meta_a = Node::new([10u8; 16], ChildrenType::None);
        let meta_a_hash = blake3::hash(b"meta A");
        meta_a.borrow_mut().self_hash = Some(meta_a_hash);

        // Insert in reverse key order
        root.borrow_mut().meta.insert([20u8; 16], meta_b);
        root.borrow_mut().meta.insert([10u8; 16], meta_a);

        let child_b = Node::new([200u8; 16], ChildrenType::None);
        let child_b_hash = blake3::hash(b"child B");
        child_b.borrow_mut().self_hash = Some(child_b_hash);

        let child_a = Node::new([100u8; 16], ChildrenType::None);
        let child_a_hash = blake3::hash(b"child A");
        child_a.borrow_mut().self_hash = Some(child_a_hash);

        // Insert in reverse key order
        Node::insert(&root, crate::core::tree::ChildType::Ref(child_b)).unwrap();
        Node::insert(&root, crate::core::tree::ChildType::Ref(child_a)).unwrap();

        let body = format_tree_body(&root.borrow()).expect("format body");
        let body_str = String::from_utf8_lossy(&body);

        let expected_body = format!(
            "{}\n{}\n\0\0\n{}\n{}\n",
            meta_a_hash.to_hex(),
            meta_b_hash.to_hex(),
            child_a_hash.to_hex(),
            child_b_hash.to_hex()
        );
        assert_eq!(body_str, expected_body);
    }
}

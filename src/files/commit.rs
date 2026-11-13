use crate::core::tree::{Children, ChildrenType, Node, NodeRef, Tree};
use crate::files::types::ObjectType;
use crate::files::utils::{hash_bytes, make_object_from_bytes_in, read_object_bytes_in};
use blake3::Hash;
use std::collections::VecDeque;
use std::path::Path;
use std::rc::Rc;

pub fn hex_from_buf16(buf: &[u8; 16]) -> String {
    let mut s = String::with_capacity(32);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

pub fn buf16_from_hex(hex_str: &str) -> Result<[u8; 16], String> {
    let hex_trimmed = hex_str.trim();
    if hex_trimmed.len() != 32 {
        return Err(format!(
            "expected 32 hex chars for 16-byte id, got {}",
            hex_trimmed.len()
        ));
    }
    let mut buf = [0u8; 16];
    for (i, chunk) in hex_trimmed.as_bytes().chunks_exact(2).enumerate() {
        let byte_str = std::str::from_utf8(chunk)
            .map_err(|e| format!("invalid utf8 in hex chunk: {}", e))?;
        buf[i] = u8::from_str_radix(byte_str, 16)
            .map_err(|e| format!("invalid hex char in id '{}': {}", hex_trimmed, e))?;
    }
    Ok(buf)
}

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

pub fn format_tree_body(node: &Node) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();

    // 1. Metadata: <id_hex> <hash_hex>\n (sorted by BTreeMap key order)
    for (key, meta_node) in &node.meta {
        let m_hash = meta_node
            .borrow()
            .self_hash
            .ok_or_else(|| format!("meta node {:?} missing self_hash", meta_node.borrow().id))?;
        let id_hex = hex_from_buf16(key);
        body.extend_from_slice(id_hex.as_bytes());
        body.push(b' ');
        body.extend_from_slice(m_hash.to_hex().as_bytes());
        body.push(b'\n');
    }

    // 2. Section separator
    body.extend_from_slice(b"\0\0\n");

    // 3. Children: container type line followed by <id_hex> <child_hash_hex>\n
    match &node.children {
        Children::Attributes(map) => {
            body.extend_from_slice(b"attrs\n");
            for (key, child_node) in map {
                let c_hash = child_node.borrow().self_hash.ok_or_else(|| {
                    format!("child node {:?} missing self_hash", child_node.borrow().id)
                })?;
                let id_hex = hex_from_buf16(key);
                body.extend_from_slice(id_hex.as_bytes());
                body.push(b' ');
                body.extend_from_slice(c_hash.to_hex().as_bytes());
                body.push(b'\n');
            }
        }
        Children::Files(map) => {
            body.extend_from_slice(b"files\n");
            for (key, child_node) in map {
                let c_hash = child_node.borrow().self_hash.ok_or_else(|| {
                    format!("child node {:?} missing self_hash", child_node.borrow().id)
                })?;
                let id_hex = hex_from_buf16(key);
                body.extend_from_slice(id_hex.as_bytes());
                body.push(b' ');
                body.extend_from_slice(c_hash.to_hex().as_bytes());
                body.push(b'\n');
            }
        }
        Children::None => {
            body.extend_from_slice(b"none\n");
        }
    }

    Ok(body)
}

pub fn load_tree_from_commit(commit_hash: &Hash) -> Result<Tree, String> {
    load_tree_from_commit_in(Path::new("."), commit_hash)
}

pub fn load_tree_from_commit_in(repo_root: &Path, commit_hash: &Hash) -> Result<Tree, String> {
    let (obj_type, body) = read_object_bytes_in(repo_root, commit_hash)?;
    if obj_type != ObjectType::Commit {
        return Err(format!(
            "object {} is not a commit (got {:?})",
            commit_hash.to_hex(),
            obj_type
        ));
    }

    let body_str = String::from_utf8(body)
        .map_err(|e| format!("invalid utf-8 in commit {}: {}", commit_hash.to_hex(), e))?;
    let first_line = body_str
        .lines()
        .next()
        .ok_or_else(|| format!("empty commit body in {}", commit_hash.to_hex()))?;
    let tree_hash = Hash::from_hex(first_line.trim())
        .map_err(|e| format!("invalid tree hash '{}' in commit: {}", first_line, e))?;

    let root_node = load_tree_node_in(repo_root, &tree_hash, [0u8; 16])?;
    Ok(Tree::from_root(root_node))
}

pub fn load_tree_node_in(
    repo_root: &Path,
    tree_hash: &Hash,
    node_id: [u8; 16],
) -> Result<NodeRef, String> {
    let (obj_type, body) = read_object_bytes_in(repo_root, tree_hash)?;
    if obj_type != ObjectType::Tree {
        return Err(format!(
            "expected tree object at {}, got {:?}",
            tree_hash.to_hex(),
            obj_type
        ));
    }

    let body_str = String::from_utf8(body)
        .map_err(|e| format!("invalid utf-8 in tree {}: {}", tree_hash.to_hex(), e))?;

    let sep = "\0\0\n";
    let sep_pos = body_str
        .find(sep)
        .ok_or_else(|| format!("tree {} missing section separator", tree_hash.to_hex()))?;

    let meta_section = &body_str[..sep_pos];
    let children_section = &body_str[sep_pos + sep.len()..];

    let mut child_lines = children_section.lines();
    let container_type_str = child_lines
        .next()
        .unwrap_or("none")
        .trim();

    let children_type = match container_type_str {
        "attrs" => ChildrenType::Attributes,
        "files" => ChildrenType::Files,
        "none" => ChildrenType::None,
        other => return Err(format!("unknown children type '{}' in tree {}", other, tree_hash.to_hex())),
    };

    let current_node = Node::new(node_id, children_type);
    {
        let mut cur = current_node.borrow_mut();
        cur.self_hash = Some(*tree_hash);
        cur.valid_self_hash = true;
        cur.valid_child_hash = true;
    }

    // Parse meta entries
    for line in meta_section.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let id_str = parts
            .next()
            .ok_or_else(|| format!("missing id in meta entry: '{}'", line))?;
        let hash_str = parts
            .next()
            .ok_or_else(|| format!("missing hash in meta entry: '{}'", line))?;

        let meta_id = buf16_from_hex(id_str)?;
        let meta_hash = Hash::from_hex(hash_str)
            .map_err(|e| format!("invalid meta hash '{}': {}", hash_str, e))?;

        let meta_node = Node::new(meta_id, ChildrenType::None);
        {
            let mut m = meta_node.borrow_mut();
            m.self_hash = Some(meta_hash);
            m.valid_self_hash = true;
            m.parent = Some(Rc::downgrade(&current_node));
        }
        current_node.borrow_mut().meta.insert(meta_id, meta_node);
    }

    // Parse children entries
    for line in child_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let id_str = parts
            .next()
            .ok_or_else(|| format!("missing id in child entry: '{}'", line))?;
        let hash_str = parts
            .next()
            .ok_or_else(|| format!("missing hash in child entry: '{}'", line))?;

        let child_id = buf16_from_hex(id_str)?;
        let child_hash = Hash::from_hex(hash_str)
            .map_err(|e| format!("invalid child hash '{}': {}", hash_str, e))?;

        let (child_obj_type, _) = read_object_bytes_in(repo_root, &child_hash)?;
        let child_node = match child_obj_type {
            ObjectType::Tree => {
                let node = load_tree_node_in(repo_root, &child_hash, child_id)?;
                node.borrow_mut().parent = Some(Rc::downgrade(&current_node));
                node
            }
            ObjectType::Blob => {
                let node = Node::new(child_id, ChildrenType::None);
                {
                    let mut n = node.borrow_mut();
                    n.self_hash = Some(child_hash);
                    n.valid_self_hash = true;
                    n.parent = Some(Rc::downgrade(&current_node));
                }
                node
            }
            other => {
                return Err(format!(
                    "unexpected child object type {:?} for hash {}",
                    other,
                    child_hash.to_hex()
                ));
            }
        };

        match &mut current_node.borrow_mut().children {
            Children::Attributes(map) | Children::Files(map) => {
                map.insert(child_id, child_node);
            }
            Children::None => {}
        }
    }

    Ok(current_node)
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
        let expected_series_body = format!(
            "{} {}\n\0\0\nfiles\n{} {}\n",
            hex_from_buf16(&[3u8; 16]),
            meta_hash.to_hex(),
            hex_from_buf16(&[2u8; 16]),
            file_blob_hash.to_hex()
        );
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
            "{} {}\n{} {}\n\0\0\nattrs\n{} {}\n{} {}\n",
            hex_from_buf16(&[10u8; 16]),
            meta_a_hash.to_hex(),
            hex_from_buf16(&[20u8; 16]),
            meta_b_hash.to_hex(),
            hex_from_buf16(&[100u8; 16]),
            child_a_hash.to_hex(),
            hex_from_buf16(&[200u8; 16]),
            child_b_hash.to_hex()
        );
        assert_eq!(body_str, expected_body);
    }

    #[test]
    fn test_buf16_hex_roundtrip() {
        let original = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
            0x32, 0x10,
        ];
        let hex = hex_from_buf16(&original);
        assert_eq!(hex, "0123456789abcdeffedcba9876543210");
        let parsed = buf16_from_hex(&hex).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn test_load_tree_from_commit_roundtrip() {
        let repo = TempRepo::new("load_tree_roundtrip");

        // Construct a tree with root -> series -> leaf_file and meta
        let root = Node::new([0u8; 16], ChildrenType::Attributes);
        let series = Node::new([1u8; 16], ChildrenType::Files);
        let leaf = Node::new([2u8; 16], ChildrenType::None);

        let blob_content = b"sample leaf blob";
        let blob_hash = hash_bytes(blob_content, ObjectType::Blob);
        make_object_from_bytes_in(blob_content, blob_hash, ObjectType::Blob, &repo.path).unwrap();

        leaf.borrow_mut().self_hash = Some(blob_hash);
        leaf.borrow_mut().valid_self_hash = true;

        let meta_content = b"sample meta json";
        let meta_hash = hash_bytes(meta_content, ObjectType::Meta);
        make_object_from_bytes_in(meta_content, meta_hash, ObjectType::Meta, &repo.path).unwrap();

        let meta_node = Node::new([3u8; 16], ChildrenType::None);
        meta_node.borrow_mut().self_hash = Some(meta_hash);
        meta_node.borrow_mut().valid_self_hash = true;

        series
            .borrow_mut()
            .meta
            .insert([3u8; 16], Rc::clone(&meta_node));
        Node::insert(&series, crate::core::tree::ChildType::Ref(Rc::clone(&leaf))).unwrap();
        Node::insert(&root, crate::core::tree::ChildType::Ref(Rc::clone(&series))).unwrap();

        let root_hash = save_tree_in(Rc::clone(&root), &repo.path).unwrap();

        // Create a commit object pointing to root_hash
        let commit_body = format!("{}\n\nfeat: test commit\n", root_hash.to_hex());
        let commit_hash = hash_bytes(commit_body.as_bytes(), ObjectType::Commit);
        make_object_from_bytes_in(
            commit_body.as_bytes(),
            commit_hash,
            ObjectType::Commit,
            &repo.path,
        )
        .unwrap();

        // Load tree from commit hash
        let loaded_tree =
            load_tree_from_commit_in(&repo.path, &commit_hash).expect("load tree from commit");
        assert_eq!(loaded_tree.root.borrow().id, [0u8; 16]);
        assert_eq!(loaded_tree.root.borrow().self_hash, Some(root_hash));

        // Verify child series exists with id [1u8; 16]
        let loaded_series = match &loaded_tree.root.borrow().children {
            Children::Attributes(map) => {
                map.get(&[1u8; 16]).cloned().expect("series node exists")
            }
            _ => panic!("expected attributes children container"),
        };
        assert_eq!(loaded_series.borrow().id, [1u8; 16]);

        // Verify meta on series exists
        assert!(loaded_series.borrow().meta.contains_key(&[3u8; 16]));

        // Verify leaf child exists on series
        let loaded_leaf = match &loaded_series.borrow().children {
            Children::Files(map) => map.get(&[2u8; 16]).cloned().expect("leaf node exists"),
            _ => panic!("expected files children container"),
        };
        assert_eq!(loaded_leaf.borrow().id, [2u8; 16]);
        assert_eq!(loaded_leaf.borrow().self_hash, Some(blob_hash));
    }
}

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use std::rc::Rc;

use crate::core::branch::load_ref_in;
use crate::core::commit::{Commit, update_branch_head_in};
use crate::core::tree::{Children, ChildrenType, Node, NodeRef, Tree};
use crate::files::commit::load_tree_from_commit_in;
use crate::files::hash::Hash;
use crate::files::types::ObjectType;
use crate::files::utils::read_object_bytes_in;
use crate::remote::traversal::is_ancestor;
use crate::views::get_view_in;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictBreaker {
    None,
    Left,
    Right,
}

pub fn read_view_name(view_path: &Path) -> Result<String, String> {
    match fs::read_to_string(view_path) {
        Ok(content) => {
            let trimmed = content.trim().to_lowercase();
            if trimmed.is_empty() {
                Ok("qomra".to_string())
            } else {
                Ok(trimmed)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("qomra".to_string()),
        Err(e) => Err(format!("failed to read {}: {}", view_path.display(), e)),
    }
}

pub fn get_current_branch_name(repo_root: &Path) -> Option<String> {
    let head_path = repo_root.join(".git-med/HEAD");
    let content = fs::read_to_string(&head_path).ok()?;
    let trimmed = content.trim();
    if let Some(stripped) = trimmed.strip_prefix("ref: refs/heads/") {
        Some(stripped.to_string())
    } else if let Some(stripped) = trimmed.strip_prefix("ref: ") {
        Some(stripped.to_string())
    } else {
        None
    }
}

pub fn load_commit_parents(repo_root: &Path, commit_hash: &Hash) -> Result<Vec<Hash>, String> {
    let (obj_type, body) = read_object_bytes_in(repo_root, commit_hash)?;
    if obj_type != ObjectType::Commit {
        return Err(format!("object {} is not a commit", commit_hash.to_hex()));
    }
    let body_str = std::str::from_utf8(&body)
        .map_err(|e| format!("invalid utf-8 in commit body: {}", e))?;
    let mut lines = body_str.lines();
    let _tree_line = lines.next();
    let mut parents = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Ok(parent_hash) = Hash::from_hex(trimmed) {
            parents.push(parent_hash);
        } else {
            break;
        }
    }
    Ok(parents)
}

pub fn find_merge_base(
    repo_root: &Path,
    commit_a: &Hash,
    commit_b: &Hash,
) -> Result<Option<Hash>, String> {
    if commit_a == commit_b {
        return Ok(Some(*commit_a));
    }

    let mut ancestors_a: HashSet<Hash> = HashSet::new();
    let mut queue_a: VecDeque<Hash> = VecDeque::new();
    ancestors_a.insert(*commit_a);
    queue_a.push_back(*commit_a);

    while let Some(current) = queue_a.pop_front() {
        let parents = load_commit_parents(repo_root, &current)?;
        for p in parents {
            if ancestors_a.insert(p) {
                queue_a.push_back(p);
            }
        }
    }

    let mut visited_b: HashSet<Hash> = HashSet::new();
    let mut queue_b: VecDeque<Hash> = VecDeque::new();
    visited_b.insert(*commit_b);
    queue_b.push_back(*commit_b);

    while let Some(current) = queue_b.pop_front() {
        if ancestors_a.contains(&current) {
            return Ok(Some(current));
        }
        let parents = load_commit_parents(repo_root, &current)?;
        for p in parents {
            if visited_b.insert(p) {
                queue_b.push_back(p);
            }
        }
    }

    Ok(None)
}

pub fn clone_subtree(node_ref: &NodeRef) -> NodeRef {
    let node = node_ref.borrow();
    let c_type = match &node.children {
        Children::Attributes(_) => ChildrenType::Attributes,
        Children::Files(_) => ChildrenType::Files,
        Children::None => ChildrenType::None,
    };
    let new_node = Node::new(&node.id, c_type);
    {
        let mut n = new_node.borrow_mut();
        n.self_hash = node.self_hash;
        n.child_hash = node.child_hash;
        n.meta_hash = node.meta_hash;
        n.valid_self_hash = node.valid_self_hash;
        n.valid_child_hash = node.valid_child_hash;
        n.node_metadata = node.node_metadata;
        n.meta_metadata = node.meta_metadata;
    }
    for (key, m_ref) in &node.meta {
        let cloned_m = clone_subtree(m_ref);
        cloned_m.borrow_mut().parent = Some(Rc::downgrade(&new_node));
        new_node.borrow_mut().meta.insert(key.clone(), cloned_m);
    }
    match &node.children {
        Children::Attributes(map) => {
            let mut new_map = BTreeMap::new();
            for (key, c_ref) in map {
                let cloned_c = clone_subtree(c_ref);
                cloned_c.borrow_mut().parent = Some(Rc::downgrade(&new_node));
                new_map.insert(key.clone(), cloned_c);
            }
            new_node.borrow_mut().children = Children::Attributes(new_map);
        }
        Children::Files(map) => {
            let mut new_map = BTreeMap::new();
            for (key, c_ref) in map {
                let cloned_c = clone_subtree(c_ref);
                cloned_c.borrow_mut().parent = Some(Rc::downgrade(&new_node));
                new_map.insert(key.clone(), cloned_c);
            }
            new_node.borrow_mut().children = Children::Files(new_map);
        }
        Children::None => {}
    }
    new_node
}

fn same_node(a: Option<&NodeRef>, b: Option<&NodeRef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(na), Some(nb)) => {
            let ga = na.borrow();
            let gb = nb.borrow();
            if ga.self_hash.is_some() && gb.self_hash.is_some() {
                ga.self_hash == gb.self_hash
            } else {
                false
            }
        }
        _ => false,
    }
}

pub fn merge_node_3way(
    base: Option<&NodeRef>,
    left: Option<&NodeRef>,
    right: Option<&NodeRef>,
    breaker: ConflictBreaker,
    path: &str,
) -> Result<Option<NodeRef>, String> {
    if left.is_none() && right.is_none() {
        return Ok(None);
    }

    if let (Some(l_ref), None) = (left, right) {
        if base.is_none() {
            return Ok(Some(clone_subtree(l_ref)));
        }
        if same_node(left, base) {
            return Ok(None);
        }
        return match breaker {
            ConflictBreaker::Left => Ok(Some(clone_subtree(l_ref))),
            ConflictBreaker::Right => Ok(None),
            ConflictBreaker::None => Err(format!("conflict at '{}'", path)),
        };
    }

    if let (None, Some(r_ref)) = (left, right) {
        if base.is_none() {
            return Ok(Some(clone_subtree(r_ref)));
        }
        if same_node(right, base) {
            return Ok(None);
        }
        return match breaker {
            ConflictBreaker::Left => Ok(None),
            ConflictBreaker::Right => Ok(Some(clone_subtree(r_ref))),
            ConflictBreaker::None => Err(format!("conflict at '{}'", path)),
        };
    }

    let l_ref = left.unwrap();
    let r_ref = right.unwrap();

    if same_node(left, right) {
        return Ok(Some(clone_subtree(l_ref)));
    }

    let left_is_leaf = matches!(l_ref.borrow().children, Children::None);
    let right_is_leaf = matches!(r_ref.borrow().children, Children::None);

    if left_is_leaf != right_is_leaf {
        return match breaker {
            ConflictBreaker::Left => Ok(Some(clone_subtree(l_ref))),
            ConflictBreaker::Right => Ok(Some(clone_subtree(r_ref))),
            ConflictBreaker::None => Err(format!("conflict at '{}'", path)),
        };
    }

    if left_is_leaf && right_is_leaf {
        if same_node(left, base) {
            return Ok(Some(clone_subtree(r_ref)));
        }
        if same_node(right, base) {
            return Ok(Some(clone_subtree(l_ref)));
        }
        return match breaker {
            ConflictBreaker::Left => Ok(Some(clone_subtree(l_ref))),
            ConflictBreaker::Right => Ok(Some(clone_subtree(r_ref))),
            ConflictBreaker::None => Err(format!("conflict at '{}'", path)),
        };
    }

    if same_node(left, base) {
        return Ok(Some(clone_subtree(r_ref)));
    }
    if same_node(right, base) {
        return Ok(Some(clone_subtree(l_ref)));
    }

    let (left_c_type, left_children) = {
        let l = l_ref.borrow();
        match &l.children {
            Children::Attributes(m) => (ChildrenType::Attributes, m.clone()),
            Children::Files(m) => (ChildrenType::Files, m.clone()),
            Children::None => (ChildrenType::None, BTreeMap::new()),
        }
    };
    let (right_c_type, right_children) = {
        let r = r_ref.borrow();
        match &r.children {
            Children::Attributes(m) => (ChildrenType::Attributes, m.clone()),
            Children::Files(m) => (ChildrenType::Files, m.clone()),
            Children::None => (ChildrenType::None, BTreeMap::new()),
        }
    };

    let base_children = if let Some(b_ref) = base {
        let b = b_ref.borrow();
        match &b.children {
            Children::Attributes(m) | Children::Files(m) => m.clone(),
            Children::None => BTreeMap::new(),
        }
    } else {
        BTreeMap::new()
    };

    let merged_c_type = if left_c_type == right_c_type {
        left_c_type
    } else if left_children.is_empty() {
        right_c_type
    } else if right_children.is_empty() {
        left_c_type
    } else {
        match breaker {
            ConflictBreaker::Left => left_c_type,
            ConflictBreaker::Right => right_c_type,
            ConflictBreaker::None => return Err(format!("conflict at '{}'", path)),
        }
    };

    let merged_node = Node::new(&l_ref.borrow().id, merged_c_type);

    let left_meta = l_ref.borrow().meta.clone();
    let right_meta = r_ref.borrow().meta.clone();
    let base_meta = base.map(|b| b.borrow().meta.clone()).unwrap_or_default();

    let mut all_meta_keys: BTreeSet<String> = BTreeSet::new();
    all_meta_keys.extend(base_meta.keys().cloned());
    all_meta_keys.extend(left_meta.keys().cloned());
    all_meta_keys.extend(right_meta.keys().cloned());

    for meta_key in all_meta_keys {
        let m_base = base_meta.get(&meta_key);
        let m_left = left_meta.get(&meta_key);
        let m_right = right_meta.get(&meta_key);
        let child_path = if path.is_empty() {
            meta_key.clone()
        } else {
            format!("{}/{}", path, meta_key)
        };

        if let Some(merged_m) = merge_node_3way(m_base, m_left, m_right, breaker, &child_path)? {
            merged_m.borrow_mut().parent = Some(Rc::downgrade(&merged_node));
            merged_node.borrow_mut().meta.insert(meta_key, merged_m);
        }
    }

    let mut all_child_keys: BTreeSet<String> = BTreeSet::new();
    all_child_keys.extend(base_children.keys().cloned());
    all_child_keys.extend(left_children.keys().cloned());
    all_child_keys.extend(right_children.keys().cloned());

    let mut merged_children_map = BTreeMap::new();
    for child_key in all_child_keys {
        let c_base = base_children.get(&child_key);
        let c_left = left_children.get(&child_key);
        let c_right = right_children.get(&child_key);
        let child_path = if path.is_empty() {
            child_key.clone()
        } else {
            format!("{}/{}", path, child_key)
        };

        if let Some(merged_c) = merge_node_3way(c_base, c_left, c_right, breaker, &child_path)? {
            merged_c.borrow_mut().parent = Some(Rc::downgrade(&merged_node));
            merged_children_map.insert(child_key, merged_c);
        }
    }

    {
        let mut m = merged_node.borrow_mut();
        match merged_c_type {
            ChildrenType::Attributes => m.children = Children::Attributes(merged_children_map),
            ChildrenType::Files => m.children = Children::Files(merged_children_map),
            ChildrenType::None => m.children = Children::None,
        }
        m.valid_self_hash = false;
        m.valid_child_hash = false;
    }

    Ok(Some(merged_node))
}

pub fn resolve_ref(repo_root: &Path, branch_name: &str) -> Result<Hash, String> {
    let clean = branch_name.trim();
    if clean.is_empty() {
        return Err("branch name cannot be empty".to_string());
    }

    if let Ok(Some(hash)) = load_ref_in(repo_root, clean) {
        return Ok(hash);
    }

    let rel_ref = clean.strip_prefix("refs/heads/").unwrap_or(clean);
    let remote_tracking_path = repo_root
        .join(".git-med/refs/remotes/origin")
        .join(rel_ref);
    if remote_tracking_path.is_file() {
        let content = fs::read_to_string(&remote_tracking_path)
            .map_err(|e| format!("failed to read remote ref: {}", e))?;
        let trimmed = content.trim();
        if let Ok(hash) = Hash::from_hex(trimmed) {
            return Ok(hash);
        }
    }

    if let Ok(hash) = Hash::from_hex(clean) {
        if read_object_bytes_in(repo_root, &hash).is_ok() {
            return Ok(hash);
        }
    }

    Err(format!("branch '{}' not found", branch_name))
}

pub fn merge_in(
    repo_root: &Path,
    branches: &[String],
    breaker: ConflictBreaker,
    custom_message: Option<String>,
) -> Result<(), String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    if branches.is_empty() {
        return Err("please provide branch name(s) to merge".to_string());
    }

    let head_hash_str = crate::core::commit::load_head_hash_in(repo_root)?
        .ok_or_else(|| "HEAD commit not found (empty repository)".to_string())?;
    let head_commit = Hash::from_hex(&head_hash_str)
        .map_err(|e| format!("invalid HEAD commit hash '{}': {}", head_hash_str, e))?;

    let mut target_branches: Vec<(String, Hash)> = Vec::new();
    for b in branches {
        let hash = resolve_ref(repo_root, b)?;
        target_branches.push((b.clone(), hash));
    }

    // Fast-Forward checks
    // 1. Check if all target branches are already ancestors of HEAD
    let mut all_targets_in_head = true;
    for (_, h) in &target_branches {
        let is_anc = is_ancestor(repo_root, h, &head_commit)
            .map_err(|e| format!("ancestry check failed: {:?}", e))?;
        if !is_anc {
            all_targets_in_head = false;
            break;
        }
    }
    if all_targets_in_head {
        println!("Already up to date.");
        return Ok(());
    }

    // 2. Check if a single target branch contains HEAD and all other target branches
    for (name, candidate_hash) in &target_branches {
        let head_is_anc = is_ancestor(repo_root, &head_commit, candidate_hash)
            .map_err(|e| format!("ancestry check failed: {:?}", e))?;
        if !head_is_anc {
            continue;
        }

        let mut all_others_anc = true;
        for (_, other_hash) in &target_branches {
            if other_hash == candidate_hash {
                continue;
            }
            let is_anc = is_ancestor(repo_root, other_hash, candidate_hash)
                .map_err(|e| format!("ancestry check failed: {:?}", e))?;
            if !is_anc {
                all_others_anc = false;
                break;
            }
        }

        if all_others_anc {
            // Fast-forward directly to candidate_hash!
            update_branch_head_in(repo_root, candidate_hash)?;
            let target_tree = load_tree_from_commit_in(repo_root, candidate_hash)?;
            let index_path = git_med_dir.join("index");
            target_tree
                .save(&index_path)
                .map_err(|e| format!("failed to save index {}: {}", index_path.display(), e))?;

            let view_name = read_view_name(&git_med_dir.join("VIEW"))?;
            let view = get_view_in(&view_name, repo_root)
                .ok_or_else(|| format!("unsupported view '{}'", view_name))?;
            view.write(&target_tree.root)
                .map_err(|e| format!("view write failed: {}", e))?;

            println!("Fast-forward merge to {}", name);
            return Ok(());
        }
    }

    // 3-way Merge in a loop with evolving tree
    let mut evolving_tree = load_tree_from_commit_in(repo_root, &head_commit)?;
    evolving_tree.hash_in(repo_root)?;

    for (_branch_name, target_hash) in &target_branches {
        let base_commit_opt = find_merge_base(repo_root, &head_commit, target_hash)?;
        let base_tree = if let Some(base_hash) = base_commit_opt {
            Some(load_tree_from_commit_in(repo_root, &base_hash)?)
        } else {
            None
        };
        let target_tree = load_tree_from_commit_in(repo_root, target_hash)?;

        let base_root = base_tree.as_ref().map(|t| &t.root);
        let left_root = Some(&evolving_tree.root);
        let right_root = Some(&target_tree.root);

        let merged_root = merge_node_3way(base_root, left_root, right_root, breaker, "")?
            .unwrap_or_else(|| Node::new("", ChildrenType::Attributes));

        evolving_tree = Tree::from_root(merged_root);
        evolving_tree.hash_in(repo_root)?;
    }

    let final_tree_hash = evolving_tree.hash_in(repo_root)?;
    let index_path = git_med_dir.join("index");
    evolving_tree
        .save(&index_path)
        .map_err(|e| format!("failed to save index {}: {}", index_path.display(), e))?;

    let mut parents = Vec::new();
    parents.push(head_commit.to_hex());
    for (_, h) in &target_branches {
        parents.push(h.to_hex());
    }

    let message = if let Some(m) = custom_message {
        m
    } else if target_branches.len() == 1 {
        format!("Merge branch '{}'", target_branches[0].0)
    } else {
        let branch_names: Vec<&str> = target_branches.iter().map(|(n, _)| n.as_str()).collect();
        let current_branch = get_current_branch_name(repo_root).unwrap_or_else(|| "HEAD".to_string());
        format!(
            "Merge branches {} into {}",
            branch_names.join(", "),
            current_branch
        )
    };

    let commit = Commit::new_with_parents(final_tree_hash, parents, message);
    let commit_hash = commit.write_in(repo_root)?;

    let view_name = read_view_name(&git_med_dir.join("VIEW"))?;
    let view = get_view_in(&view_name, repo_root)
        .ok_or_else(|| format!("unsupported view '{}'", view_name))?;
    view.write(&evolving_tree.root)
        .map_err(|e| format!("view write failed: {}", e))?;

    if target_branches.len() == 1 {
        println!("Merge made by recursive strategy: {}", commit_hash.to_hex());
    } else {
        println!("Merge made by the 'octopus' strategy: {}", commit_hash.to_hex());
    }

    Ok(())
}

pub fn parse_merge_args(
    args: &[String],
) -> Result<(Vec<String>, ConflictBreaker, Option<String>), String> {
    let mut branches = Vec::new();
    let mut breaker = ConflictBreaker::None;
    let mut breaker_left = false;
    let mut breaker_right = false;
    let mut custom_message = None;

    let mut i = 2;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--left" || arg == "–left" || arg == "—left" || arg == "-left" {
            breaker_left = true;
            i += 1;
        } else if arg == "--right" || arg == "–right" || arg == "—right" || arg == "-right" {
            breaker_right = true;
            i += 1;
        } else if arg == "-m" || arg == "--message" {
            if i + 1 < args.len() {
                custom_message = Some(args[i + 1].clone());
                i += 2;
            } else {
                return Err("-m requires a commit message".to_string());
            }
        } else if !arg.starts_with('-') && !arg.starts_with('–') {
            branches.push(arg.clone());
            i += 1;
        } else {
            return Err(format!("unknown option '{}'", arg));
        }
    }

    if breaker_left && breaker_right {
        return Err("cannot specify both --left and --right".to_string());
    }

    if breaker_left {
        breaker = ConflictBreaker::Left;
    } else if breaker_right {
        breaker = ConflictBreaker::Right;
    }

    if branches.is_empty() {
        return Err("please provide branch name(s) to merge".to_string());
    }

    Ok((branches, breaker, custom_message))
}

pub fn merge(args: &Vec<String>) {
    let (branches, breaker, custom_message) = match parse_merge_args(args) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("git-med merge failed: {}", err);
            std::process::exit(1);
        }
    };

    if let Err(err) = merge_in(Path::new("."), &branches, breaker, custom_message) {
        eprintln!("git-med merge failed: {}", err);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::add::add_in;
    use crate::core::branch::branch_in;
    use crate::core::commit::commit_with_message_in;
    use crate::core::init::init_at;
    use crate::tester::generate::sample::qomra;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempMergeRepo {
        path: PathBuf,
    }

    impl TempMergeRepo {
        fn new(name: &str) -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "git_med_merge_test_{}_{}_{}_{}",
                name,
                std::process::id(),
                id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create repo dir");
            init_at(&path);
            Self { path }
        }
    }

    impl Drop for TempMergeRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_merge_fast_forward_linear() {
        let repo = TempMergeRepo::new("ff_linear");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit 1").unwrap();

        // Create feature branch and add second commit
        branch_in(&repo.path, "feature").unwrap();
        fs::write(repo.path.join("data/extra.txt"), b"feature file").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit 2").unwrap();

        // Switch back to main
        branch_in(&repo.path, "main").unwrap();
        assert!(!repo.path.join("data/extra.txt").exists());

        // Merge feature into main (should fast-forward)
        merge_in(&repo.path, &["feature".to_string()], ConflictBreaker::None, None).unwrap();

        // Verify fast-forward materialized file
        assert!(repo.path.join("data/extra.txt").exists());
        let main_hash = load_ref_in(&repo.path, "main").unwrap().unwrap();
        let feat_hash = load_ref_in(&repo.path, "feature").unwrap().unwrap();
        assert_eq!(main_hash, feat_hash);
    }

    #[test]
    fn test_merge_already_up_to_date() {
        let repo = TempMergeRepo::new("up_to_date");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit 1").unwrap();

        branch_in(&repo.path, "feature").unwrap();
        branch_in(&repo.path, "main").unwrap();

        // Merging feature which is at the same commit as main
        merge_in(&repo.path, &["feature".to_string()], ConflictBreaker::None, None).unwrap();
    }

    #[test]
    fn test_merge_3way_diverged_clean() {
        let repo = TempMergeRepo::new("3way_clean");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Base commit").unwrap();

        // Branch feature_a
        branch_in(&repo.path, "feature_a").unwrap();
        fs::write(repo.path.join("data/file_a.txt"), b"from branch a").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit A").unwrap();

        // Branch feature_b from main
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "feature_b").unwrap();
        fs::write(repo.path.join("data/file_b.txt"), b"from branch b").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B").unwrap();

        // Switch back to feature_a and merge feature_b
        branch_in(&repo.path, "feature_a").unwrap();
        merge_in(&repo.path, &["feature_b".to_string()], ConflictBreaker::None, None).unwrap();

        // Both files should be present
        assert!(repo.path.join("data/file_a.txt").exists());
        assert!(repo.path.join("data/file_b.txt").exists());

        // Verify merge commit has 2 parents
        let head_hash = crate::core::commit::load_head_hash_in(&repo.path).unwrap().unwrap();
        let head = Hash::from_hex(&head_hash).unwrap();
        let parents = load_commit_parents(&repo.path, &head).unwrap();
        assert_eq!(parents.len(), 2);
    }

    #[test]
    fn test_merge_conflict_fails_without_breaker() {
        let repo = TempMergeRepo::new("conflict_fail");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Base commit").unwrap();

        // Branch feature_a edits common file
        branch_in(&repo.path, "feature_a").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version A").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit A").unwrap();

        // Branch feature_b edits common file differently
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "feature_b").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version B").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B").unwrap();

        // Switch to feature_a and merge feature_b without breaker
        branch_in(&repo.path, "feature_a").unwrap();
        let err = merge_in(&repo.path, &["feature_b".to_string()], ConflictBreaker::None, None)
            .expect_err("merge should fail on conflict");

        assert!(err.contains("conflict at 'data/conflict.txt'"), "err was: {}", err);
    }

    #[test]
    fn test_merge_conflict_breaker_left_and_right() {
        let repo = TempMergeRepo::new("conflict_breaker");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Base commit").unwrap();

        // Branch feature_a
        branch_in(&repo.path, "feature_a").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version A").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit A").unwrap();

        // Branch feature_b
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "feature_b").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version B").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B").unwrap();

        // Merge with --left: keep version A
        branch_in(&repo.path, "feature_a").unwrap();
        merge_in(&repo.path, &["feature_b".to_string()], ConflictBreaker::Left, None).unwrap();
        assert_eq!(
            fs::read_to_string(repo.path.join("data/conflict.txt")).unwrap(),
            "version A"
        );

        // Test with --right on a fresh branch
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "feature_c").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version C").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit C").unwrap();

        merge_in(&repo.path, &["feature_b".to_string()], ConflictBreaker::Right, None).unwrap();
        assert_eq!(
            fs::read_to_string(repo.path.join("data/conflict.txt")).unwrap(),
            "version B"
        );
    }

    #[test]
    fn test_merge_no_common_ancestry_empty_tree_base() {
        let repo = TempMergeRepo::new("disconnected");

        // Commit on branch 1
        fs::write(repo.path.join("data_1.txt"), b"hello branch 1").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Initial branch 1").unwrap();

        // Create an orphan/disconnected branch 2 by resetting HEAD to an empty state
        // Point HEAD to refs/heads/branch2
        fs::write(repo.path.join(".git-med/HEAD"), "ref: refs/heads/branch2\n").unwrap();
        // Clear index
        let empty_tree = Tree::new("");
        empty_tree.save(repo.path.join(".git-med/index")).unwrap();
        // Remove working tree file
        let _ = fs::remove_file(repo.path.join("data_1.txt"));

        // Commit on branch 2 (unborn, no parent)
        fs::write(repo.path.join("data_2.txt"), b"hello branch 2").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Initial branch 2").unwrap();

        // Now merge main (which has data_1.txt) into branch2 (which has data_2.txt)
        // No common ancestry -> uses empty tree base
        merge_in(&repo.path, &["main".to_string()], ConflictBreaker::None, None).unwrap();

        assert!(repo.path.join("data_1.txt").exists());
        assert!(repo.path.join("data_2.txt").exists());
    }

    #[test]
    fn test_octopus_merge_evolving_tree() {
        let repo = TempMergeRepo::new("octopus");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Base commit").unwrap();

        // Create branch B1
        branch_in(&repo.path, "b1").unwrap();
        fs::write(repo.path.join("data/b1.txt"), b"from b1").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B1").unwrap();

        // Create branch B2
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "b2").unwrap();
        fs::write(repo.path.join("data/b2.txt"), b"from b2").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B2").unwrap();

        // Create branch B3
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "b3").unwrap();
        fs::write(repo.path.join("data/b3.txt"), b"from b3").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B3").unwrap();

        // Switch to main and perform octopus merge of b1, b2, b3
        branch_in(&repo.path, "main").unwrap();
        merge_in(
            &repo.path,
            &["b1".to_string(), "b2".to_string(), "b3".to_string()],
            ConflictBreaker::None,
            None,
        )
        .unwrap();

        // All 3 files must be present in the working directory
        assert!(repo.path.join("data/b1.txt").exists());
        assert!(repo.path.join("data/b2.txt").exists());
        assert!(repo.path.join("data/b3.txt").exists());

        // Check merge commit has 4 parents (main, b1, b2, b3)
        let head_hash = crate::core::commit::load_head_hash_in(&repo.path).unwrap().unwrap();
        let head = Hash::from_hex(&head_hash).unwrap();
        let parents = load_commit_parents(&repo.path, &head).unwrap();
        assert_eq!(parents.len(), 4);
    }

    #[test]
    fn test_octopus_merge_fast_forward() {
        let repo = TempMergeRepo::new("octopus_ff");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit 1").unwrap();

        // Branch b1 from main
        branch_in(&repo.path, "b1").unwrap();
        fs::write(repo.path.join("data/f1.txt"), b"f1").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B1").unwrap();

        // Branch b2 from b1
        branch_in(&repo.path, "b2").unwrap();
        fs::write(repo.path.join("data/f2.txt"), b"f2").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B2").unwrap();

        // Branch b3 from b2
        branch_in(&repo.path, "b3").unwrap();
        fs::write(repo.path.join("data/f3.txt"), b"f3").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B3").unwrap();

        // Switch to main and merge b1, b2, b3
        branch_in(&repo.path, "main").unwrap();
        merge_in(
            &repo.path,
            &["b1".to_string(), "b2".to_string(), "b3".to_string()],
            ConflictBreaker::None,
            None,
        )
        .unwrap();

        // Main should be fast-forwarded to b3
        let main_hash = load_ref_in(&repo.path, "main").unwrap().unwrap();
        let b3_hash = load_ref_in(&repo.path, "b3").unwrap().unwrap();
        assert_eq!(main_hash, b3_hash);
        assert!(repo.path.join("data/f1.txt").exists());
        assert!(repo.path.join("data/f2.txt").exists());
        assert!(repo.path.join("data/f3.txt").exists());
    }

    #[test]
    fn test_octopus_merge_conflict_breaker() {
        let repo = TempMergeRepo::new("octopus_conflict");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Base commit").unwrap();

        // b1 writes conflict.txt
        branch_in(&repo.path, "b1").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version B1").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B1").unwrap();

        // b2 writes conflict.txt differently
        branch_in(&repo.path, "main").unwrap();
        branch_in(&repo.path, "b2").unwrap();
        fs::write(repo.path.join("data/conflict.txt"), b"version B2").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit B2").unwrap();

        // Merge on main without breaker should fail
        branch_in(&repo.path, "main").unwrap();
        let err = merge_in(
            &repo.path,
            &["b1".to_string(), "b2".to_string()],
            ConflictBreaker::None,
            None,
        )
        .expect_err("octopus should fail on conflict without breaker");
        assert!(err.contains("conflict at 'data/conflict.txt'"));

        // Merge with --right: takes B2
        merge_in(
            &repo.path,
            &["b1".to_string(), "b2".to_string()],
            ConflictBreaker::Right,
            None,
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(repo.path.join("data/conflict.txt")).unwrap(),
            "version B2"
        );
    }

    #[test]
    fn test_merge_custom_message() {
        let repo = TempMergeRepo::new("custom_msg");
        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Base commit").unwrap();

        branch_in(&repo.path, "f1").unwrap();
        fs::write(repo.path.join("data/f1.txt"), b"f1").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit F1").unwrap();

        branch_in(&repo.path, "main").unwrap();
        fs::write(repo.path.join("data/main.txt"), b"main").unwrap();
        add_in(&repo.path).unwrap();
        commit_with_message_in(&repo.path, "Commit Main").unwrap();

        merge_in(
            &repo.path,
            &["f1".to_string()],
            ConflictBreaker::None,
            Some("My Custom Merge Commit Message".to_string()),
        )
        .unwrap();

        let head_hash = crate::core::commit::load_head_hash_in(&repo.path).unwrap().unwrap();
        let head = Hash::from_hex(&head_hash).unwrap();
        let (_, body) = read_object_bytes_in(&repo.path, &head).unwrap();
        let body_str = String::from_utf8(body).unwrap();
        assert!(body_str.contains("My Custom Merge Commit Message"));
    }

    #[test]
    fn test_merge_cli_runner() {
        use crate::tester::cli_runner::GitMedCli;
        let repo = TempMergeRepo::new("cli_runner");
        let cli = GitMedCli::new(&repo.path);

        qomra(&repo.path, &["data"], 1, 1, 1, 1);
        cli.add().unwrap();
        cli.commit("Base commit").unwrap();

        cli.branch("feature").unwrap();
        fs::write(repo.path.join("data/feature.txt"), b"from feature").unwrap();
        cli.add().unwrap();
        cli.commit("Feature commit").unwrap();

        cli.branch("main").unwrap();
        let output = cli.merge_cmd(&["feature"]).unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "stdout: {}, stderr: {}", stdout, stderr);
        assert!(repo.path.join("data/feature.txt").exists(), "stdout: {}, stderr: {}", stdout, stderr);
    }
}

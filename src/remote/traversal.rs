use std::collections::{HashSet, VecDeque};
use std::path::Path;

use crate::files::hash::Hash;
use crate::files::types::ObjectType;
use crate::files::utils::read_object_bytes_in;
use crate::remote::types::RemoteError;

/// Traverse commit and tree reachability to determine which content-addressed
/// objects exist locally under `<repo_root>` that are not present on the remote.
///
/// Stops traversal at commits or objects matching `known_remote_hashes`.
/// Returns an ordered list of `(Hash, ObjectType, Vec<u8>)` objects ready for packfile generation.
pub fn collect_objects_to_pack(
    repo_root: &Path,
    start_commit: &Hash,
    known_remote_hashes: &[Hash],
) -> Result<Vec<(Hash, ObjectType, Vec<u8>)>, RemoteError> {
    let mut visited_commits: HashSet<Hash> = HashSet::new();
    let mut visited_objects: HashSet<Hash> = HashSet::new();
    let mut objects: Vec<(Hash, ObjectType, Vec<u8>)> = Vec::new();
    let mut commit_queue: VecDeque<Hash> = VecDeque::new();

    commit_queue.push_back(*start_commit);

    while let Some(commit_hash) = commit_queue.pop_front() {
        if known_remote_hashes.contains(&commit_hash) || visited_commits.contains(&commit_hash) {
            continue;
        }
        visited_commits.insert(commit_hash);

        // Read commit object
        let (obj_type, commit_body) = read_object_bytes_in(repo_root, &commit_hash).map_err(|e| {
            RemoteError::Protocol(format!(
                "failed to read commit object {}: {}",
                commit_hash.to_hex(),
                e
            ))
        })?;

        if obj_type != ObjectType::Commit {
            return Err(RemoteError::Protocol(format!(
                "object {} is not a commit (got {:?})",
                commit_hash.to_hex(),
                obj_type
            )));
        }

        if visited_objects.insert(commit_hash) {
            objects.push((commit_hash, ObjectType::Commit, commit_body.clone()));
        }

        // Parse commit body to extract tree hash and parent hash
        let body_str = std::str::from_utf8(&commit_body).map_err(|e| {
            RemoteError::Protocol(format!(
                "invalid utf-8 in commit body {}: {}",
                commit_hash.to_hex(),
                e
            ))
        })?;

        let mut lines = body_str.lines();
        let tree_line = lines.next().ok_or_else(|| {
            RemoteError::Protocol(format!(
                "empty commit body in {}",
                commit_hash.to_hex()
            ))
        })?.trim();

        let tree_hash = Hash::from_hex(tree_line).map_err(|e| {
            RemoteError::InvalidHash(format!(
                "invalid tree hash '{}' in commit {}: {}",
                tree_line,
                commit_hash.to_hex(),
                e
            ))
        })?;

        // If parent commit exists, enqueue it if not already known to the remote
        if let Some(parent_line) = lines.next() {
            let parent_trimmed = parent_line.trim();
            if !parent_trimmed.is_empty() {
                let parent_hash = Hash::from_hex(parent_trimmed).map_err(|e| {
                    RemoteError::InvalidHash(format!(
                        "invalid parent hash '{}' in commit {}: {}",
                        parent_trimmed,
                        commit_hash.to_hex(),
                        e
                    ))
                })?;

                if !known_remote_hashes.contains(&parent_hash)
                    && !visited_commits.contains(&parent_hash)
                {
                    commit_queue.push_back(parent_hash);
                }
            }
        }

        // Recursively traverse tree and all referenced children / metadata
        traverse_tree(
            repo_root,
            &tree_hash,
            &mut visited_objects,
            &mut objects,
            known_remote_hashes,
        )?;
    }

    Ok(objects)
}

/// Recursively traverse a tree object and collect all missing subtrees, metadata objects, and blobs.
fn traverse_tree(
    repo_root: &Path,
    tree_hash: &Hash,
    visited_objects: &mut HashSet<Hash>,
    objects: &mut Vec<(Hash, ObjectType, Vec<u8>)>,
    known_remote_hashes: &[Hash],
) -> Result<(), RemoteError> {
    if known_remote_hashes.contains(tree_hash) || !visited_objects.insert(*tree_hash) {
        return Ok(());
    }

    let (obj_type, tree_body) = read_object_bytes_in(repo_root, tree_hash).map_err(|e| {
        RemoteError::Protocol(format!(
            "failed to read tree object {}: {}",
            tree_hash.to_hex(),
            e
        ))
    })?;

    if obj_type != ObjectType::Tree {
        return Err(RemoteError::Protocol(format!(
            "object {} is not a tree (got {:?})",
            tree_hash.to_hex(),
            obj_type
        )));
    }

    objects.push((*tree_hash, ObjectType::Tree, tree_body.clone()));

    // Split metadata section and children section by "\0\0\n"
    let sep = b"\0\0\n";
    let sep_pos = match tree_body.windows(sep.len()).position(|w| w == sep) {
        Some(pos) => pos,
        None => {
            return Err(RemoteError::Protocol(format!(
                "tree {} missing section separator",
                tree_hash.to_hex()
            )));
        }
    };

    let meta_section = &tree_body[..sep_pos];
    let children_section = &tree_body[sep_pos + sep.len()..];

    // 1. Process metadata entries: <name>\0<hash_hex>\n
    for line in meta_section.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(|&b| b == b'\0');
        let _name = parts.next();
        let hash_bytes = parts.next().ok_or_else(|| {
            RemoteError::Protocol(format!(
                "missing hash in meta entry in tree {}",
                tree_hash.to_hex()
            ))
        })?;
        let hash_str = std::str::from_utf8(hash_bytes)
            .map_err(|e| RemoteError::Protocol(format!("invalid utf-8 in meta hash: {}", e)))?
            .trim();
        let meta_hash = Hash::from_hex(hash_str).map_err(|e| {
            RemoteError::InvalidHash(format!(
                "invalid meta hash in tree {}: {}",
                tree_hash.to_hex(),
                e
            ))
        })?;

        if !known_remote_hashes.contains(&meta_hash) && visited_objects.insert(meta_hash) {
            let (m_type, m_body) = read_object_bytes_in(repo_root, &meta_hash).map_err(|e| {
                RemoteError::Protocol(format!(
                    "failed to read meta object {}: {}",
                    meta_hash.to_hex(),
                    e
                ))
            })?;
            objects.push((meta_hash, m_type, m_body));
        }
    }

    // 2. Process children entries: container line followed by <name>\0<child_hash_hex>\n
    let mut child_lines = children_section.split(|&b| b == b'\n');
    let _container_type = child_lines.next();

    for line in child_lines {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split(|&b| b == b'\0');
        let _name = parts.next();
        let hash_bytes = parts.next().ok_or_else(|| {
            RemoteError::Protocol(format!(
                "missing hash in child entry in tree {}",
                tree_hash.to_hex()
            ))
        })?;
        let hash_str = std::str::from_utf8(hash_bytes)
            .map_err(|e| RemoteError::Protocol(format!("invalid utf-8 in child hash: {}", e)))?
            .trim();
        let child_hash = Hash::from_hex(hash_str).map_err(|e| {
            RemoteError::InvalidHash(format!(
                "invalid child hash in tree {}: {}",
                tree_hash.to_hex(),
                e
            ))
        })?;

        if known_remote_hashes.contains(&child_hash) {
            continue;
        }

        let (child_obj_type, child_body) = read_object_bytes_in(repo_root, &child_hash).map_err(|e| {
            RemoteError::Protocol(format!(
                "failed to read child object {}: {}",
                child_hash.to_hex(),
                e
            ))
        })?;

        match child_obj_type {
            ObjectType::Tree => {
                traverse_tree(
                    repo_root,
                    &child_hash,
                    visited_objects,
                    objects,
                    known_remote_hashes,
                )?;
            }
            ObjectType::Blob | ObjectType::Meta | ObjectType::Lfs => {
                if visited_objects.insert(child_hash) {
                    objects.push((child_hash, child_obj_type, child_body));
                }
            }
            other => {
                return Err(RemoteError::Protocol(format!(
                    "unexpected child object type {:?} for hash {}",
                    other,
                    child_hash.to_hex()
                )));
            }
        }
    }

    Ok(())
}

/// Check if `ancestor` is an ancestor of `descendant` in the repository commit DAG.
/// Returns `Ok(true)` if `ancestor == descendant` or reachable via parent links.
pub fn is_ancestor(
    repo_root: &Path,
    ancestor: &Hash,
    descendant: &Hash,
) -> Result<bool, RemoteError> {
    if ancestor == descendant {
        return Ok(true);
    }

    let mut visited: HashSet<Hash> = HashSet::new();
    let mut queue: VecDeque<Hash> = VecDeque::new();
    queue.push_back(*descendant);

    while let Some(current) = queue.pop_front() {
        if current == *ancestor {
            return Ok(true);
        }

        if !visited.insert(current) {
            continue;
        }

        let (obj_type, body) = match read_object_bytes_in(repo_root, &current) {
            Ok(res) => res,
            Err(e) => {
                return Err(RemoteError::Protocol(format!(
                    "failed to read commit object {}: {}",
                    current.to_hex(),
                    e
                )));
            }
        };

        if obj_type != ObjectType::Commit {
            return Err(RemoteError::Protocol(format!(
                "object {} is not a commit (got {:?})",
                current.to_hex(),
                obj_type
            )));
        }

        let body_str = std::str::from_utf8(&body).map_err(|e| {
            RemoteError::Protocol(format!(
                "invalid utf-8 in commit body {}: {}",
                current.to_hex(),
                e
            ))
        })?;

        let mut lines = body_str.lines();
        let _tree_line = lines.next();

        if let Some(parent_line) = lines.next() {
            let parent_trimmed = parent_line.trim();
            if !parent_trimmed.is_empty() {
                let parent_hash = Hash::from_hex(parent_trimmed).map_err(|e| {
                    RemoteError::InvalidHash(format!(
                        "invalid parent hash '{}' in commit {}: {}",
                        parent_trimmed,
                        current.to_hex(),
                        e
                    ))
                })?;

                if parent_hash == *ancestor {
                    return Ok(true);
                }

                if !visited.contains(&parent_hash) {
                    queue.push_back(parent_hash);
                }
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::init::init_at;
    use crate::files::utils::make_object_from_bytes_in;
    use crate::core::commit::format_commit_body;
    use crate::files::hash::hash_raw;
    use std::fs;
    use std::path::PathBuf;

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "git_med_traversal_test_{}_{}_{}",
                name,
                std::process::id(),
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

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write_test_commit(repo_root: &Path, tree_hash: &Hash, parent: Option<&Hash>, msg: &str) -> Hash {
        let parent_str = parent.map(|p| p.to_hex());
        let body = format_commit_body(tree_hash, parent_str.as_deref(), msg);
        let commit_hash = crate::files::utils::hash_bytes(&body, ObjectType::Commit);
        make_object_from_bytes_in(&body, commit_hash, ObjectType::Commit, repo_root)
            .expect("write commit object");
        commit_hash
    }

    #[test]
    fn test_is_ancestor_linear_history() {
        let repo = TempRepo::new("is_ancestor_linear");
        let dummy_tree = hash_raw(b"tree1");

        // C1 (root) -> C2 -> C3
        let c1 = write_test_commit(&repo.path, &dummy_tree, None, "commit 1");
        let c2 = write_test_commit(&repo.path, &dummy_tree, Some(&c1), "commit 2");
        let c3 = write_test_commit(&repo.path, &dummy_tree, Some(&c2), "commit 3");

        assert!(is_ancestor(&repo.path, &c1, &c1).unwrap());
        assert!(is_ancestor(&repo.path, &c1, &c2).unwrap());
        assert!(is_ancestor(&repo.path, &c1, &c3).unwrap());
        assert!(is_ancestor(&repo.path, &c2, &c3).unwrap());

        // Descendant cannot be ancestor of parent
        assert!(!is_ancestor(&repo.path, &c2, &c1).unwrap());
        assert!(!is_ancestor(&repo.path, &c3, &c1).unwrap());
        assert!(!is_ancestor(&repo.path, &c3, &c2).unwrap());
    }

    #[test]
    fn test_is_ancestor_diverged_history() {
        let repo = TempRepo::new("is_ancestor_diverged");
        let dummy_tree = hash_raw(b"tree2");

        // C1 (root) -> C2a
        //           -> C2b
        let c1 = write_test_commit(&repo.path, &dummy_tree, None, "commit 1");
        let c2a = write_test_commit(&repo.path, &dummy_tree, Some(&c1), "commit 2a");
        let c2b = write_test_commit(&repo.path, &dummy_tree, Some(&c1), "commit 2b");

        assert!(is_ancestor(&repo.path, &c1, &c2a).unwrap());
        assert!(is_ancestor(&repo.path, &c1, &c2b).unwrap());

        // Diverged branches are not ancestors of each other
        assert!(!is_ancestor(&repo.path, &c2a, &c2b).unwrap());
        assert!(!is_ancestor(&repo.path, &c2b, &c2a).unwrap());
    }
}

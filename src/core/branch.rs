use crate::core::commit::load_head_hash_in;
use crate::core::tree::Tree;
use crate::files::commit::load_tree_from_commit_in;
#[allow(unused_imports)]
pub use crate::files::commit::load_tree_from_commit;
use crate::views::get_view_in;
use blake3::Hash;
use std::fs::{self, File};
use std::path::Path;

/// Resolve the stored commit hash for `branch_name` in `<repo_root>/.git-med/refs/heads/<branch_name>`.
pub fn load_ref(branch_name: &str) -> Result<Option<Hash>, String> {
    load_ref_in(Path::new("."), branch_name)
}

/// Resolve the stored commit hash for `branch_name` in `<repo_root>/.git-med/refs/heads/<branch_name>`.
pub fn load_ref_in(repo_root: &Path, branch_name: &str) -> Result<Option<Hash>, String> {
    let clean_name = branch_name.trim();
    if clean_name.is_empty() {
        return Err("branch name cannot be empty".to_string());
    }

    let rel_ref = if let Some(stripped) = clean_name.strip_prefix("refs/heads/") {
        stripped
    } else {
        clean_name
    };

    let target_path = repo_root.join(".git-med/refs/heads").join(rel_ref);
    if !target_path.is_file() {
        return Ok(None);
    }

    let content = fs::read_to_string(&target_path)
        .map_err(|e| format!("failed to read ref file {}: {}", target_path.display(), e))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let hash = Hash::from_hex(trimmed)
        .map_err(|e| format!("invalid hash '{}' in ref {}: {}", trimmed, target_path.display(), e))?;
    Ok(Some(hash))
}

fn read_view_name(view_path: &Path) -> Result<String, String> {
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

/// Switch to or create `branch_name` under `repo_root`:
/// 1- Update HEAD
/// 2- If new, create a new ref
/// 3- Use the helper function to create the tree and save the new index
/// 4- Select the current view
/// 5- view.write(root)
pub fn branch_in(repo_root: &Path, branch_name: &str) -> Result<(), String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    let clean_name = branch_name.trim();
    if clean_name.is_empty() {
        return Err("branch name cannot be empty".to_string());
    }

    let rel_ref = if let Some(stripped) = clean_name.strip_prefix("refs/heads/") {
        stripped
    } else {
        clean_name
    };

    let target_ref_path = git_med_dir.join("refs/heads").join(rel_ref);
    let is_new = !target_ref_path.is_file();

    // Prior HEAD commit hash (before updating HEAD)
    let prior_commit_opt = load_head_hash_in(repo_root)?;

    // Existing target commit (if branch already existed)
    let target_commit_opt = if !is_new {
        load_ref_in(repo_root, rel_ref)?
    } else {
        None
    };

    // 1- Update HEAD
    let head_path = git_med_dir.join("HEAD");
    let head_content = format!("ref: refs/heads/{}\n", rel_ref);
    fs::write(&head_path, head_content)
        .map_err(|e| format!("failed to update HEAD {}: {}", head_path.display(), e))?;

    // 2- If new, create a new ref
    let commit_for_tree = if is_new {
        if let Some(parent) = target_ref_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create ref dir {}: {}", parent.display(), e))?;
        }
        if let Some(ref prior_hex) = prior_commit_opt {
            fs::write(&target_ref_path, format!("{}\n", prior_hex))
                .map_err(|e| format!("failed to write ref {}: {}", target_ref_path.display(), e))?;
            Some(Hash::from_hex(prior_hex.as_str()).map_err(|e| format!("invalid commit hash: {}", e))?)
        } else {
            // No prior commits exist (unborn repository)
            File::create(&target_ref_path)
                .map_err(|e| format!("failed to create empty ref {}: {}", target_ref_path.display(), e))?;
            None
        }
    } else {
        target_commit_opt
    };

    // 3- Use the helper function to create the tree and save the new index
    let tree = if let Some(commit_hash) = commit_for_tree {
        load_tree_from_commit_in(repo_root, &commit_hash)?
    } else {
        Tree::new("")
    };

    let index_path = git_med_dir.join("index");
    tree.save(&index_path)
        .map_err(|e| format!("failed to save index {}: {}", index_path.display(), e))?;

    // 4- Select the current view
    let view_name = read_view_name(&git_med_dir.join("VIEW"))?;
    let view = get_view_in(&view_name, repo_root)
        .ok_or_else(|| format!("unsupported view '{}'", view_name))?;

    // 5- view.write(root)
    view.write(&tree.root)
        .map_err(|e| format!("view write failed: {}", e))?;

    Ok(())
}

pub fn branch(args: &Vec<String>) {
    if args.len() < 3 {
        eprintln!("git-med branch failed: please provide a branch name");
        std::process::exit(1);
    }
    let branch_name = &args[2];
    if let Err(err) = branch_in(Path::new("."), branch_name) {
        eprintln!("git-med branch failed: {}", err);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::add::add_in;
    use crate::core::commit::commit_with_message_in;
    use crate::core::init::init_at;
    use crate::tester::generate::sample::qomra;
    use std::path::PathBuf;

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "git_med_branch_test_{}_{}_{}",
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

    #[test]
    fn test_load_ref_missing_and_existing() {
        let repo = TempRepo::new("load_ref");

        // Initial unborn main branch
        let initial_ref = load_ref_in(&repo.path, "main").expect("load_ref_in main");
        assert_eq!(initial_ref, None);

        // Non-existent branch
        let missing = load_ref_in(&repo.path, "nonexistent").expect("load_ref_in nonexistent");
        assert_eq!(missing, None);

        // Commit on main
        qomra(&repo.path, &[], 1, 1, 1, 1);
        add_in(&repo.path).expect("add");
        let commit_hash = commit_with_message_in(&repo.path, "initial commit").expect("commit");

        // Now main ref contains the commit hash
        let main_ref = load_ref_in(&repo.path, "main").expect("load_ref_in main after commit");
        assert_eq!(main_ref, Some(commit_hash));

        // Prefix refs/heads/ should also resolve
        let main_ref_prefixed = load_ref_in(&repo.path, "refs/heads/main").expect("load_ref_in prefixed");
        assert_eq!(main_ref_prefixed, Some(commit_hash));
    }

    #[test]
    fn test_branch_new_from_existing_commit() {
        let repo = TempRepo::new("branch_new");

        // Commit on main
        qomra(&repo.path, &[], 1, 1, 1, 1);
        add_in(&repo.path).expect("add");
        let commit_1 = commit_with_message_in(&repo.path, "initial commit").expect("commit");

        // Branch to feature
        branch_in(&repo.path, "feature").expect("branch to feature");

        // 1- HEAD should point to feature
        let head_content = fs::read_to_string(repo.path.join(".git-med/HEAD")).unwrap();
        assert_eq!(head_content.trim(), "ref: refs/heads/feature");

        // 2- refs/heads/feature should be created with commit_1
        let feature_ref = load_ref_in(&repo.path, "feature").unwrap();
        assert_eq!(feature_ref, Some(commit_1));

        // 3- index should contain the tree from commit_1
        let tree = crate::core::tree::load_or_fail_tree(&repo.path.join(".git-med/index")).unwrap();
        assert_eq!(tree.root.borrow().self_hash, Some(commit_1_tree_hash(&repo.path, &commit_1)));
    }

    fn commit_1_tree_hash(repo_root: &Path, commit_hash: &Hash) -> Hash {
        let (_, body) = crate::files::utils::read_object_bytes_in(repo_root, commit_hash).unwrap();
        let body_str = String::from_utf8(body).unwrap();
        let first_line = body_str.lines().next().unwrap();
        Hash::from_hex(first_line.trim()).unwrap()
    }

    #[test]
    fn test_branch_switch_existing_and_update_index() {
        let repo = TempRepo::new("switch_existing");

        // Commit 1 on main
        qomra(&repo.path, &[], 1, 1, 1, 1);
        add_in(&repo.path).expect("add");
        let commit_1 = commit_with_message_in(&repo.path, "commit 1").expect("commit 1");
        let tree_1_hash = commit_1_tree_hash(&repo.path, &commit_1);

        // Branch to feature
        branch_in(&repo.path, "feature").expect("branch to feature");

        // Remove a file and commit on feature
        let target_file = repo.path.join(
            "data/clinic/subject_001/study_001/series_001/instance_001.dcm",
        );
        fs::remove_file(&target_file).expect("remove file");
        add_in(&repo.path).expect("add on feature");
        let commit_2 = commit_with_message_in(&repo.path, "commit 2 on feature").expect("commit 2");
        let tree_2_hash = commit_1_tree_hash(&repo.path, &commit_2);
        assert_ne!(tree_1_hash, tree_2_hash);

        // Switch back to main
        branch_in(&repo.path, "main").expect("switch to main");
        assert_eq!(
            fs::read_to_string(repo.path.join(".git-med/HEAD")).unwrap().trim(),
            "ref: refs/heads/main"
        );
        let tree_main = crate::core::tree::load_or_fail_tree(&repo.path.join(".git-med/index")).unwrap();
        assert_eq!(tree_main.root.borrow().self_hash, Some(tree_1_hash));
        assert!(
            target_file.is_file(),
            "target file must be restored to disk by view.write when switching to main"
        );

        // Switch back to feature
        branch_in(&repo.path, "feature").expect("switch to feature");
        assert_eq!(
            fs::read_to_string(repo.path.join(".git-med/HEAD")).unwrap().trim(),
            "ref: refs/heads/feature"
        );
        let tree_feature = crate::core::tree::load_or_fail_tree(&repo.path.join(".git-med/index")).unwrap();
        assert_eq!(tree_feature.root.borrow().self_hash, Some(tree_2_hash));
    }

    #[test]
    fn test_branch_in_unborn_repo() {
        let repo = TempRepo::new("unborn");

        // In a fresh repo without commits, branch to new_branch
        branch_in(&repo.path, "new_branch").expect("branch in unborn repo");

        // HEAD points to new_branch
        assert_eq!(
            fs::read_to_string(repo.path.join(".git-med/HEAD")).unwrap().trim(),
            "ref: refs/heads/new_branch"
        );
        // Ref file is created empty
        assert!(repo.path.join(".git-med/refs/heads/new_branch").is_file());
        assert_eq!(load_ref_in(&repo.path, "new_branch").unwrap(), None);

        // Index exists
        assert!(repo.path.join(".git-med/index").is_file());
    }

    #[test]
    fn test_load_tree_from_commit_helper() {
        let repo = TempRepo::new("load_tree_helper");
        qomra(&repo.path, &[], 1, 1, 1, 1);
        add_in(&repo.path).expect("add");
        let commit_hash = commit_with_message_in(&repo.path, "commit for helper test").unwrap();

        let tree = load_tree_from_commit_in(&repo.path, &commit_hash).expect("load tree");
        assert_eq!(tree.root.borrow().id, "");
        assert!(tree.root.borrow().self_hash.is_some());
    }
}

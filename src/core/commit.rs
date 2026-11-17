use crate::core::tree::load_or_fail_tree;
use crate::files::types::ObjectType;
use crate::files::utils::{hash_bytes, make_object_from_bytes_in};
use crate::files::hash::Hash;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub tree: Hash,
    pub parent: Option<String>,
    pub message: String,
}

impl Commit {
    pub fn new(tree: Hash, parent: Option<String>, message: String) -> Self {
        Self {
            tree,
            parent,
            message,
        }
    }

    pub fn write(&self) -> Result<Hash, String> {
        self.write_in(Path::new("."))
    }

    pub fn write_in(&self, repo_root: &Path) -> Result<Hash, String> {
        let body = format_commit_body(&self.tree, self.parent.as_deref(), &self.message);
        let commit_hash = hash_bytes(&body, ObjectType::Commit);

        make_object_from_bytes_in(&body, commit_hash, ObjectType::Commit, repo_root)
            .map_err(|e| format!("failed to write commit object: {}", e))?;

        update_branch_head_in(repo_root, &commit_hash)?;

        Ok(commit_hash)
    }
}

pub fn format_commit_body(tree: &Hash, parent: Option<&str>, message: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(tree.to_hex().as_bytes());
    body.push(b'\n');

    if let Some(p) = parent {
        let trimmed_parent = p.trim();
        if !trimmed_parent.is_empty() {
            body.extend_from_slice(trimmed_parent.as_bytes());
        }
    }
    body.push(b'\n');

    body.extend_from_slice(message.as_bytes());
    if !message.ends_with('\n') {
        body.push(b'\n');
    }

    body
}

pub fn load_head_hash() -> Result<Option<String>, String> {
    load_head_hash_in(Path::new("."))
}

pub fn load_head_hash_in(repo_root: &Path) -> Result<Option<String>, String> {
    let head_path = repo_root.join(".git-med/HEAD");
    if !head_path.exists() {
        return Err("HEAD file not found".to_string());
    }

    let head_content = fs::read_to_string(&head_path)
        .map_err(|e| format!("failed to read HEAD: {}", e))?;
    let trimmed = head_content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    if let Some(ref_path) = trimmed.strip_prefix("ref:") {
        let target = repo_root.join(".git-med").join(ref_path.trim());
        if !target.exists() {
            return Ok(None);
        }
        let hash = fs::read_to_string(&target)
            .map_err(|e| format!("failed to read ref {}: {}", target.display(), e))?;
        let hash_trimmed = hash.trim().to_string();
        if hash_trimmed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hash_trimmed))
        }
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

pub fn update_branch_head_in(repo_root: &Path, commit_hash: &Hash) -> Result<(), String> {
    let head_path = repo_root.join(".git-med/HEAD");
    if !head_path.exists() {
        return Err("HEAD file not found".to_string());
    }

    let head_content = fs::read_to_string(&head_path)
        .map_err(|e| format!("failed to read HEAD: {}", e))?;
    let trimmed = head_content.trim();

    let target_ref_path = if let Some(ref_path) = trimmed.strip_prefix("ref:") {
        repo_root.join(".git-med").join(ref_path.trim())
    } else {
        head_path.clone()
    };

    let target_dir = target_ref_path
        .parent()
        .ok_or_else(|| "invalid target ref path".to_string())?;
    fs::create_dir_all(target_dir)
        .map_err(|e| format!("failed to create ref directory {}: {}", target_dir.display(), e))?;

    let filename = target_ref_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("ref");
    let tmp_seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = target_dir.join(format!(
        ".{}.tmp.{}.{}",
        filename,
        std::process::id(),
        tmp_seq
    ));

    let content = format!("{}\n", commit_hash.to_hex());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)
        .map_err(|e| format!("failed to create temp ref file {}: {}", tmp_path.display(), e))?;

    if let Err(e) = file.write_all(content.as_bytes()).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("failed to write temp ref file: {}", e));
    }

    if let Err(e) = fs::rename(&tmp_path, &target_ref_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!(
            "failed to rename {} to {}: {}",
            tmp_path.display(),
            target_ref_path.display(),
            e
        ));
    }

    Ok(())
}

pub fn parse_commit_message(args: &[String]) -> Result<String, String> {
    let mut messages: Vec<String> = Vec::new();
    let mut i = 2;
    while i < args.len() {
        if args[i] == "-m" {
            if i + 1 < args.len() {
                messages.push(args[i + 1].clone());
                i += 2;
            } else {
                return Err("-m requires a commit message".to_string());
            }
        } else if !args[i].starts_with('-') && messages.is_empty() {
            messages.push(args[i].clone());
            i += 1;
        } else {
            i += 1;
        }
    }

    if messages.is_empty() {
        Ok("Commit".to_string())
    } else {
        Ok(messages.join("\n\n"))
    }
}

pub fn commit(args: &Vec<String>) {
    let message = match parse_commit_message(args) {
        Ok(msg) => msg,
        Err(err) => {
            eprintln!("git-med commit failed: {}", err);
            std::process::exit(1);
        }
    };

    if let Err(err) = commit_with_message_in(Path::new("."), &message) {
        eprintln!("git-med commit failed: {}", err);
        std::process::exit(1);
    }
}

pub fn commit_head() -> Result<Hash, String> {
    commit_in(Path::new("."))
}

pub fn commit_in(repo_root: &Path) -> Result<Hash, String> {
    commit_with_message_in(repo_root, "Commit")
}

pub fn commit_with_message_in(repo_root: &Path, message: &str) -> Result<Hash, String> {
    let head_hash = load_head_hash_in(repo_root)?;
    let tree_index = repo_root.join(".git-med/index");
    let tree = load_or_fail_tree(&tree_index)
        .map_err(|e| format!("failed to load index: {}", e))?;
    let tree_hash = tree.hash_in(repo_root)?;

    let _ = tree.save(&tree_index);

    let commit = Commit {
        tree: tree_hash,
        parent: head_hash,
        message: message.to_string(),
    };
    commit.write_in(repo_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::init::init_at;
    use crate::files::hash::hash_raw;
    use std::path::PathBuf;

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "git_med_core_commit_test_{}_{}_{}",
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
    fn test_format_commit_body_initial() {
        let tree_hash = hash_raw(b"tree root");
        let body = format_commit_body(&tree_hash, None, "Initial commit");
        let expected = format!("{}\n\nInitial commit\n", tree_hash.to_hex());
        assert_eq!(String::from_utf8_lossy(&body), expected);
    }

    #[test]
    fn test_format_commit_body_with_parent() {
        let tree_hash = hash_raw(b"tree root 2");
        let parent_hash_str = "a".repeat(64);
        let body = format_commit_body(&tree_hash, Some(&parent_hash_str), "Second commit\n");
        let expected = format!("{}\n{}\nSecond commit\n", tree_hash.to_hex(), parent_hash_str);
        assert_eq!(String::from_utf8_lossy(&body), expected);
    }

    #[test]
    fn test_commit_write_initial_and_subsequent() {
        let repo = TempRepo::new("commit_flow");
        let tree_1 = hash_raw(b"tree 1");

        // 1. Initial commit (no parent)
        assert_eq!(load_head_hash_in(&repo.path).unwrap(), None);

        let commit_1 = Commit::new(tree_1, None, "First commit".to_string());
        let commit_1_hash = commit_1.write_in(&repo.path).expect("first commit write");

        // Verify commit object file exists and has correct format
        let hex_1 = commit_1_hash.to_hex();
        let obj_1_path = repo
            .path
            .join(".git-med/objects")
            .join(&hex_1[..2])
            .join(&hex_1[2..]);
        assert!(obj_1_path.is_file(), "commit 1 object must exist on disk");

        let obj_1_bytes = fs::read(&obj_1_path).unwrap();
        let expected_body_1 = format!("{}\n\nFirst commit\n", tree_1.to_hex());
        let expected_header_1 = format!("commit {}\0", expected_body_1.len());
        let mut expected_full_1 = Vec::new();
        expected_full_1.extend_from_slice(expected_header_1.as_bytes());
        expected_full_1.extend_from_slice(expected_body_1.as_bytes());
        assert_eq!(obj_1_bytes, expected_full_1);

        // Verify branch head is updated
        let branch_ref_path = repo.path.join(".git-med/refs/heads/main");
        let branch_content = fs::read_to_string(&branch_ref_path).unwrap();
        assert_eq!(branch_content, format!("{}\n", hex_1));
        assert_eq!(load_head_hash_in(&repo.path).unwrap(), Some(hex_1.to_string()));

        // 2. Second commit (with parent)
        let tree_2 = hash_raw(b"tree 2");
        let parent_hash = load_head_hash_in(&repo.path).unwrap();
        let commit_2 = Commit::new(tree_2, parent_hash, "Second commit".to_string());
        let commit_2_hash = commit_2.write_in(&repo.path).expect("second commit write");

        let hex_2 = commit_2_hash.to_hex();
        let obj_2_path = repo
            .path
            .join(".git-med/objects")
            .join(&hex_2[..2])
            .join(&hex_2[2..]);
        assert!(obj_2_path.is_file(), "commit 2 object must exist on disk");

        let obj_2_bytes = fs::read(&obj_2_path).unwrap();
        let expected_body_2 = format!("{}\n{}\nSecond commit\n", tree_2.to_hex(), hex_1);
        let expected_header_2 = format!("commit {}\0", expected_body_2.len());
        let mut expected_full_2 = Vec::new();
        expected_full_2.extend_from_slice(expected_header_2.as_bytes());
        expected_full_2.extend_from_slice(expected_body_2.as_bytes());
        assert_eq!(obj_2_bytes, expected_full_2);

        // Verify branch head is updated to second commit
        let branch_content_2 = fs::read_to_string(&branch_ref_path).unwrap();
        assert_eq!(branch_content_2, format!("{}\n", hex_2));
        assert_eq!(load_head_hash_in(&repo.path).unwrap(), Some(hex_2.to_string()));
    }

    #[test]
    fn test_commit_write_detached_head() {
        let repo = TempRepo::new("detached");
        let tree_hash = hash_raw(b"detached tree");

        // Make HEAD detached with an initial fake commit hash
        let initial_hash = "b".repeat(64);
        fs::write(repo.path.join(".git-med/HEAD"), format!("{}\n", initial_hash)).unwrap();

        assert_eq!(load_head_hash_in(&repo.path).unwrap(), Some(initial_hash.clone()));

        let commit = Commit::new(tree_hash, Some(initial_hash), "Detached commit".to_string());
        let commit_hash = commit.write_in(&repo.path).expect("write detached commit");

        // HEAD should be updated directly
        let head_content = fs::read_to_string(repo.path.join(".git-med/HEAD")).unwrap();
        assert_eq!(head_content, format!("{}\n", commit_hash.to_hex()));
    }

    #[test]
    fn test_parse_commit_message_variants() {
        let empty_args: Vec<String> = vec!["git-med".to_string(), "commit".to_string()];
        assert_eq!(parse_commit_message(&empty_args).unwrap(), "Commit");

        let m_flag_args: Vec<String> = vec![
            "git-med".to_string(),
            "commit".to_string(),
            "-m".to_string(),
            "custom message".to_string(),
        ];
        assert_eq!(parse_commit_message(&m_flag_args).unwrap(), "custom message");

        let positional_args: Vec<String> = vec![
            "git-med".to_string(),
            "commit".to_string(),
            "positional message".to_string(),
        ];
        assert_eq!(parse_commit_message(&positional_args).unwrap(), "positional message");

        let multiple_m_args: Vec<String> = vec![
            "git-med".to_string(),
            "commit".to_string(),
            "-m".to_string(),
            "title".to_string(),
            "-m".to_string(),
            "body".to_string(),
        ];
        assert_eq!(parse_commit_message(&multiple_m_args).unwrap(), "title\n\nbody");

        let missing_m_val: Vec<String> = vec![
            "git-med".to_string(),
            "commit".to_string(),
            "-m".to_string(),
        ];
        assert!(parse_commit_message(&missing_m_val).is_err());
    }

    #[test]
    fn test_commit_in_empty_index_fails() {
        let repo = TempRepo::new("empty_index");
        let err = commit_in(&repo.path).expect_err("commit should fail on empty index");
        assert!(err.contains("No tree found") || err.contains("failed to load index"));
    }

    #[test]
    fn test_commit_in_end_to_end_with_add() {
        use crate::core::add::add_in;

        let repo = TempRepo::new("commit_e2e");
        let series_dir = repo.path.join("data/clinic/subject_001/study_001/series_001");
        fs::create_dir_all(&series_dir).unwrap();
        fs::write(repo.path.join("README.md"), "# Test Dataset\n").unwrap();
        fs::write(series_dir.join("instance_001.dcm"), "DICOM DATA 1").unwrap();

        add_in(&repo.path).expect("add_in should succeed");

        let commit_1_hash = commit_with_message_in(&repo.path, "Initial commit")
            .expect("first commit should succeed");

        let hex_1 = commit_1_hash.to_hex();
        let head_1 = load_head_hash_in(&repo.path).unwrap();
        assert_eq!(head_1, Some(hex_1.to_string()));

        // Commit object should exist on disk
        let obj_1_path = repo
            .path
            .join(".git-med/objects")
            .join(&hex_1[..2])
            .join(&hex_1[2..]);
        assert!(obj_1_path.is_file());

        // Second commit
        fs::write(series_dir.join("instance_002.dcm"), "DICOM DATA 2").unwrap();
        add_in(&repo.path).expect("second add_in should succeed");

        let commit_2_hash = commit_with_message_in(&repo.path, "Second commit")
            .expect("second commit should succeed");

        let hex_2 = commit_2_hash.to_hex();
        let head_2 = load_head_hash_in(&repo.path).unwrap();
        assert_eq!(head_2, Some(hex_2.to_string()));

        let obj_2_path = repo
            .path
            .join(".git-med/objects")
            .join(&hex_2[..2])
            .join(&hex_2[2..]);
        assert!(obj_2_path.is_file());

        let commit_2_bytes = fs::read(&obj_2_path).unwrap();
        let commit_2_str = String::from_utf8_lossy(&commit_2_bytes);
        assert!(commit_2_str.contains(hex_1.as_str()), "commit 2 must reference commit 1 as parent");
        assert!(commit_2_str.contains("Second commit"), "commit 2 must contain its message");
    }

    #[test]
    fn test_qomra_e2e_workflow_generate_add_commit_delete_add_commit() {
        use crate::core::add::add_in;
        use crate::tester::generate::sample::qomra;

        let repo = TempRepo::new("qomra_e2e");

        // 1. Generate Qomra dataset with tester
        qomra(&repo.path, &[], 1, 1, 1, 1);

        let target_file = repo.path.join(
            "segmentation/model_v1/subject_001/study_001/series_001/instance_001_seg.nii.gz",
        );
        assert!(target_file.is_file(), "generated target file must exist");

        // 2. Add all files
        add_in(&repo.path).expect("first add_in should succeed");

        // 3. Commit initial state
        let commit_1 = commit_with_message_in(&repo.path, "feat: initial qomra commit")
            .expect("first commit should succeed");
        let hex_1 = commit_1.to_hex();
        assert_eq!(load_head_hash_in(&repo.path).unwrap(), Some(hex_1.to_string()));

        // 4. Delete a file
        fs::remove_file(&target_file).expect("remove target file");
        assert!(!target_file.exists());

        // 5. Add after delete (prunes node from tree)
        add_in(&repo.path).expect("second add_in should succeed");

        // 6. Commit after delete
        let commit_2 = commit_with_message_in(&repo.path, "remove model_v1 segmentation")
            .expect("second commit should succeed");
        let hex_2 = commit_2.to_hex();
        assert_ne!(hex_1, hex_2, "commits should have different hashes");
        assert_eq!(load_head_hash_in(&repo.path).unwrap(), Some(hex_2.to_string()));

        // 7. Verify parent linkage and commit object on disk
        let obj_2_path = repo
            .path
            .join(".git-med/objects")
            .join(&hex_2[..2])
            .join(&hex_2[2..]);
        assert!(obj_2_path.is_file());

        let commit_2_bytes = fs::read(&obj_2_path).unwrap();
        let commit_2_str = String::from_utf8_lossy(&commit_2_bytes);
        assert!(
            commit_2_str.contains(hex_1.as_str()),
            "commit 2 parent must point to commit 1"
        );
        assert!(
            commit_2_str.contains("remove model_v1 segmentation"),
            "commit 2 must contain commit message"
        );
    }
}

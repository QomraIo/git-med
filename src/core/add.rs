use crate::core::tree::load_or_init_tree;
use crate::core::tree::{ChildrenType, NodeRef, get_or_insert_child, get_or_insert_meta};
use crate::views::qomra::is_metafile;
use crate::views::traits::View;
use crate::views::{self, get_view_in};
use regex::Regex;
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

struct IndexLockGuard {
    path: PathBuf,
    armed: bool,
}

impl IndexLockGuard {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("failed to acquire {}: {}", path.display(), e))?;
        Ok(Self { path, armed: true })
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for IndexLockGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn compile_gitignore_pattern(line: &str) -> Option<Regex> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }
    let anchored = trimmed.starts_with('/');
    let pat = trimmed.trim_start_matches('/').trim_end_matches('/');
    if pat.is_empty() {
        return None;
    }

    let mut re_str = String::from(if anchored { "^" } else { "(^|/)" });
    let chars: Vec<char> = pat.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' {
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                if i + 2 < chars.len() && chars[i + 2] == '/' {
                    re_str.push_str("(.*/)?");
                    i += 3;
                } else {
                    re_str.push_str(".*");
                    i += 2;
                }
            } else {
                re_str.push_str("[^/]*");
                i += 1;
            }
        } else if chars[i] == '?' {
            re_str.push_str("[^/]");
            i += 1;
        } else {
            let mut buf = [0u8; 4];
            re_str.push_str(&regex::escape(chars[i].encode_utf8(&mut buf)));
            i += 1;
        }
    }
    re_str.push_str("(/|$)");
    Regex::new(&re_str).ok()
}

fn build_skip_patterns(repo_root: &Path) -> Vec<Regex> {
    let mut patterns = vec![
        Regex::new(r"(^|/)\.git(/|$)").unwrap(),
        Regex::new(r"(^|/)\.git-med(/|$)").unwrap(),
        Regex::new(r"(^|/)\.view").unwrap(),
    ];

    if let Ok(gitignore_content) = fs::read_to_string(repo_root.join(".gitignore")) {
        for line in gitignore_content.lines() {
            if let Some(re) = compile_gitignore_pattern(line) {
                patterns.push(re);
            }
        }
    }
    patterns
}

fn should_skip(rel_path: &Path, skip: &[Regex]) -> bool {
    let Some(path_str) = rel_path.to_str() else {
        return false;
    };
    skip.iter().any(|re| re.is_match(path_str))
}

fn normalize_rel_path(repo_root: &Path, path: &Path) -> Option<PathBuf> {
    let stripped = path.strip_prefix(repo_root).ok()?;
    let mut normalized = PathBuf::new();
    for comp in stripped.components() {
        if let Component::Normal(part) = comp {
            normalized.push(part);
        }
    }
    Some(normalized)
}

fn mark_visited(nodes: &[&NodeRef]) {
    for node in nodes {
        node.borrow_mut().visited = true;
    }
}

fn index_file_with_view(
    repo_root: &Path,
    rel_path: &Path,
    root: &NodeRef,
    view: &mut dyn View,
) -> Result<(), String> {
    if !view.is_trackable(rel_path) {
        return Ok(());
    }

    let attrs = view.index(rel_path);
    let file_id = rel_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("missing file name for {}", rel_path.display()))?;
    let err_msg = || format!("failed to index {}", rel_path.display());

    let Some(task_id) = attrs.task else {
        let meta_node = get_or_insert_meta(root, file_id);
        views::index_root_in(view, repo_root, rel_path, &meta_node).map_err(|_| err_msg())?;
        return Ok(());
    };
    let task_node =
        get_or_insert_child(root, &task_id, ChildrenType::Attributes).map_err(|_| err_msg())?;

    let Some(source_id) = attrs.source else {
        let meta_node = get_or_insert_meta(&task_node, file_id);
        views::index_task_in(view, repo_root, rel_path, &meta_node).map_err(|_| err_msg())?;
        mark_visited(&[&task_node]);
        return Ok(());
    };
    let source_node = get_or_insert_child(&task_node, &source_id, ChildrenType::Attributes)
        .map_err(|_| err_msg())?;

    let Some(subject_id) = attrs.subject else {
        let meta_node = get_or_insert_meta(&source_node, file_id);
        views::index_source_in(view, repo_root, rel_path, &meta_node, &task_id)
            .map_err(|_| err_msg())?;
        mark_visited(&[&task_node, &source_node]);
        return Ok(());
    };
    let subject_node = get_or_insert_child(&source_node, &subject_id, ChildrenType::Attributes)
        .map_err(|_| err_msg())?;

    let Some(study_id) = attrs.study else {
        let meta_node = get_or_insert_meta(&subject_node, file_id);
        views::index_subject_in(view, repo_root, rel_path, &meta_node, &task_id, &source_id)
            .map_err(|_| err_msg())?;
        mark_visited(&[&task_node, &source_node, &subject_node]);
        return Ok(());
    };
    let study_node = get_or_insert_child(&subject_node, &study_id, ChildrenType::Attributes)
        .map_err(|_| err_msg())?;

    let Some(series_id) = attrs.series else {
        let meta_node = get_or_insert_meta(&study_node, file_id);
        views::index_study_in(
            view,
            repo_root,
            rel_path,
            &meta_node,
            &task_id,
            &source_id,
            &subject_id,
        )
        .map_err(|_| err_msg())?;
        mark_visited(&[&task_node, &source_node, &subject_node, &study_node]);
        return Ok(());
    };
    let series_node =
        get_or_insert_child(&study_node, &series_id, ChildrenType::Files).map_err(|_| err_msg())?;

    let Some(instance_id) = attrs.instance else {
        let meta_node = get_or_insert_meta(&series_node, file_id);
        views::index_series_in(
            view,
            repo_root,
            rel_path,
            &meta_node,
            &task_id,
            &source_id,
            &subject_id,
            &study_id,
        )
        .map_err(|_| err_msg())?;
        mark_visited(&[
            &task_node,
            &source_node,
            &subject_node,
            &study_node,
            &series_node,
        ]);
        return Ok(());
    };

    let instance_node = get_or_insert_child(&series_node, &instance_id, ChildrenType::None)
        .map_err(|_| err_msg())?;
    let target_node = if is_metafile(rel_path) {
        get_or_insert_meta(&instance_node, file_id)
    } else {
        Rc::clone(&instance_node)
    };

    views::index_instance_in(
        view,
        repo_root,
        rel_path,
        &target_node,
        &task_id,
        &source_id,
        &subject_id,
        &study_id,
        &series_id,
    )
    .map_err(|_| err_msg())?;

    mark_visited(&[
        &task_node,
        &source_node,
        &subject_node,
        &study_node,
        &series_node,
        &instance_node,
    ]);
    Ok(())
}

fn dfs_in(repo_root: &Path, root: &NodeRef, view: &mut dyn View) -> Result<(), String> {
    let skip = build_skip_patterns(repo_root);
    let mut fringe: VecDeque<PathBuf> = VecDeque::new();
    fringe.push_back(repo_root.to_path_buf());

    while let Some(dir) = fringe.pop_front() {
        let entries =
            fs::read_dir(&dir).map_err(|e| format!("failed to read {}: {}", dir.display(), e))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| format!("failed to read entry in {}: {}", dir.display(), e))?;
            let file_type = entry
                .file_type()
                .map_err(|e| format!("failed to inspect {}: {}", entry.path().display(), e))?;
            let path = entry.path();
            let Some(rel_path) = normalize_rel_path(repo_root, &path) else {
                continue;
            };
            if rel_path.as_os_str().is_empty() || should_skip(&rel_path, &skip) {
                continue;
            }

            if file_type.is_dir() {
                fringe.push_back(path);
            } else if file_type.is_file() {
                index_file_with_view(repo_root, &rel_path, root, view)?;
            }
        }
    }
    Ok(())
}

fn read_view_name(view_path: &Path) -> Result<String, String> {
    match fs::read_to_string(view_path) {
        Ok(content) => {
            let trimmed = content.trim().to_ascii_lowercase();
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

/// Index all tracked files under `repo_root` into `<repo_root>/.git-med/index`.
pub fn add_in(repo_root: &Path) -> Result<(), String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    let mut lock = IndexLockGuard::acquire(git_med_dir.join("index.lock"))?;
    let view_name = read_view_name(&git_med_dir.join("VIEW"))?;
    let mut view = get_view_in(&view_name, repo_root)
        .ok_or_else(|| format!("unsupported view '{}'", view_name))?;

    let index_path = git_med_dir.join("index");
    let tree = load_or_init_tree(&index_path)?;
    tree.reset_visited();
    tree.root.borrow_mut().visited = true;

    dfs_in(repo_root, &tree.root, view.as_mut())?;
    tree.prune_unvisited();

    view.post_add(&tree.root)?;

    tree.save(&lock.path)
        .map_err(|e| format!("failed to save {}: {}", lock.path.display(), e))?;
    fs::rename(&lock.path, &index_path)
        .map_err(|e| format!("failed to update {}: {}", index_path.display(), e))?;
    lock.disarm();
    Ok(())
}

pub fn add(_args: &Vec<String>) {
    if let Err(err) = add_in(Path::new(".")) {
        eprintln!("git-med add failed: {}", err);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::init::init_at;
    use crate::core::tree::{Children, Tree};
    use crate::files::types::ObjectType;
    use crate::files::utils::hash;
    use crate::tester::generate::sample::qomra;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

    struct AddTempRepo {
        path: PathBuf,
    }

    impl AddTempRepo {
        fn new(prefix: &str) -> Self {
            let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "git_med_add_test_{}_{}_{}",
                prefix,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }
    }

    impl Drop for AddTempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn get_child(parent: &NodeRef, name: &str) -> Option<NodeRef> {
        let guard = parent.borrow();
        match &guard.children {
            Children::Attributes(map) => map.get(name).cloned(),
            Children::Files(map) => map.get(name).cloned(),
            Children::None => None,
        }
    }

    fn get_meta(parent: &NodeRef, filename: &str) -> Option<NodeRef> {
        parent.borrow().meta.get(filename).cloned()
    }

    fn expected_object_file(
        repo_root: &Path,
        obj_type: ObjectType,
        digest: &blake3::Hash,
    ) -> PathBuf {
        let hex = digest.to_hex();
        repo_root
            .join(".git-med")
            .join(obj_type.dir_name())
            .join(&hex[..2])
            .join(&hex[2..])
    }

    fn mark_all_hash_flags_valid(node: &NodeRef) {
        {
            let mut guard = node.borrow_mut();
            guard.valid_self_hash = true;
            guard.valid_child_hash = true;
        }
        let child_nodes: Vec<NodeRef> = match &node.borrow().children {
            Children::Attributes(map) => map.values().cloned().collect(),
            Children::Files(map) => map.values().cloned().collect(),
            Children::None => Vec::new(),
        };
        for child in child_nodes {
            mark_all_hash_flags_valid(&child);
        }
        let meta_nodes: Vec<NodeRef> = node.borrow().meta.values().cloned().collect();
        for meta in meta_nodes {
            mark_all_hash_flags_valid(&meta);
        }
    }

    #[test]
    fn test_add_requires_git_med_repo() {
        let repo = AddTempRepo::new("no_repo");
        let err = add_in(&repo.path).expect_err("add_in should fail outside repo");
        assert!(
            err.contains("not a git-med repository"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_add_respects_existing_index_lock() {
        let repo = AddTempRepo::new("lock");
        init_at(&repo.path);

        let index_path = repo.path.join(".git-med/index");
        let lock_path = repo.path.join(".git-med/index.lock");
        fs::write(&index_path, b"sentinel").expect("failed to write sentinel index");
        fs::write(&lock_path, b"locked").expect("failed to write lock file");

        let err = add_in(&repo.path).expect_err("add_in should fail when index.lock exists");
        assert!(err.contains("index.lock"), "unexpected error: {}", err);
        assert!(lock_path.exists(), "existing lock file must not be removed");
        assert_eq!(
            fs::read(&index_path).expect("failed to read index"),
            b"sentinel",
            "existing index must not be modified on lock contention"
        );
    }

    #[test]
    fn test_add_indexes_qomra_tree_metafiles_and_skip_rules() {
        let repo = AddTempRepo::new("qomra_full");
        init_at(&repo.path);

        assert_eq!(
            fs::read_to_string(repo.path.join(".git-med/VIEW")).expect("read VIEW"),
            "QOMRA\n"
        );

        qomra(&repo.path, &["data"], 1, 1, 1, 2);

        let series_dir = repo
            .path
            .join("data/clinic/subject_001/study_001/series_001");
        fs::write(repo.path.join(".gitignore"), "*.tmp\nignored_dir/\n").expect("write .gitignore");
        fs::create_dir_all(repo.path.join(".git")).expect("create .git");
        fs::write(repo.path.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write .git/HEAD");
        fs::write(repo.path.join("scratch.tmp"), "ignore me").expect("write tmp");
        fs::create_dir_all(repo.path.join("ignored_dir")).expect("create ignored_dir");
        fs::write(repo.path.join("ignored_dir/secret.dcm"), "ignore me").expect("write ignored");
        fs::write(series_dir.join(".view.instance_001.dcm"), "clinic\ndata\n")
            .expect("write sidecar");

        fs::write(repo.path.join("README.md"), "# Dataset\n").expect("write README");
        fs::write(repo.path.join("data/meta.json"), r#"{"level":"task"}"#)
            .expect("write task meta");
        fs::write(
            repo.path.join("data/clinic/source.notes.txt"),
            "clinic notes",
        )
        .expect("write source stray");
        fs::write(
            repo.path.join("data/clinic/subject_001/meta.subject.json"),
            r#"{"level":"subject"}"#,
        )
        .expect("write subject meta");
        fs::write(
            repo.path
                .join("data/clinic/subject_001/study_001/meta.study.json"),
            r#"{"level":"study"}"#,
        )
        .expect("write study meta");
        fs::write(series_dir.join("meta.series.json"), r#"{"level":"series"}"#)
            .expect("write series meta");

        add_in(&repo.path).expect("add_in should succeed");
        assert!(
            !repo.path.join(".git-med/index.lock").exists(),
            "index.lock should be removed after commit-to-index"
        );

        let tree = Tree::load(&repo.path.join(".git-med/index")).expect("load saved index");
        assert!(tree.root.borrow().visited);
        assert!(
            get_meta(&tree.root, ".gitignore").is_some(),
            ".gitignore should be indexed"
        );
        assert!(
            get_meta(&tree.root, "README.md").is_some(),
            "README.md should be indexed"
        );
        assert!(
            get_meta(&tree.root, "scratch.tmp").is_none(),
            "*.tmp must be skipped"
        );
        assert!(
            get_child(&tree.root, ".git").is_none(),
            ".git must be skipped"
        );
        assert!(
            get_child(&tree.root, "ignored_dir").is_none(),
            "ignored_dir must be skipped"
        );

        let task = get_child(&tree.root, "data").expect("missing task node");
        assert!(task.borrow().visited);
        let task_meta = get_meta(&task, "meta.json").expect("missing task meta.json");
        let expected_task_meta_hash =
            hash(&repo.path.join("data/meta.json"), None, ObjectType::Meta);
        assert_eq!(task_meta.borrow().self_hash, Some(expected_task_meta_hash));
        assert!(
            expected_object_file(&repo.path, ObjectType::Meta, &expected_task_meta_hash).is_file()
        );

        let source = get_child(&task, "clinic").expect("missing source node");
        assert!(
            get_meta(&source, "source.notes.txt").is_some(),
            "missing source stray file"
        );

        let subject = get_child(&source, "subject_001").expect("missing subject node");
        assert!(
            get_meta(&subject, "meta.subject.json").is_some(),
            "missing subject meta"
        );

        let study = get_child(&subject, "study_001").expect("missing study node");
        assert!(
            get_meta(&study, "meta.study.json").is_some(),
            "missing study meta"
        );

        let series = get_child(&study, "series_001").expect("missing series node");
        assert!(
            get_meta(&series, "meta.series.json").is_some(),
            "missing series meta"
        );
        assert!(
            get_meta(&series, ".view.instance_001.dcm").is_none(),
            ".view sidecar must be skipped"
        );

        let inst1 = get_child(&series, "instance_001.dcm").expect("missing instance_001.dcm");
        let inst2 = get_child(&series, "instance_002.dcm").expect("missing instance_002.dcm");
        assert!(inst1.borrow().visited && inst2.borrow().visited);

        let expected_blob_hash = hash(&series_dir.join("instance_001.dcm"), None, ObjectType::Blob);
        assert_eq!(inst1.borrow().self_hash, Some(expected_blob_hash));
        assert!(expected_object_file(&repo.path, ObjectType::Blob, &expected_blob_hash).is_file());
    }

    #[test]
    fn test_add_incremental_update_and_prune_deleted_files() {
        let repo = AddTempRepo::new("incremental");
        init_at(&repo.path);
        qomra(&repo.path, &["data"], 1, 1, 1, 2);

        let series_dir = repo
            .path
            .join("data/clinic/subject_001/study_001/series_001");
        let readme_path = repo.path.join("README.md");
        let series_meta_path = series_dir.join("meta.json");
        let inst1_path = series_dir.join("instance_001.dcm");
        let inst2_path = series_dir.join("instance_002.dcm");

        fs::write(&readme_path, "initial readme\n").expect("write README");
        fs::write(&series_meta_path, r#"{"v":1}"#).expect("write meta.json");

        add_in(&repo.path).expect("first add_in failed");

        let index_path = repo.path.join(".git-med/index");
        let initial_tree = Tree::load(&index_path).expect("load initial index");
        mark_all_hash_flags_valid(&initial_tree.root);
        initial_tree
            .save(&index_path)
            .expect("save marked initial index");

        let old_inst1_hash = get_child(
            &get_child(
                &get_child(
                    &get_child(
                        &get_child(&get_child(&initial_tree.root, "data").unwrap(), "clinic")
                            .unwrap(),
                        "subject_001",
                    )
                    .unwrap(),
                    "study_001",
                )
                .unwrap(),
                "series_001",
            )
            .unwrap(),
            "instance_001.dcm",
        )
        .unwrap()
        .borrow()
        .self_hash
        .unwrap();

        let old_kspace_hash = get_child(
            &get_child(
                &get_child(
                    &get_child(
                        &get_child(&get_child(&initial_tree.root, "data").unwrap(), "k-space")
                            .unwrap(),
                        "subject_001",
                    )
                    .unwrap(),
                    "study_001",
                )
                .unwrap(),
                "series_001",
            )
            .unwrap(),
            "instance_001.dcm",
        )
        .unwrap()
        .borrow()
        .self_hash
        .unwrap();

        let old_meta_hash = hash(&series_meta_path, None, ObjectType::Meta);

        thread::sleep(Duration::from_millis(20));
        fs::write(&inst1_path, b"modified dicom payload with different length")
            .expect("modify instance_001");
        fs::write(&series_meta_path, r#"{"v":2,"updated":true}"#).expect("modify meta.json");
        fs::remove_file(&inst2_path).expect("delete instance_002");
        fs::remove_file(&readme_path).expect("delete README");

        add_in(&repo.path).expect("second add_in failed");

        let updated_tree = Tree::load(&index_path).expect("load updated index");
        assert!(
            get_meta(&updated_tree.root, "README.md").is_none(),
            "deleted README.md must be pruned from root.meta"
        );
        assert!(
            !updated_tree.root.borrow().valid_self_hash,
            "root valid_self_hash must be invalidated after meta deletion"
        );
        assert!(
            !updated_tree.root.borrow().valid_child_hash,
            "root valid_child_hash must be invalidated after child updates"
        );

        let data = get_child(&updated_tree.root, "data").expect("missing data");
        assert!(!data.borrow().valid_child_hash);

        let kspace = get_child(&data, "k-space").expect("missing k-space");
        assert!(
            kspace.borrow().valid_child_hash,
            "untouched k-space subtree should preserve valid_child_hash"
        );
        let kspace_inst1 = get_child(
            &get_child(
                &get_child(&get_child(&kspace, "subject_001").unwrap(), "study_001").unwrap(),
                "series_001",
            )
            .unwrap(),
            "instance_001.dcm",
        )
        .unwrap();
        assert!(kspace_inst1.borrow().visited);
        assert_eq!(kspace_inst1.borrow().self_hash, Some(old_kspace_hash));

        let clinic = get_child(&data, "clinic").expect("missing clinic");
        let subject = get_child(&clinic, "subject_001").expect("missing subject");
        let study = get_child(&subject, "study_001").expect("missing study");
        let series = get_child(&study, "series_001").expect("missing series");
        assert!(!clinic.borrow().valid_child_hash);
        assert!(!subject.borrow().valid_child_hash);
        assert!(!study.borrow().valid_child_hash);
        assert!(!series.borrow().valid_child_hash);
        assert!(!series.borrow().valid_self_hash);

        assert!(
            get_child(&series, "instance_002.dcm").is_none(),
            "deleted instance_002.dcm must be pruned"
        );

        let new_inst1 =
            get_child(&series, "instance_001.dcm").expect("missing modified instance_001");
        let expected_new_inst1_hash = hash(&inst1_path, None, ObjectType::Blob);
        assert!(new_inst1.borrow().visited);
        assert_ne!(old_inst1_hash, expected_new_inst1_hash);
        assert_eq!(new_inst1.borrow().self_hash, Some(expected_new_inst1_hash));

        let new_meta = get_meta(&series, "meta.json").expect("missing modified meta.json");
        let expected_new_meta_hash = hash(&series_meta_path, None, ObjectType::Meta);
        assert!(new_meta.borrow().visited);
        assert_ne!(old_meta_hash, expected_new_meta_hash);
        assert_eq!(new_meta.borrow().self_hash, Some(expected_new_meta_hash));
    }
}

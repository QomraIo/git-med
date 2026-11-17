use crate::core::tree::{Children, NodeRef};
use crate::files::hash::Hash;
use crate::files::utils::read_object_bytes_in;
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use crate::views::utils::{get_name, git_add_file_in};
use std::fs::{self, read_to_string};
use std::path::{Path, PathBuf};

/// Check whether `path` represents a NIfTI file based on extension.
pub fn is_nifti(path: &Path) -> bool {
    let s = path.to_str().unwrap_or_default().to_ascii_lowercase();
    s.ends_with(".nii") || s.ends_with(".nii.gz")
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Brats {
    repo_root: PathBuf,
}

impl Default for Brats {
    fn default() -> Self {
        Self {
            repo_root: PathBuf::from("."),
        }
    }
}

impl Brats {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
        }
    }
}

impl View for Brats {
    fn is_trackable(&self, path: &Path) -> bool {
        is_nifti(path)
    }

    fn index(&mut self, path: &Path) -> ViewAttributesIndex {
        let mut ancestors = path.ancestors();

        let instance_path = ancestors.next();
        let instance_str = instance_path
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let instance = get_name(&instance_path);

        let series = Some(format!("series_{}", instance_str));

        let next_path = ancestors.next();
        let next_str = next_path
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        let (subject, study) = if next_str.starts_with("study") {
            let study = get_name(&next_path);
            let subject = get_name(&ancestors.next());
            (subject, study)
        } else {
            let subject = get_name(&next_path);
            let study = Some(format!("study_{}", next_str));
            (subject, study)
        };

        // read the sidecar .view.<filename> (source\ntask) in txt if present
        let disk_path = if path.exists() {
            path.to_path_buf()
        } else {
            self.repo_root.join(path)
        };
        let sidecar_path = disk_path
            .parent()
            .map(|p| p.join(format!(".view.{}", instance_str)))
            .filter(|p| p.exists());

        let file_to_read = sidecar_path.as_deref().unwrap_or(&disk_path);

        let others: Vec<String> = read_to_string(file_to_read)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();

        let source = Some(
            others.first().map(String::as_str).unwrap_or_default().to_string(),
        );
        let task = Some(if instance_str.contains("_seg") {
            "segmentation".to_string()
        } else {
            others.get(1).map(String::as_str).unwrap_or_default().to_string()
        });

        ViewAttributesIndex {
            subject,
            study,
            series,
            instance,
            source,
            task,
        }
    }

    fn view(&self, view_attributes: &ViewAttributes) -> (PathBuf, String) {
        let mut path = PathBuf::new().join(view_attributes.subject);
        let synthetic_study = format!("study_{}", view_attributes.subject);
        if !view_attributes.study.is_empty() && view_attributes.study != synthetic_study {
            path = path.join(view_attributes.study);
        }
        path = path.join(view_attributes.instance);

        (
            path,
            format!("{0}\n{1}", view_attributes.source, view_attributes.task),
        )
    }

    fn index_root(&self, _path: &Path, node: &NodeRef) -> Result<(), ()> {
        node.borrow_mut().visited = true;
        Ok(())
    }

    fn index_task(&self, _path: &Path, node: &NodeRef) -> Result<(), ()> {
        node.borrow_mut().visited = true;
        Ok(())
    }

    fn index_source(&self, _path: &Path, node: &NodeRef, _task_id: &str) -> Result<(), ()> {
        node.borrow_mut().visited = true;
        Ok(())
    }

    fn index_subject(
        &self,
        _path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
    ) -> Result<(), ()> {
        node.borrow_mut().visited = true;
        Ok(())
    }

    fn index_study(
        &self,
        _path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
    ) -> Result<(), ()> {
        node.borrow_mut().visited = true;
        Ok(())
    }

    fn index_series(
        &self,
        _path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
        _study_id: &str,
    ) -> Result<(), ()> {
        node.borrow_mut().visited = true;
        Ok(())
    }

    fn index_instance(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
        _study_id: &str,
        _series_id: &str,
    ) -> Result<(), ()> {
        if !is_nifti(path) {
            return Ok(());
        }
        git_add_file_in(&self.repo_root, path, node)
    }

    fn write(&self, root: &NodeRef) -> Result<(), String> {
        crate::views::utils::clean_workspace(&self.repo_root)?;
        let mut instances = Vec::new();
        collect_instances(root, 0, "", "", "", "", "", &mut instances)?;

        for entry in instances {
            let nifti_instance = if entry.instance.ends_with(".nii.gz") || entry.instance.ends_with(".nii") {
                entry.instance.clone()
            } else if let Some(stripped) = entry.instance.strip_suffix(".dcm") {
                format!("{}.nii.gz", stripped)
            } else {
                format!("{}.nii.gz", entry.instance)
            };

            let view_attrs = ViewAttributes {
                subject: &entry.subject,
                study: &entry.study,
                series: &entry.series,
                instance: &nifti_instance,
                source: &entry.source,
                task: &entry.task,
            };

            let (rel_instance_path, sidecar_content) = self.view(&view_attrs);
            let full_instance_path = self.repo_root.join(&rel_instance_path);

            if let Some(parent) = full_instance_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create dir {}: {}", parent.display(), e))?;
            }

            // 1. Write the instance file from blob
            let (_, blob_bytes) = read_object_bytes_in(&self.repo_root, &entry.blob_hash)?;
            fs::write(&full_instance_path, blob_bytes).map_err(|e| {
                format!(
                    "failed to write instance file {}: {}",
                    full_instance_path.display(),
                    e
                )
            })?;

            // 2. Write the .view.<filename> sidecar with format <source>\n<task>\n
            let instance_filename = full_instance_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| format!("invalid filename for {}", full_instance_path.display()))?;
            let sidecar_filename = format!(".view.{}", instance_filename);
            let sidecar_path = full_instance_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(sidecar_filename);
            let sidecar_bytes = format!("{}\n", sidecar_content.trim());
            fs::write(&sidecar_path, sidecar_bytes).map_err(|e| {
                format!(
                    "failed to write sidecar file {}: {}",
                    sidecar_path.display(),
                    e
                )
            })?;
        }

        Ok(())
    }
}

struct InstanceEntry {
    task: String,
    source: String,
    subject: String,
    study: String,
    series: String,
    instance: String,
    blob_hash: Hash,
}

fn collect_instances(
    node_ref: &NodeRef,
    depth: usize,
    task: &str,
    source: &str,
    subject: &str,
    study: &str,
    series: &str,
    out: &mut Vec<InstanceEntry>,
) -> Result<(), String> {
    let node = node_ref.borrow();
    match &node.children {
        Children::Attributes(map) | Children::Files(map) => {
            for (child_id, child_node) in map {
                match depth {
                    0 => collect_instances(child_node, 1, child_id, source, subject, study, series, out)?,
                    1 => collect_instances(child_node, 2, task, child_id, subject, study, series, out)?,
                    2 => collect_instances(child_node, 3, task, source, child_id, study, series, out)?,
                    3 => collect_instances(child_node, 4, task, source, subject, child_id, series, out)?,
                    4 => collect_instances(child_node, 5, task, source, subject, study, child_id, out)?,
                    _ => {
                        let is_leaf = matches!(child_node.borrow().children, Children::None);
                        if is_leaf {
                            let blob_hash = child_node.borrow().self_hash.ok_or_else(|| {
                                format!("leaf node {} missing self_hash", child_id)
                            })?;
                            out.push(InstanceEntry {
                                task: task.to_string(),
                                source: source.to_string(),
                                subject: subject.to_string(),
                                study: study.to_string(),
                                series: series.to_string(),
                                instance: child_id.clone(),
                                blob_hash,
                            });
                        } else {
                            collect_instances(child_node, depth + 1, task, source, subject, study, child_id, out)?;
                        }
                    }
                }
            }
        }
        Children::None => {
            if depth > 0 {
                let blob_hash = node.self_hash.ok_or_else(|| {
                    format!("leaf node {} missing self_hash", node.id)
                })?;
                out.push(InstanceEntry {
                    task: task.to_string(),
                    source: source.to_string(),
                    subject: subject.to_string(),
                    study: study.to_string(),
                    series: series.to_string(),
                    instance: node.id.clone(),
                    blob_hash,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, remove_dir_all, write};
    use std::path::{Path, PathBuf};

    use crate::core::add::add_in;
    use crate::core::tree::{ChildrenType, Node};
    use crate::files::commit::save_tree_in;
    use crate::tester::generate::sample::brats;

    struct TestContext {
        output_dir: String,
    }

    impl TestContext {
        fn setup() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let output_dir = format!("_tmp_views_brats_test_{}_{}", std::process::id(), id);

            let n_subjects = 1;
            let study_per_subjects = 1;
            let file_per_study = 5;
            brats(
                Path::new(&output_dir),
                n_subjects,
                study_per_subjects,
                file_per_study,
            );
            Self { output_dir }
        }
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            println!("Tearing down: deleting {0}", self.output_dir);
            let _ = remove_dir_all(&self.output_dir);
        }
    }

    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_index() {
        let ctx = TestContext::setup();

        let file_path = Path::new(&ctx.output_dir)
            .join("subject_001")
            .join("study_001")
            .join("subject_001_study_001_t1.nii.gz");

        let mut brats = Brats::default();
        let brats = brats.index(&file_path);

        assert_eq!(brats.subject.as_deref(), Some("subject_001"));
        assert_eq!(brats.study.as_deref(), Some("study_001"));
        assert_eq!(
            brats.series.as_deref(),
            Some("series_subject_001_study_001_t1.nii.gz")
        );
        assert_eq!(
            brats.instance.as_deref(),
            Some("subject_001_study_001_t1.nii.gz")
        );
        assert_eq!(brats.source.as_deref(), Some("clinic"));
        assert_eq!(brats.task.as_deref(), Some("data"));
    }

    #[test]
    fn test_index_seg() {
        let ctx = TestContext::setup();

        let file_path = Path::new(&ctx.output_dir)
            .join("subject_001")
            .join("study_001")
            .join("subject_001_study_001_seg.nii.gz");

        let mut brats = Brats::default();
        let brats = brats.index(&file_path);

        assert_eq!(brats.subject.as_deref(), Some("subject_001"));
        assert_eq!(brats.study.as_deref(), Some("study_001"));
        assert_eq!(
            brats.series.as_deref(),
            Some("series_subject_001_study_001_seg.nii.gz")
        );
        assert_eq!(
            brats.instance.as_deref(),
            Some("subject_001_study_001_seg.nii.gz")
        );
        assert_eq!(brats.source.as_deref(), Some("consensus"));
        assert_eq!(brats.task.as_deref(), Some("segmentation"));
    }

    #[test]
    fn test_view() {
        let _ctx = TestContext::setup();

        let brats = Brats::default();

        let view_attributes = ViewAttributes {
            subject: "subject_001",
            study: "study_001",
            series: "series_subject_001_study_001_t1.nii.gz",
            instance: "subject_001_study_001_t1.nii.gz",
            source: "clinic",
            task: "data",
        };

        let (path, content) = brats.view(&view_attributes);

        let expected_path = PathBuf::new()
            .join("subject_001")
            .join("study_001")
            .join("subject_001_study_001_t1.nii.gz");

        assert_eq!(path, expected_path);
        assert_eq!(content, "clinic\ndata");
    }

    #[test]
    fn test_is_nifti_and_trackable() {
        let brats = Brats::default();
        assert!(is_nifti(Path::new("scan.nii")));
        assert!(is_nifti(Path::new("scan.nii.gz")));
        assert!(is_nifti(Path::new("SCAN.NII.GZ")));
        assert!(!is_nifti(Path::new("notes.txt")));
        assert!(!is_nifti(Path::new("meta.json")));

        assert!(brats.is_trackable(Path::new("sub/scan.nii.gz")));
        assert!(!brats.is_trackable(Path::new("sub/readme.md")));
    }

    #[test]
    fn test_metadata_indexing_marks_visited() {
        let brats = Brats::default();
        let dummy_path = Path::new("dummy");

        let root_node = Node::new("", ChildrenType::Attributes);
        assert!(!root_node.borrow().visited);
        assert!(brats.index_root(dummy_path, &root_node).is_ok());
        assert!(root_node.borrow().visited);
        assert!(root_node.borrow().self_hash.is_none());

        let task_node = Node::new("data", ChildrenType::Attributes);
        assert!(brats.index_task(dummy_path, &task_node).is_ok());
        assert!(task_node.borrow().visited);
        assert!(task_node.borrow().self_hash.is_none());

        let source_node = Node::new("clinic", ChildrenType::Attributes);
        assert!(brats.index_source(dummy_path, &source_node, "data").is_ok());
        assert!(source_node.borrow().visited);
        assert!(source_node.borrow().self_hash.is_none());

        let subject_node = Node::new("sub1", ChildrenType::Attributes);
        assert!(brats.index_subject(dummy_path, &subject_node, "data", "clinic").is_ok());
        assert!(subject_node.borrow().visited);
        assert!(subject_node.borrow().self_hash.is_none());

        let study_node = Node::new("std1", ChildrenType::Attributes);
        assert!(brats.index_study(dummy_path, &study_node, "data", "clinic", "sub1").is_ok());
        assert!(study_node.borrow().visited);
        assert!(study_node.borrow().self_hash.is_none());

        let series_node = Node::new("ser1", ChildrenType::Files);
        assert!(brats.index_series(dummy_path, &series_node, "data", "clinic", "sub1", "std1").is_ok());
        assert!(series_node.borrow().visited);
        assert!(series_node.borrow().self_hash.is_none());
    }

    #[test]
    fn test_index_instance_hashes_blob_and_skips_non_nifti() {
        let ctx = TestContext::setup();
        let repo_path = Path::new(&ctx.output_dir);
        let git_med_dir = repo_path.join(".git-med");
        create_dir_all(&git_med_dir).unwrap();

        let brats = Brats::new(repo_path);
        let rel_file = Path::new("subject_001")
            .join("study_001")
            .join("subject_001_study_001_t1.nii.gz");

        let node = Node::new("subject_001_study_001_t1.nii.gz", ChildrenType::None);
        assert!(node.borrow().self_hash.is_none());
        assert!(!node.borrow().visited);

        let res = brats.index_instance(
            &rel_file,
            &node,
            "data",
            "clinic",
            "subject_001",
            "study_001",
            "series_subject_001_study_001_t1.nii.gz",
        );
        assert!(res.is_ok());
        assert!(node.borrow().visited);
        let self_hash = node.borrow().self_hash;
        assert!(self_hash.is_some());

        // Check object was written to .git-med/objects/
        let hex = self_hash.unwrap().to_hex();
        let object_path = git_med_dir
            .join("objects")
            .join(&hex[..2])
            .join(&hex[2..]);
        assert!(object_path.exists());

        // Non-nifti file: index_instance should return Ok(()) without setting self_hash
        let non_nifti_node = Node::new("notes.txt", ChildrenType::None);
        let non_nifti_res = brats.index_instance(
            Path::new("notes.txt"),
            &non_nifti_node,
            "data",
            "clinic",
            "subject_001",
            "study_001",
            "series_notes",
        );
        assert!(non_nifti_res.is_ok());
        assert!(non_nifti_node.borrow().self_hash.is_none());
    }

    #[test]
    fn test_brats_add_end_to_end() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let repo_dir = PathBuf::from(format!("_tmp_brats_add_e2e_{}_{}", std::process::id(), id));
        let _guard = TempDirGuard(repo_dir.clone());

        // Create sample dataset
        brats(&repo_dir, 1, 1, 2);

        // Initialize .git-med structure
        let git_med_dir = repo_dir.join(".git-med");
        create_dir_all(&git_med_dir).unwrap();
        write(git_med_dir.join("VIEW"), "brats").unwrap();

        // Also add an untracked non-NIfTI file to verify it's skipped cleanly
        write(repo_dir.join("README.txt"), "This should be ignored").unwrap();

        // Run add_in
        let add_res = add_in(&repo_dir);
        assert!(add_res.is_ok(), "add_in failed: {:?}", add_res.err());

        // Load the tree and verify commit/save_tree works cleanly without missing hash errors
        let index_path = git_med_dir.join("index");
        let tree = crate::core::tree::load_or_init_tree(&index_path).expect("load tree");

        let commit_hash = save_tree_in(tree.root, &repo_dir);
        assert!(commit_hash.is_ok(), "save_tree_in failed: {:?}", commit_hash.err());

        let _ = remove_dir_all(&repo_dir);
    }

    #[test]
    fn test_write_nested_with_sidecars() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let repo_dir = PathBuf::from(format!("_tmp_brats_write_nested_{}_{}", std::process::id(), id));
        let _guard = TempDirGuard(repo_dir.clone());

        // Create sample dataset: 1 subject, 1 study, 2 files
        brats(&repo_dir, 1, 1, 2);

        // Initialize .git-med
        let git_med_dir = repo_dir.join(".git-med");
        create_dir_all(&git_med_dir).unwrap();
        write(git_med_dir.join("VIEW"), "brats").unwrap();

        // Index and commit
        add_in(&repo_dir).expect("initial add");
        let index_path = git_med_dir.join("index");
        let tree = crate::core::tree::load_or_init_tree(&index_path).expect("load tree");
        let orig_hash = save_tree_in(tree.root.clone(), &repo_dir).expect("save tree");

        // Delete working copy files
        let subject_dir = repo_dir.join("subject_001");
        remove_dir_all(&subject_dir).expect("delete working directory");
        assert!(!subject_dir.exists());

        // Call write to reconstruct
        let brats_view = Brats::new(&repo_dir);
        let write_res = brats_view.write(&tree.root);
        assert!(write_res.is_ok(), "write failed: {:?}", write_res.err());

        // Verify files and sidecars exist with exact content
        let file1 = subject_dir.join("study_001").join("subject_001_study_001_t1.nii.gz");
        let sidecar1 = subject_dir.join("study_001").join(".view.subject_001_study_001_t1.nii.gz");
        assert!(file1.exists(), "file1 should exist");
        assert!(sidecar1.exists(), "sidecar1 should exist");
        let content1 = fs::read_to_string(&sidecar1).unwrap();
        assert_eq!(content1, "clinic\ndata\n");

        let file2 = subject_dir.join("study_001").join("subject_001_study_001_t1ce.nii.gz");
        let sidecar2 = subject_dir.join("study_001").join(".view.subject_001_study_001_t1ce.nii.gz");
        assert!(file2.exists(), "file2 should exist");
        assert!(sidecar2.exists(), "sidecar2 should exist");
        let content2 = fs::read_to_string(&sidecar2).unwrap();
        assert_eq!(content2, "clinic\ndata\n");

        // Round-trip re-index matches original tree hash
        add_in(&repo_dir).expect("re-add");
        let tree2 = crate::core::tree::load_or_init_tree(&index_path).expect("load tree2");
        let re_hash = save_tree_in(tree2.root, &repo_dir).expect("save tree2");
        assert_eq!(orig_hash, re_hash);

        let _ = remove_dir_all(&repo_dir);
    }

    #[test]
    fn test_write_flat_with_sidecars() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let repo_dir = PathBuf::from(format!("_tmp_brats_write_flat_{}_{}", std::process::id(), id));
        let _guard = TempDirGuard(repo_dir.clone());

        // Create sample dataset: 1 subject, 0 study (flat), 2 files
        brats(&repo_dir, 1, 0, 2);

        // Initialize .git-med
        let git_med_dir = repo_dir.join(".git-med");
        create_dir_all(&git_med_dir).unwrap();
        write(git_med_dir.join("VIEW"), "brats").unwrap();

        // Index and commit
        add_in(&repo_dir).expect("initial add");
        let index_path = git_med_dir.join("index");
        let tree = crate::core::tree::load_or_init_tree(&index_path).expect("load tree");
        let orig_hash = save_tree_in(tree.root.clone(), &repo_dir).expect("save tree");

        // Delete working copy files
        let subject_dir = repo_dir.join("subject_001");
        remove_dir_all(&subject_dir).expect("delete working directory");
        assert!(!subject_dir.exists());

        // Call write to reconstruct
        let brats_view = Brats::new(&repo_dir);
        let write_res = brats_view.write(&tree.root);
        assert!(write_res.is_ok(), "write failed: {:?}", write_res.err());

        // Verify flat layout: files are directly under subject_001, NO study_subject_001 subdirectory!
        assert!(!subject_dir.join("study_subject_001").exists());

        let file1 = subject_dir.join("subject_001_t1.nii.gz");
        let sidecar1 = subject_dir.join(".view.subject_001_t1.nii.gz");
        assert!(file1.exists(), "flat file1 should exist");
        assert!(sidecar1.exists(), "flat sidecar1 should exist");
        let content1 = fs::read_to_string(&sidecar1).unwrap();
        assert_eq!(content1, "clinic\ndata\n");

        let file2 = subject_dir.join("subject_001_t1ce.nii.gz");
        let sidecar2 = subject_dir.join(".view.subject_001_t1ce.nii.gz");
        assert!(file2.exists(), "flat file2 should exist");
        assert!(sidecar2.exists(), "flat sidecar2 should exist");
        let content2 = fs::read_to_string(&sidecar2).unwrap();
        assert_eq!(content2, "clinic\ndata\n");

        // Round-trip re-index matches original tree hash
        add_in(&repo_dir).expect("re-add");
        let tree2 = crate::core::tree::load_or_init_tree(&index_path).expect("load tree2");
        let re_hash = save_tree_in(tree2.root, &repo_dir).expect("save tree2");
        assert_eq!(orig_hash, re_hash);

        let _ = remove_dir_all(&repo_dir);
    }
}

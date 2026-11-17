use crate::core::tree::{Children, NodeRef};
use crate::files::utils::read_object_bytes_in;
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Qomra {
    repo_root: PathBuf,
}

impl Default for Qomra {
    fn default() -> Self {
        Self {
            repo_root: PathBuf::from("."),
        }
    }
}

impl Qomra {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
        }
    }

    fn add_meta_with_max_depth(
        &self,
        path: &Path,
        node: &NodeRef,
        max_depth: usize,
    ) -> Result<(), ()> {
        if normalized_depth(path) > max_depth {
            Err(())
        } else {
            git_add_metafile_in(&self.repo_root, path, node)
        }
    }

    fn write_node(&self, node_ref: &NodeRef, current_dir: &Path) -> Result<(), String> {
        let node = node_ref.borrow();

        // 1. Write metadata files belonging to this node into current_dir
        for (meta_id, meta_node) in &node.meta {
            let filename = meta_id.as_str();
            if filename.is_empty() {
                continue;
            }
            let meta_hash = meta_node.borrow().self_hash.ok_or_else(|| {
                format!("meta node {} missing self_hash", meta_node.borrow().id)
            })?;
            let (_, content) = read_object_bytes_in(&self.repo_root, &meta_hash)?;
            fs::create_dir_all(current_dir)
                .map_err(|e| format!("failed to create dir {}: {}", current_dir.display(), e))?;
            let file_path = current_dir.join(filename);
            fs::write(&file_path, content)
                .map_err(|e| format!("failed to write meta file {}: {}", file_path.display(), e))?;
        }

        // 2. Write children
        match &node.children {
            Children::Attributes(map) | Children::Files(map) => {
                for (child_id, child_node) in map {
                    let name = child_id.as_str();
                    if name.is_empty() {
                        continue;
                    }
                    let is_leaf = matches!(child_node.borrow().children, Children::None);
                    if is_leaf {
                        let file_path = current_dir.join(name);
                        let blob_hash = child_node.borrow().self_hash.ok_or_else(|| {
                            format!("leaf node {} missing self_hash", child_node.borrow().id)
                        })?;
                        let (_, content) = read_object_bytes_in(&self.repo_root, &blob_hash)?;
                        fs::create_dir_all(current_dir).map_err(|e| {
                            format!("failed to create dir {}: {}", current_dir.display(), e)
                        })?;
                        fs::write(&file_path, content).map_err(|e| {
                            format!("failed to write file {}: {}", file_path.display(), e)
                        })?;

                        // Write any metadata attached to the leaf node
                        for (meta_id, meta_node) in &child_node.borrow().meta {
                            let meta_filename = meta_id.as_str();
                            if meta_filename.is_empty() {
                                continue;
                            }
                            let meta_hash = meta_node.borrow().self_hash.ok_or_else(|| {
                                format!(
                                    "meta node {} missing self_hash",
                                    meta_node.borrow().id
                                )
                            })?;
                            let (_, meta_content) =
                                read_object_bytes_in(&self.repo_root, &meta_hash)?;
                            let meta_file_path = current_dir.join(meta_filename);
                            fs::write(&meta_file_path, meta_content).map_err(|e| {
                                format!(
                                    "failed to write meta file {}: {}",
                                    meta_file_path.display(),
                                    e
                                )
                            })?;
                        }
                    } else {
                        let subdir = current_dir.join(name);
                        fs::create_dir_all(&subdir).map_err(|e| {
                            format!("failed to create subdir {}: {}", subdir.display(), e)
                        })?;
                        self.write_node(child_node, &subdir)?;
                    }
                }
            }
            Children::None => {}
        }

        Ok(())
    }
}

/// Return `true` if `path`'s filename matches `meta.*.json` (including `meta.json` and `<id>.json`).
pub fn is_metafile(path: &Path) -> bool {
    path.file_name()
        .and_then(|f| f.to_str())
        .map_or(false, |s| s.ends_with(".json"))
}

fn normalized_depth(path: &Path) -> usize {
    path.components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

pub use crate::views::utils::{
    add_node_object_in, git_add_file, git_add_file_in, git_add_metafile, git_add_metafile_in,
};

impl View for Qomra {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex {
        let comps: Vec<String> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str().map(String::from),
                _ => None,
            })
            .collect();

        let mut attrs = ViewAttributesIndex {
            subject: None,
            study: None,
            series: None,
            instance: None,
            source: None,
            task: None,
        };

        if comps.len() >= 2 {
            attrs.task = Some(comps[0].clone());
        }
        if comps.len() >= 3 {
            attrs.source = Some(comps[1].clone());
        }
        if comps.len() >= 4 {
            attrs.subject = Some(comps[2].clone());
        }
        if comps.len() >= 5 {
            attrs.study = Some(comps[3].clone());
        }
        if comps.len() >= 6 {
            attrs.series = Some(comps[4].clone());
        }
        if comps.len() > 6 || (comps.len() == 6 && !is_metafile(path)) {
            attrs.instance = Some(comps[5].clone());
        }

        attrs
    }

    fn view(&self, view_attributes: &ViewAttributes) -> (PathBuf, String) {
        (
            PathBuf::new()
                .join(view_attributes.task)
                .join(view_attributes.source)
                .join(view_attributes.subject)
                .join(view_attributes.study)
                .join(view_attributes.series)
                .join(view_attributes.instance),
            String::new(),
        )
    }

    fn index_root(&self, path: &Path, node: &NodeRef) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 1)
    }

    fn index_task(&self, path: &Path, node: &NodeRef) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 2)
    }

    fn index_source(&self, path: &Path, node: &NodeRef, _task_id: &str) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 3)
    }

    fn index_subject(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
    ) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 4)
    }

    fn index_study(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
    ) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 5)
    }

    fn index_series(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
        _study_id: &str,
    ) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 6)
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
        if normalized_depth(path) > 7 {
            Err(())
        } else if is_metafile(path) {
            git_add_metafile_in(&self.repo_root, path, node)
        } else {
            git_add_file_in(&self.repo_root, path, node)
        }
    }

    fn write(&self, root: &NodeRef) -> Result<(), String> {
        crate::views::utils::clean_workspace(&self.repo_root)?;
        self.write_node(root, &self.repo_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::types::ObjectType;
    use std::fs::remove_dir_all;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use crate::tester::generate::sample::qomra;
    struct TestContext {
        output_dir: String,
    }

    impl TestContext {
        // PRE-TEST PROCESS (Setup)
        fn setup() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let output_dir = format!("_tmp_views_qomra_test_{}_{}", std::process::id(), id);

            let n_subjects = 1;
            let study_per_subjects = 1;
            let series_per_study = 1;
            let file_per_series = 1;
            qomra(
                &Path::new(&output_dir),
                &["data"],
                n_subjects,
                study_per_subjects,
                series_per_study,
                file_per_series,
            );
            Self { output_dir }
        }
    }

    impl Drop for TestContext {
        // POST-TEST PROCESS (Teardown)
        // Automatically called when `_ctx` goes out of scope, even on panic!
        fn drop(&mut self) {
            println!("Tearing down: deleting {0}", self.output_dir);
            let _ = remove_dir_all(&self.output_dir);
        }
    }

    #[test]
    fn test_index() {
        let ctx = TestContext::setup();

        let file_path = Path::new(&ctx.output_dir)
            .join("data")
            .join("clinic")
            .join("subject_001")
            .join("study_001")
            .join("series_001")
            .join("instance_001.dcm");
        let rel_path = file_path.strip_prefix(&ctx.output_dir).unwrap();

        let mut qomra = Qomra::default();
        let indexed = qomra.index(rel_path);

        assert_eq!(indexed.task.as_deref(), Some("data"));
        assert_eq!(indexed.source.as_deref(), Some("clinic"));
        assert_eq!(indexed.subject.as_deref(), Some("subject_001"));
        assert_eq!(indexed.study.as_deref(), Some("study_001"));
        assert_eq!(indexed.series.as_deref(), Some("series_001"));
        assert_eq!(indexed.instance.as_deref(), Some("instance_001.dcm"));
    }

    #[test]
    fn test_index_shallow_and_stray_metafiles() {
        let mut qomra = Qomra::default();

        for root_file in ["README.md", ".gitignore", "meta.json", "./README.md"] {
            let attrs = qomra.index(Path::new(root_file));
            assert!(attrs.task.is_none());
            assert!(attrs.instance.is_none());
        }

        let task_meta = qomra.index(Path::new("./data/README.md"));
        assert_eq!(task_meta.task.as_deref(), Some("data"));
        assert!(task_meta.source.is_none());

        let series_meta = qomra.index(Path::new(
            "data/clinic/subject_001/study_001/series_001/meta.json",
        ));
        assert_eq!(series_meta.task.as_deref(), Some("data"));
        assert_eq!(series_meta.series.as_deref(), Some("series_001"));
        assert!(series_meta.instance.is_none());

        assert!(is_metafile(Path::new("meta.json")));
        assert!(is_metafile(Path::new("meta.extra.json")));
        assert!(!is_metafile(Path::new("README.md")));
        assert!(!is_metafile(Path::new("metajson")));
    }

    #[test]
    fn test_view() {
        let _ctx = TestContext::setup();

        let qomra = Qomra::default();

        let view_attributes = ViewAttributes {
            subject: "subject_001",
            study: "study_001",
            series: "series_001",
            instance: "instance_001.dcm",
            source: "clinic",
            task: "data",
        };

        let (path, content) = qomra.view(&view_attributes);

        let expected_path = PathBuf::new()
            .join("data")
            .join("clinic")
            .join("subject_001")
            .join("study_001")
            .join("series_001")
            .join("instance_001.dcm");

        assert_eq!(path, expected_path);
        assert_eq!(content, "");
    }

    #[test]
    fn test_qomra_write_root_meta_and_tree_hierarchy() {
        use crate::core::tree::{ChildrenType, Node};
        use crate::files::commit::save_tree_in;
        use crate::files::utils::make_object_from_bytes_in;

        let temp_dir = std::env::temp_dir().join(format!(
            "git_med_qomra_write_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join(".git-med/objects")).unwrap();

        let qomra = Qomra::new(&temp_dir);

        // 1. Root node
        let root = Node::new("", ChildrenType::Attributes);

        // 2. Root metadata file (meta.json)
        let root_meta_payload = b"{\"repo\": \"root_meta\"}";
        let root_meta_hash = blake3::hash(root_meta_payload);
        make_object_from_bytes_in(
            root_meta_payload,
            root_meta_hash,
            ObjectType::Meta,
            &temp_dir,
        )
        .unwrap();

        let root_meta_id = "meta.json";
        let root_meta_node = Node::new(root_meta_id, ChildrenType::None);
        root_meta_node.borrow_mut().self_hash = Some(root_meta_hash);
        root_meta_node.borrow_mut().valid_self_hash = true;
        root.borrow_mut().meta.insert(root_meta_id.to_string(), root_meta_node);

        // 3. Task attribute folder: "data"
        let task_id = "data";
        let task_node = Node::new(task_id, ChildrenType::Attributes);

        // 4. Source attribute folder: "clinic"
        let source_id = "clinic";
        let source_node = Node::new(source_id, ChildrenType::Attributes);

        // Meta on clinic folder: "meta.clinic.json"
        let clinic_meta_payload = b"{\"clinic\": \"data_meta\"}";
        let clinic_meta_hash = blake3::hash(clinic_meta_payload);
        make_object_from_bytes_in(
            clinic_meta_payload,
            clinic_meta_hash,
            ObjectType::Meta,
            &temp_dir,
        )
        .unwrap();

        let clinic_meta_id = "meta.clinic.json";
        let clinic_meta_node = Node::new(clinic_meta_id, ChildrenType::None);
        clinic_meta_node.borrow_mut().self_hash = Some(clinic_meta_hash);
        clinic_meta_node.borrow_mut().valid_self_hash = true;
        source_node
            .borrow_mut()
            .meta
            .insert(clinic_meta_id.to_string(), clinic_meta_node);

        // 5. Series folder: "series_001"
        let series_id = "series_001";
        let series_node = Node::new(series_id, ChildrenType::Files);

        // 6. Leaf instance file: "slice.dcm"
        let leaf_id = "slice.dcm";
        let leaf_node = Node::new(leaf_id, ChildrenType::None);
        let leaf_blob_payload = b"SAMPLE DICOM BLOB CONTENT";
        let leaf_blob_hash = blake3::hash(leaf_blob_payload);
        make_object_from_bytes_in(
            leaf_blob_payload,
            leaf_blob_hash,
            ObjectType::Blob,
            &temp_dir,
        )
        .unwrap();
        leaf_node.borrow_mut().self_hash = Some(leaf_blob_hash);
        leaf_node.borrow_mut().valid_self_hash = true;

        // Build hierarchy: root -> task -> source -> series -> leaf
        Node::insert(
            &series_node,
            crate::core::tree::ChildType::Ref(Rc::clone(&leaf_node)),
        )
        .unwrap();
        Node::insert(
            &source_node,
            crate::core::tree::ChildType::Ref(Rc::clone(&series_node)),
        )
        .unwrap();
        Node::insert(
            &task_node,
            crate::core::tree::ChildType::Ref(Rc::clone(&source_node)),
        )
        .unwrap();
        Node::insert(
            &root,
            crate::core::tree::ChildType::Ref(Rc::clone(&task_node)),
        )
        .unwrap();

        // Save tree objects to .git-med/objects
        save_tree_in(Rc::clone(&root), &temp_dir).unwrap();

        // Execute view.write(&root)
        qomra.write(&root).expect("qomra write must succeed");

        // Verify root metadata file exists in root
        let root_meta_path = temp_dir.join("meta.json");
        assert!(root_meta_path.is_file(), "root meta.json must exist in root");
        assert_eq!(
            fs::read(&root_meta_path).unwrap(),
            root_meta_payload,
            "root meta content must match"
        );

        // Verify clinic metadata file exists in data/clinic/meta.clinic.json
        let clinic_meta_path = temp_dir.join("data/clinic/meta.clinic.json");
        assert!(
            clinic_meta_path.is_file(),
            "meta.clinic.json must exist in data/clinic"
        );
        assert_eq!(
            fs::read(&clinic_meta_path).unwrap(),
            clinic_meta_payload
        );

        // Verify leaf file exists in data/clinic/series_001/slice.dcm
        let leaf_file_path = temp_dir.join("data/clinic/series_001/slice.dcm");
        assert!(
            leaf_file_path.is_file(),
            "slice.dcm must exist in data/clinic/series_001"
        );
        assert_eq!(
            fs::read(&leaf_file_path).unwrap(),
            leaf_blob_payload
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

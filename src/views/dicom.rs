use crate::core::tree::{Children, ChildrenType, Node, NodeMetadata, NodeRef};
use crate::files::dicom::{parse_dicom_header, reconstruct_dicom_bytes, save_or_merge_draft};
use crate::files::nifti::{dicom_to_nifti2, extract_nifti2_pixel_data};
use crate::files::types::ObjectType;
use crate::files::utils::{
    get_file_metadata, hash_bytes, make_object_from_bytes_in, read_object_bytes_in,
};
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use crate::views::utils::get_name;
use std::collections::BTreeMap;
use std::fs::{self, read_to_string};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Check whether `path` represents a DICOM file based on extension.
pub fn is_dcm(path: &Path) -> bool {
    let s = path.to_str().unwrap_or_default().to_ascii_lowercase();
    s.ends_with(".dcm")
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Dicom {
    repo_root: PathBuf,
}

impl Default for Dicom {
    fn default() -> Self {
        Self {
            repo_root: PathBuf::from("."),
        }
    }
}

impl Dicom {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
        }
    }
}

impl View for Dicom {
    fn is_trackable(&self, path: &Path) -> bool {
        is_dcm(path)
    }

    fn index(&mut self, path: &Path) -> ViewAttributesIndex {
        let mut ancestors = path.ancestors();

        let instance_path = ancestors.next();
        let instance_str = instance_path
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        let instance = get_name(&instance_path);
        let series = get_name(&ancestors.next());
        let study = get_name(&ancestors.next());
        let subject = get_name(&ancestors.next());

        let disk_path = if path.is_absolute() || path.exists() {
            path.to_path_buf()
        } else {
            self.repo_root.join(path)
        };

        let sidecar_path = disk_path
            .parent()
            .map(|p| p.join(format!(".view.{}", instance_str)))
            .filter(|p| p.exists());

        let (source, task) = if let Some(ref sc) = sidecar_path {
            let lines: Vec<String> = read_to_string(sc)
                .unwrap_or_default()
                .lines()
                .map(String::from)
                .collect();
            let src = lines
                .get(0)
                .cloned()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "clinic".to_string());
            let tsk = lines
                .get(1)
                .cloned()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "data".to_string());
            (Some(src), Some(tsk))
        } else {
            (Some("clinic".to_string()), Some("data".to_string()))
        };

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
        (
            PathBuf::new()
                .join(view_attributes.subject)
                .join(view_attributes.study)
                .join(view_attributes.series)
                .join(view_attributes.instance),
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
        subject_id: &str,
        study_id: &str,
        series_id: &str,
    ) -> Result<(), ()> {
        let disk_path = if path.is_absolute() || path.exists() {
            path.to_path_buf()
        } else {
            self.repo_root.join(path)
        };

        // 1. Parse header using streaming parser
        let meta = parse_dicom_header(&disk_path).map_err(|_| ())?;

        // 2. Locate ancestor nodes: series -> study -> subject
        let series_node = node.borrow().parent().ok_or(())?;
        let study_node = series_node.borrow().parent().ok_or(())?;
        let subject_node = study_node.borrow().parent().ok_or(())?;

        // 3. Update draft files in .git-med/draft_object/
        let draft_dir = self.repo_root.join(".git-med").join("draft_object");
        save_or_merge_draft(&draft_dir, series_id, &meta.series).map_err(|_| ())?;
        save_or_merge_draft(&draft_dir, study_id, &meta.study).map_err(|_| ())?;
        save_or_merge_draft(&draft_dir, subject_id, &meta.patient).map_err(|_| ())?;

        // 4. Update ancestor node metadata timestamps and invalidate hashes
        let file_meta = get_file_metadata(&disk_path);
        let instance_mtime = file_meta.mtime;

        for ancestor_ref in [&series_node, &study_node, &subject_node] {
            {
                let mut ancestor = ancestor_ref.borrow_mut();
                let new_node_mtime = match ancestor.draft_node_metadata {
                    Some(existing) => existing.mtime.max(instance_mtime),
                    None => instance_mtime,
                };
                ancestor.draft_node_metadata = Some(NodeMetadata {
                    atime: file_meta.atime,
                    mtime: new_node_mtime,
                    size: None,
                });

                let new_meta_mtime = match ancestor.draft_meta_metadata {
                    Some(existing) => existing.mtime.max(instance_mtime),
                    None => instance_mtime,
                };
                ancestor.draft_meta_metadata = Some(NodeMetadata {
                    atime: file_meta.atime,
                    mtime: new_meta_mtime,
                    size: None,
                });

                ancestor.valid_self_hash = false;
                ancestor.valid_child_hash = false;
            }
            ancestor_ref.borrow().invalidate_ancestors_hash();
        }

        // 5. Update instance node: do not compute hash; mark valid_self_hash = false
        {
            let mut inst = node.borrow_mut();
            inst.valid_self_hash = false;
            inst.node_metadata = file_meta;
            inst.visited = true;
        }

        Ok(())
    }

    fn write(&self, root: &NodeRef) -> Result<(), String> {
        let get_children_map = |node: &NodeRef| -> Vec<(String, NodeRef)> {
            match &node.borrow().children {
                Children::Attributes(map) | Children::Files(map) => {
                    map.iter().map(|(k, v)| (k.clone(), Rc::clone(v))).collect()
                }
                Children::None => Vec::new(),
            }
        };

        let load_meta_tags = |node: &NodeRef, filename: &str| -> BTreeMap<String, String> {
            let n = node.borrow();
            if let Some(meta_node) = n.meta.get(filename) {
                if let Some(hash) = meta_node.borrow().self_hash {
                    if let Ok((_, content)) = read_object_bytes_in(&self.repo_root, &hash) {
                        if let Ok(json_val) = serde_json::from_slice::<serde_json::Value>(&content) {
                            if let Some(obj) = json_val.as_object() {
                                let mut out = BTreeMap::new();
                                for (k, v) in obj {
                                    if let Some(s) = v.as_str() {
                                        out.insert(k.clone(), s.to_string());
                                    } else {
                                        out.insert(k.clone(), v.to_string());
                                    }
                                }
                                return out;
                            }
                        }
                    }
                }
            }
            BTreeMap::new()
        };

        let tasks = get_children_map(root);
        for (task_id, task_node) in tasks {
            let sources = get_children_map(&task_node);
            for (source_id, source_node) in sources {
                let subjects = get_children_map(&source_node);
                for (subject_id, subject_node) in subjects {
                    let patient_tags = load_meta_tags(&subject_node, &format!("{}.json", subject_id));

                    let studies = get_children_map(&subject_node);
                    for (study_id, study_node) in studies {
                        let study_tags = load_meta_tags(&study_node, &format!("{}.json", study_id));

                        let series_list = get_children_map(&study_node);
                        for (series_id, series_node) in series_list {
                            let series_tags = load_meta_tags(&series_node, &format!("{}.json", series_id));

                            let instances = get_children_map(&series_node);
                            for (instance_id, instance_node) in instances {
                                let blob_hash = instance_node.borrow().self_hash.ok_or_else(|| {
                                    format!("instance node {} missing self_hash", instance_id)
                                })?;

                                let (_, nifti_bytes) =
                                    read_object_bytes_in(&self.repo_root, &blob_hash)?;
                                let px = extract_nifti2_pixel_data(&nifti_bytes)?;

                                let dcm_bytes = reconstruct_dicom_bytes(
                                    &patient_tags,
                                    &study_tags,
                                    &series_tags,
                                    px.rows,
                                    px.cols,
                                    px.bits_allocated,
                                    px.pixel_data,
                                    &instance_id,
                                )?;

                                let view_attrs = ViewAttributes {
                                    subject: &subject_id,
                                    study: &study_id,
                                    series: &series_id,
                                    instance: &instance_id,
                                    source: &source_id,
                                    task: &task_id,
                                };
                                let (rel_path, sidecar_content) = self.view(&view_attrs);
                                let target_file = self.repo_root.join(&rel_path);

                                if let Some(parent) = target_file.parent() {
                                    fs::create_dir_all(parent).map_err(|e| {
                                        format!("failed to create dir {}: {}", parent.display(), e)
                                    })?;
                                }

                                fs::write(&target_file, &dcm_bytes).map_err(|e| {
                                    format!("failed to write {}: {}", target_file.display(), e)
                                })?;

                                // Write .view.<instance_id> sidecar file
                                if let Some(parent) = target_file.parent() {
                                    let sidecar_file = parent.join(format!(".view.{}", instance_id));
                                    fs::write(&sidecar_file, format!("{}\n", sidecar_content.trim_end())).map_err(|e| {
                                        format!("failed to write sidecar {}: {}", sidecar_file.display(), e)
                                    })?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn post_add(&self, root: &NodeRef) -> Result<(), String> {
        let draft_dir = self.repo_root.join(".git-med").join("draft_object");

        let process_ancestor = |node: &NodeRef| -> Result<(), String> {
            let id = node.borrow().id.clone();
            let draft_file = draft_dir.join(format!("{}.json", id));

            if draft_file.exists() {
                let content = fs::read(&draft_file)
                    .map_err(|e| format!("failed to read draft {}: {}", draft_file.display(), e))?;
                let meta_hash = hash_bytes(&content, ObjectType::Meta);

                // Write to .git-med/meta_objects/
                make_object_from_bytes_in(&content, meta_hash, ObjectType::Meta, &self.repo_root)
                    .map_err(|e| format!("failed to save meta object for {}: {}", id, e))?;

                // Create meta node and attach to node.meta
                let meta_file_name = format!("{}.json", id);
                let meta_node = Node::new(&meta_file_name, ChildrenType::None);
                {
                    let mut m = meta_node.borrow_mut();
                    m.self_hash = Some(meta_hash);
                    m.valid_self_hash = true;
                    m.parent = Some(Rc::downgrade(node));
                }

                let mut n = node.borrow_mut();
                n.meta.insert(meta_file_name, meta_node);
                n.meta_hash = Some(meta_hash);
            }

            // Update timestamps from draft metadata
            let mut n = node.borrow_mut();
            if let Some(draft_node) = n.draft_node_metadata.take() {
                n.node_metadata = draft_node;
            }
            if let Some(draft_meta) = n.draft_meta_metadata.take() {
                n.meta_metadata = draft_meta;
            }

            Ok(())
        };

        let get_children_map = |node: &NodeRef| -> Vec<(String, NodeRef)> {
            match &node.borrow().children {
                Children::Attributes(map) | Children::Files(map) => {
                    map.iter().map(|(k, v)| (k.clone(), Rc::clone(v))).collect()
                }
                Children::None => Vec::new(),
            }
        };

        let tasks = get_children_map(root);
        for (_task_id, task_node) in tasks {
            let sources = get_children_map(&task_node);
            for (_source_id, source_node) in sources {
                let subjects = get_children_map(&source_node);
                for (subject_id, subject_node) in subjects {
                    process_ancestor(&subject_node)?;

                    let studies = get_children_map(&subject_node);
                    for (study_id, study_node) in studies {
                        process_ancestor(&study_node)?;

                        let series_list = get_children_map(&study_node);
                        for (series_id, series_node) in series_list {
                            process_ancestor(&series_node)?;

                            let instances = get_children_map(&series_node);
                            for (instance_id, instance_node) in instances {
                                let mut disk_path = self
                                    .repo_root
                                    .join(&subject_id)
                                    .join(&study_id)
                                    .join(&series_id)
                                    .join(&instance_id);

                                if !disk_path.exists() {
                                    disk_path = PathBuf::from(&subject_id)
                                        .join(&study_id)
                                        .join(&series_id)
                                        .join(&instance_id);
                                }

                                if disk_path.exists() {
                                    let should_convert = {
                                        let inst = instance_node.borrow();
                                        !inst.valid_self_hash || inst.self_hash.is_none()
                                    };

                                    if should_convert {
                                        let nifti_bytes = dicom_to_nifti2(&disk_path)?;
                                        let blob_hash = hash_bytes(&nifti_bytes, ObjectType::Blob);

                                        make_object_from_bytes_in(
                                            &nifti_bytes,
                                            blob_hash,
                                            ObjectType::Blob,
                                            &self.repo_root,
                                        )
                                        .map_err(|e| {
                                            format!(
                                                "failed to save NIfTI blob for {}: {}",
                                                instance_id, e
                                            )
                                        })?;

                                        let mut inst = instance_node.borrow_mut();
                                        inst.self_hash = Some(blob_hash);
                                        inst.valid_self_hash = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Clean up draft_object directory
        if draft_dir.exists() {
            let _ = fs::remove_dir_all(&draft_dir);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, remove_dir_all, write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::core::add::add_in;
    use crate::core::tree::load_or_init_tree;
    use crate::tester::generate::sample::dicom;

    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = remove_dir_all(&self.0);
        }
    }

    struct TestContext {
        output_dir: String,
    }

    impl TestContext {
        fn setup() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let output_dir = format!("_tmp_views_dicom_test_{}_{}", std::process::id(), id);

            let n_subjects = 1;
            let study_per_subjects = 1;
            let series_per_study = 1;
            let file_per_series = 1;
            dicom(
                &Path::new(&output_dir),
                n_subjects,
                study_per_subjects,
                series_per_study,
                file_per_series,
            );
            Self { output_dir }
        }
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            let _ = remove_dir_all(&self.output_dir);
        }
    }

    #[test]
    fn test_index() {
        let ctx = TestContext::setup();

        let file_path = Path::new(&ctx.output_dir)
            .join("subject_001")
            .join("study_001")
            .join("series_001")
            .join("instance_001.dcm");

        let mut dicom_view = Dicom::new(Path::new(&ctx.output_dir));
        let dicom = dicom_view.index(&file_path);

        assert_eq!(dicom.subject.as_deref(), Some("subject_001"));
        assert_eq!(dicom.study.as_deref(), Some("study_001"));
        assert_eq!(dicom.series.as_deref(), Some("series_001"));
        assert_eq!(dicom.instance.as_deref(), Some("instance_001.dcm"));
        assert_eq!(dicom.source.as_deref(), Some("clinic"));
        assert_eq!(dicom.task.as_deref(), Some("data"));
    }

    #[test]
    fn test_view() {
        let _ctx = TestContext::setup();

        let dicom = Dicom::default();

        let view_attributes = ViewAttributes {
            subject: "subject_001",
            study: "study_001",
            series: "series_001",
            instance: "instance_001.dcm",
            source: "clinic",
            task: "data",
        };

        let (path, content) = dicom.view(&view_attributes);

        let expected_path = PathBuf::new()
            .join("subject_001")
            .join("study_001")
            .join("series_001")
            .join("instance_001.dcm");

        assert_eq!(path, expected_path);
        assert_eq!(content, "clinic\ndata");
    }

    #[test]
    fn test_dicom_add_in_and_draft_metadata() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let repo_dir = PathBuf::from(format!("_tmp_dicom_add_e2e_{}_{}", std::process::id(), id));
        let _guard = TempDirGuard(repo_dir.clone());

        // Create sample DICOM dataset: 1 subject, 1 study, 1 series, 2 instances
        dicom(&repo_dir, 1, 1, 1, 2);

        // Initialize .git-med structure
        let git_med_dir = repo_dir.join(".git-med");
        create_dir_all(&git_med_dir).unwrap();
        write(git_med_dir.join("VIEW"), "dicom\n").unwrap();

        // Also add an untracked non-DICOM file to verify it's skipped cleanly
        write(repo_dir.join("README.txt"), "This should be ignored").unwrap();

        // Run add_in
        let add_res = add_in(&repo_dir);
        assert!(add_res.is_ok(), "add_in failed: {:?}", add_res.err());

        // 1. Verify draft_object dir has been cleaned up by post_add
        let draft_dir = git_med_dir.join("draft_object");
        assert!(!draft_dir.exists(), "draft_object dir should be removed by post_add");

        // 2. Load index tree and verify metadata and hash flags
        let index_path = git_med_dir.join("index");
        let tree = load_or_init_tree(&index_path).expect("load tree");

        fn get_child(node: &NodeRef, name: &str) -> Option<NodeRef> {
            match &node.borrow().children {
                crate::core::tree::Children::Attributes(map)
                | crate::core::tree::Children::Files(map) => map.get(name).cloned(),
                crate::core::tree::Children::None => None,
            }
        }

        // Navigate tree: data -> clinic -> subject_001 -> study_001 -> series_001 -> instance_001.dcm
        let task_node = get_child(&tree.root, "data").expect("data node");
        let source_node = get_child(&task_node, "clinic").expect("clinic node");
        let subject_node = get_child(&source_node, "subject_001").expect("subject node");
        let study_node = get_child(&subject_node, "study_001").expect("study node");
        let series_node = get_child(&study_node, "series_001").expect("series node");
        let instance_node = get_child(&series_node, "instance_001.dcm").expect("instance_001 node");

        // 3. Ancestor nodes (subject, study, series) have meta nodes in meta_objects
        let sub_borrow = subject_node.borrow();
        assert!(sub_borrow.meta.contains_key("subject_001.json"));
        assert!(sub_borrow.meta_hash.is_some());
        assert!(sub_borrow.draft_node_metadata.is_none());
        assert!(sub_borrow.draft_meta_metadata.is_none());
        let sub_meta_hex = sub_borrow.meta_hash.unwrap().to_hex();
        let sub_meta_obj = git_med_dir.join("meta_objects").join(&sub_meta_hex[..2]).join(&sub_meta_hex[2..]);
        assert!(sub_meta_obj.is_file(), "subject meta object must exist in meta_objects");

        let std_borrow = study_node.borrow();
        assert!(std_borrow.meta.contains_key("study_001.json"));
        assert!(std_borrow.meta_hash.is_some());
        assert!(std_borrow.draft_node_metadata.is_none());
        assert!(std_borrow.draft_meta_metadata.is_none());

        let ser_borrow = series_node.borrow();
        assert!(ser_borrow.meta.contains_key("series_001.json"));
        assert!(ser_borrow.meta_hash.is_some());
        assert!(ser_borrow.draft_node_metadata.is_none());
        assert!(ser_borrow.draft_meta_metadata.is_none());

        // 4. Instance node has been transformed to NIfTI-2 and saved as blob object
        let inst_borrow = instance_node.borrow();
        assert!(inst_borrow.valid_self_hash, "instance must have valid_self_hash = true");
        assert!(inst_borrow.self_hash.is_some(), "instance must have self_hash set");
        let inst_blob_hex = inst_borrow.self_hash.unwrap().to_hex();
        let inst_blob_path = git_med_dir.join("objects").join(&inst_blob_hex[..2]).join(&inst_blob_hex[2..]);
        assert!(inst_blob_path.is_file(), "NIfTI blob object must exist in objects");

        // Verify the blob object content contains the NIfTI-2 header (after git-med object header)
        let blob_raw = fs::read(&inst_blob_path).unwrap();
        // Skip git-med object header: "blob <size>\0"
        let null_pos = blob_raw.iter().position(|&b| b == 0).expect("null separator in object header");
        let payload = &blob_raw[null_pos + 1..];
        let sizeof_hdr = i32::from_le_bytes(payload[0..4].try_into().unwrap());
        assert_eq!(sizeof_hdr, 540, "NIfTI-2 header size must be 540");
        assert_eq!(&payload[4..12], b"n+2\0\r\n\x1a\n", "NIfTI-2 magic mismatch");
    }

    #[test]
    fn test_dicom_write() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let repo_dir = PathBuf::from(format!("_tmp_dicom_write_e2e_{}_{}", std::process::id(), id));
        let _guard = TempDirGuard(repo_dir.clone());

        // 1. Create sample dataset and add into git-med
        dicom(&repo_dir, 1, 1, 1, 2);

        let git_med_dir = repo_dir.join(".git-med");
        create_dir_all(&git_med_dir).unwrap();
        write(git_med_dir.join("VIEW"), "dicom\n").unwrap();

        add_in(&repo_dir).expect("add_in should succeed");

        // 2. Delete the working directory instance files
        let subject_dir = repo_dir.join("subject_001");
        assert!(subject_dir.exists());
        remove_dir_all(&subject_dir).expect("delete working directory");
        assert!(!subject_dir.exists(), "working directory files should be deleted");

        // 3. Load tree from index and run write
        let index_path = git_med_dir.join("index");
        let tree = load_or_init_tree(&index_path).expect("load tree");

        let dicom_view = Dicom::new(&repo_dir);
        let write_res = dicom_view.write(&tree.root);
        assert!(write_res.is_ok(), "write failed: {:?}", write_res.err());

        // 4. Verify reconstructed files and sidecars exist
        let series_dir = repo_dir.join("subject_001").join("study_001").join("series_001");
        let inst_1 = series_dir.join("instance_001.dcm");
        let inst_2 = series_dir.join("instance_002.dcm");
        assert!(inst_1.is_file(), "instance_001.dcm must be reconstructed");
        assert!(inst_2.is_file(), "instance_002.dcm must be reconstructed");

        let sc_1 = series_dir.join(".view.instance_001.dcm");
        let sc_2 = series_dir.join(".view.instance_002.dcm");
        assert!(sc_1.is_file(), ".view.instance_001.dcm sidecar must be reconstructed");
        assert!(sc_2.is_file(), ".view.instance_002.dcm sidecar must be reconstructed");
        assert_eq!(read_to_string(&sc_1).unwrap().trim(), "clinic\ndata");

        // 5. Verify reconstructed DICOM tags and pixel data
        let parsed = parse_dicom_header(&inst_1).expect("parse reconstructed instance_001");
        assert_eq!(parsed.patient.get("PatientID").map(String::as_str), Some("J3 DE P5 C5"));
        assert_eq!(parsed.study.get("StudyInstanceUID").map(String::as_str), Some("1.2.826.0.1.4978912.2142.20180719150610"));
        assert_eq!(parsed.series.get("Modality").map(String::as_str), Some("MR"));

        let px = crate::files::dicom::extract_dicom_pixel_data(&inst_1).expect("extract pixel data");
        assert_eq!(px.rows, 256);
        assert_eq!(px.cols, 256);
        assert_eq!(px.bits_allocated, 16);
        assert_eq!(px.data.len(), 256 * 256 * 2);
    }
}

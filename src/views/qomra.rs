use crate::core::tree::NodeRef;
use crate::files::types::ObjectType;
use crate::files::utils::{hash, make_object_in};
use crate::views::get_file_metadata;
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use crate::views::utils::bytes2buf128;
use std::os::unix::ffi::OsStrExt;
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
}

/// Return `true` if `path`'s filename matches `meta.*.json` (including `meta.json`).
pub fn is_metafile(path: &Path) -> bool {
    path.file_name()
        .and_then(|f| f.to_str())
        .map_or(false, |s| {
            let parts: Vec<&str> = s.split('.').collect();
            parts.len() >= 2 && parts[0] == "meta" && parts[parts.len() - 1] == "json"
        })
}

fn normalized_depth(path: &Path) -> usize {
    path.components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

fn add_node_object_in(
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    obj_type: ObjectType,
) -> Result<(), ()> {
    let disk_path = repo_root.join(rel_path);
    let digest = hash(&disk_path, None, obj_type);
    let mut node_mut = node.borrow_mut();

    if node_mut.self_hash != Some(digest) {
        make_object_in(&disk_path, digest, obj_type, repo_root).map_err(|_| ())?;
        node_mut.self_hash = Some(digest);
        if obj_type == ObjectType::Meta {
            if let Some(parent_ref) = node_mut.parent() {
                parent_ref.borrow_mut().valid_self_hash = false;
                parent_ref.borrow().invalidate_ancestors_hash();
            }
        } else {
            node_mut.invalidate_ancestors_hash();
        }
    }

    node_mut.node_metadata = get_file_metadata(&disk_path);
    node_mut.visited = true;
    Ok(())
}

pub fn git_add_file(path: &Path, node: &NodeRef) -> Result<(), ()> {
    git_add_file_in(Path::new("."), path, node)
}

pub fn git_add_metafile(path: &Path, node: &NodeRef) -> Result<(), ()> {
    git_add_metafile_in(Path::new("."), path, node)
}

pub fn git_add_file_in(repo_root: &Path, rel_path: &Path, node: &NodeRef) -> Result<(), ()> {
    add_node_object_in(repo_root, rel_path, node, ObjectType::Blob)
}

pub fn git_add_metafile_in(repo_root: &Path, rel_path: &Path, node: &NodeRef) -> Result<(), ()> {
    add_node_object_in(repo_root, rel_path, node, ObjectType::Meta)
}

impl View for Qomra {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex {
        let comps: Vec<[u8; 16]> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(bytes2buf128(s.as_bytes())),
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
            attrs.task = Some(comps[0]);
        }
        if comps.len() >= 3 {
            attrs.source = Some(comps[1]);
        }
        if comps.len() >= 4 {
            attrs.subject = Some(comps[2]);
        }
        if comps.len() >= 5 {
            attrs.study = Some(comps[3]);
        }
        if comps.len() >= 6 {
            attrs.series = Some(comps[4]);
        }
        if comps.len() > 6 || (comps.len() == 6 && !is_metafile(path)) {
            attrs.instance = Some(comps[5]);
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

    fn index_source(&self, path: &Path, node: &NodeRef, _task_id: &[u8; 16]) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 3)
    }

    fn index_subject(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &[u8; 16],
        _source_id: &[u8; 16],
    ) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 4)
    }

    fn index_study(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &[u8; 16],
        _source_id: &[u8; 16],
        _subject_id: &[u8; 16],
    ) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 5)
    }

    fn index_series(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &[u8; 16],
        _source_id: &[u8; 16],
        _subject_id: &[u8; 16],
        _study_id: &[u8; 16],
    ) -> Result<(), ()> {
        self.add_meta_with_max_depth(path, node, 6)
    }

    fn index_instance(
        &self,
        path: &Path,
        node: &NodeRef,
        _task_id: &[u8; 16],
        _source_id: &[u8; 16],
        _subject_id: &[u8; 16],
        _study_id: &[u8; 16],
        _series_id: &[u8; 16],
    ) -> Result<(), ()> {
        if normalized_depth(path) > 7 {
            Err(())
        } else if is_metafile(path) {
            git_add_metafile_in(&self.repo_root, path, node)
        } else {
            git_add_file_in(&self.repo_root, path, node)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::utils::string2buf128;
    use std::fs::remove_dir_all;
    use std::path::{Path, PathBuf};

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

        assert_eq!(indexed.task, Some(string2buf128("data")));
        assert_eq!(indexed.source, Some(string2buf128("clinic")));
        assert_eq!(indexed.subject, Some(string2buf128("subject_001")));
        assert_eq!(indexed.study, Some(string2buf128("study_001")));
        assert_eq!(indexed.series, Some(string2buf128("series_001")));
        assert_eq!(indexed.instance, Some(string2buf128("instance_001.dcm")));
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
        assert_eq!(task_meta.task, Some(string2buf128("data")));
        assert!(task_meta.source.is_none());

        let series_meta = qomra.index(Path::new(
            "data/clinic/subject_001/study_001/series_001/meta.json",
        ));
        assert_eq!(series_meta.task, Some(string2buf128("data")));
        assert_eq!(series_meta.series, Some(string2buf128("series_001")));
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
}

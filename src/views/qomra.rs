use crate::core::tree::NodeRef;
use crate::files::types::ObjectType;
use crate::files::utils::{hash, make_object};
use crate::views::get_file_metadata;
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use crate::views::utils::get_name;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct Qomra {}

fn add_node_object(path: &Path, node: &NodeRef, obj_type: ObjectType) -> Result<(), ()> {
    let digest = hash(path, None, obj_type);
    let mut node = node.borrow_mut();
    let old_hash = match obj_type {
        ObjectType::Blob => node.self_hash.ok_or(())?,
        ObjectType::Meta => node.meta_hash.ok_or(())?,
    };

    if digest != old_hash {
        make_object(path, digest, obj_type).map_err(|_| ())?;
        match obj_type {
            ObjectType::Blob => node.self_hash = Some(digest),
            ObjectType::Meta => {
                node.meta_hash = Some(digest);
                node.valid_self_hash = false;
            }
        }
        node.invalidate_ancestors_hash();
    }

    let metadata = get_file_metadata(path);
    match obj_type {
        ObjectType::Blob => node.node_metadata = metadata,
        ObjectType::Meta => node.meta_metadata = metadata,
    }
    node.visited = true;
    Ok(())
}

pub fn git_add_file(path: &Path, node: &NodeRef) -> Result<(), ()> {
    add_node_object(path, node, ObjectType::Blob)
}

pub fn git_add_metafile(path: &Path, node: &NodeRef) -> Result<(), ()> {
    add_node_object(path, node, ObjectType::Meta)
}

impl View for Qomra {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex {
        let mut ancestors = path.ancestors();

        ViewAttributesIndex {
            instance: get_name(&ancestors.next()),
            series: get_name(&ancestors.next()),
            study: get_name(&ancestors.next()),
            subject: get_name(&ancestors.next()),
            source: get_name(&ancestors.next()),
            task: get_name(&ancestors.next()),
        }
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
        let n_ancestors = path.ancestors().count();

        if n_ancestors > 2 {
            Err(())
        } else {
            git_add_metafile(path, node)
        }
    }

    fn index_task(&self, path: &Path, node: &NodeRef) -> Result<(), ()> {
        let n_ancestors = path.ancestors().count();

        if n_ancestors > 3 {
            Err(())
        } else {
            git_add_metafile(path, node)
        }
    }

    fn index_source(&self, path: &Path, node: &NodeRef, task_id: &[u8; 16]) -> Result<(), ()> {
        let n_ancestors = path.ancestors().count();

        if n_ancestors > 4 {
            Err(())
        } else {
            git_add_metafile(path, node)
        }
    }

    fn index_subject(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
    ) -> Result<(), ()> {
        {
            let n_ancestors = path.ancestors().count();

            if n_ancestors > 5 {
                Err(())
            } else {
                git_add_metafile(path, node)
            }
        }
    }

    fn index_study(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
    ) -> Result<(), ()> {
        let n_ancestors = path.ancestors().count();

        if n_ancestors > 6 {
            Err(())
        } else {
            git_add_metafile(path, node)
        }
    }

    fn index_series(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
        study_id: &[u8; 16],
    ) -> Result<(), ()> {
        let n_ancestors = path.ancestors().count();

        if n_ancestors > 7 {
            Err(())
        } else {
            git_add_metafile(path, node)
        }
    }

    fn index_instance(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
        study_id: &[u8; 16],
        series_id: &[u8; 16],
    ) -> Result<(), ()> {
        let n_ancestors = path.ancestors().count();
        // if meta.*.json then add as meta
        let filename = path.file_name().ok_or(())?;
        let is_metafile = filename.to_str().map_or(false, |s| {
            let parts: Vec<&str> = s.split('.').collect();
            parts[0] == "meta" && parts[parts.len() - 1] == "json"
        });

        if n_ancestors > 8 {
            Err(())
        } else {
            if is_metafile {
                git_add_metafile(path, node)
            } else {
                git_add_file(path, node)
            }
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

        let mut qomra = Qomra::default();
        let qomra = qomra.index(&file_path);

        assert_eq!(qomra.task, Some(string2buf128("data")));
        assert_eq!(qomra.source, Some(string2buf128("clinic")));
        assert_eq!(qomra.subject, Some(string2buf128("subject_001")));
        assert_eq!(qomra.study, Some(string2buf128("study_001")));
        assert_eq!(qomra.series, Some(string2buf128("series_001")));
        assert_eq!(qomra.instance, Some(string2buf128("instance_001.dcm")));
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

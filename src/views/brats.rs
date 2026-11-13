use crate::core::tree::NodeRef;
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use crate::views::utils::{get_name, string2buf128};
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct Brats {}

impl View for Brats {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex {
        let mut ancestors = path.ancestors();

        let instance_path = ancestors.next();
        let instance_str = instance_path
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let instance = get_name(&instance_path);

        let series = Some(string2buf128(&format!("series_{}", instance_str)));

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
            let study = Some(string2buf128(&format!("study_{}", next_str)));
            (subject, study)
        };

        // read the sidecar .view.<filename> (source\ntask) in txt if present
        let sidecar_path = path
            .parent()
            .map(|p| p.join(format!(".view.{}", instance_str)))
            .filter(|p| p.exists());

        let file_to_read = sidecar_path.as_deref().unwrap_or(path);

        let others: Vec<String> = read_to_string(file_to_read)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();

        let source = Some(string2buf128(
            others.get(0).map(String::as_str).unwrap_or_default(),
        ));
        let task = Some(if instance_str.contains("_seg") {
            string2buf128("segmentation")
        } else {
            string2buf128(others.get(1).map(String::as_str).unwrap_or_default())
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
        if !view_attributes.study.is_empty() {
            path = path.join(view_attributes.study);
        }
        path = path.join(view_attributes.instance);

        (
            path,
            format!("{0}\n{1}", view_attributes.source, view_attributes.task),
        )
    }

    fn index_root(&self, path: &Path, node: &NodeRef) -> Result<(), ()> {
        Err(())
    }

    fn index_task(&self, path: &Path, node: &NodeRef) -> Result<(), ()> {
        Err(())
    }

    fn index_source(&self, path: &Path, node: &NodeRef, task_id: &[u8; 16]) -> Result<(), ()> {
        Err(())
    }

    fn index_subject(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
    ) -> Result<(), ()> {
        Err(())
    }

    fn index_study(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
    ) -> Result<(), ()> {
        Err(())
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
        Err(())
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
        Err(())
    }

    fn write(&self, _root: &NodeRef) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::remove_dir_all;
    use std::path::{Path, PathBuf};

    use crate::tester::generate::sample::brats;
    struct TestContext {
        output_dir: String,
    }

    impl TestContext {
        // PRE-TEST PROCESS (Setup)
        fn setup() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let output_dir = format!("_tmp_views_brats_test_{}_{}", std::process::id(), id);

            let n_subjects = 1;
            let study_per_subjects = 1;
            let file_per_study = 5;
            brats(
                &Path::new(&output_dir),
                n_subjects,
                study_per_subjects,
                file_per_study,
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
            .join("subject_001")
            .join("study_001")
            .join("subject_001_study_001_t1.nii.gz");

        let mut brats = Brats::default();
        let brats = brats.index(&file_path);

        assert_eq!(brats.subject, Some(string2buf128("subject_001")));
        assert_eq!(brats.study, Some(string2buf128("study_001")));
        assert_eq!(
            brats.series,
            Some(string2buf128("series_subject_001_study_001_t1.nii.gz"))
        );
        assert_eq!(
            brats.instance,
            Some(string2buf128("subject_001_study_001_t1.nii.gz"))
        );
        assert_eq!(brats.source, Some(string2buf128("clinic")));
        assert_eq!(brats.task, Some(string2buf128("data")));
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

        assert_eq!(brats.subject, Some(string2buf128("subject_001")));
        assert_eq!(brats.study, Some(string2buf128("study_001")));
        assert_eq!(
            brats.series,
            Some(string2buf128("series_subject_001_study_001_seg.nii.gz"))
        );
        assert_eq!(
            brats.instance,
            Some(string2buf128("subject_001_study_001_seg.nii.gz"))
        );
        assert_eq!(brats.source, Some(string2buf128("consensus")));
        assert_eq!(brats.task, Some(string2buf128("segmentation")));
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
}

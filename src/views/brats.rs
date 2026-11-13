use crate::core::tree::NodeRef;
use crate::views::traits::View;
use crate::views::types::{ViewAttributes, ViewAttributesIndex};
use crate::views::utils::get_name;
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

        let source = Some(
            others.get(0).map(String::as_str).unwrap_or_default().to_string(),
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
        if !view_attributes.study.is_empty() {
            path = path.join(view_attributes.study);
        }
        path = path.join(view_attributes.instance);

        (
            path,
            format!("{0}\n{1}", view_attributes.source, view_attributes.task),
        )
    }

    fn index_root(&self, _path: &Path, _node: &NodeRef) -> Result<(), ()> {
        Err(())
    }

    fn index_task(&self, _path: &Path, _node: &NodeRef) -> Result<(), ()> {
        Err(())
    }

    fn index_source(&self, _path: &Path, _node: &NodeRef, _task_id: &str) -> Result<(), ()> {
        Err(())
    }

    fn index_subject(
        &self,
        _path: &Path,
        _node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
    ) -> Result<(), ()> {
        Err(())
    }

    fn index_study(
        &self,
        _path: &Path,
        _node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
    ) -> Result<(), ()> {
        Err(())
    }

    fn index_series(
        &self,
        _path: &Path,
        _node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
        _study_id: &str,
    ) -> Result<(), ()> {
        Err(())
    }

    fn index_instance(
        &self,
        _path: &Path,
        _node: &NodeRef,
        _task_id: &str,
        _source_id: &str,
        _subject_id: &str,
        _study_id: &str,
        _series_id: &str,
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
}

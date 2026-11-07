use crate::views::traits::View;
use crate::views::types::ViewAttributes;
use crate::views::utils::get_name;
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct Brats {
    pub subject: String,
    pub study: String,
    pub series: String,
    pub instance: String,

    pub source: String,
    pub task: String,
}

impl View for Brats {
    fn index(&mut self, path: &Path) {
        let mut ancestors = path.ancestors();

        self.instance = get_name(&ancestors.next());
        self.series = format!("series_{}", self.instance);

        let next_name = get_name(&ancestors.next());
        if next_name.starts_with("study") {
            self.study = next_name;
            self.subject = get_name(&ancestors.next());
        } else {
            self.subject = next_name;
            self.study = format!("study_{}", self.subject);
        }

        // read the sidecar .view.<filename> (source\ntask) in txt if present
        let sidecar_path = path
            .parent()
            .map(|p| p.join(format!(".view.{}", self.instance)))
            .filter(|p| p.exists());

        let file_to_read = sidecar_path.as_deref().unwrap_or(path);

        let others: Vec<String> = read_to_string(file_to_read)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect();

        self.source = others.get(0).cloned().unwrap_or_default();
        self.task = if self.instance.contains("_seg") {
            "segmentation".to_string()
        } else {
            others.get(1).cloned().unwrap_or_default()
        };
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
        brats.index(&file_path);

        assert_eq!(brats.subject, "subject_001");
        assert_eq!(brats.study, "study_001");
        assert_eq!(brats.series, "series_subject_001_study_001_t1.nii.gz");
        assert_eq!(brats.instance, "subject_001_study_001_t1.nii.gz");
        assert_eq!(brats.source, "clinic");
        assert_eq!(brats.task, "data");
    }

    #[test]
    fn test_index_seg() {
        let ctx = TestContext::setup();

        let file_path = Path::new(&ctx.output_dir)
            .join("subject_001")
            .join("study_001")
            .join("subject_001_study_001_seg.nii.gz");

        let mut brats = Brats::default();
        brats.index(&file_path);

        assert_eq!(brats.subject, "subject_001");
        assert_eq!(brats.study, "study_001");
        assert_eq!(brats.series, "series_subject_001_study_001_seg.nii.gz");
        assert_eq!(brats.instance, "subject_001_study_001_seg.nii.gz");
        assert_eq!(brats.source, "consensus");
        assert_eq!(brats.task, "segmentation");
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

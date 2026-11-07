use crate::views::traits::View;
use crate::views::types::ViewAttributes;
use crate::views::utils::get_name;
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default, Debug, PartialEq, Eq, Clone)]
pub struct Dicom {
    pub subject: String,
    pub study: String,
    pub series: String,
    pub instance: String,

    pub source: String,
    pub task: String,
}

impl View for Dicom {
    fn index(&mut self, path: &Path) {
        let mut ancestors = path.ancestors();

        self.instance = get_name(&ancestors.next());
        self.series = get_name(&ancestors.next());
        self.study = get_name(&ancestors.next());
        self.subject = get_name(&ancestors.next());

        let sidecar_path = path
            .parent()
            .map(|p| p.join(format!(".view.{}", self.instance)))
            .filter(|p| p.exists());

        let file_to_read = sidecar_path.as_deref().unwrap_or(path);

        let others: Vec<String> = read_to_string(file_to_read)
            .unwrap() // panic on possible file-reading errors
            .lines() // split the string into an iterator of string slices
            .map(String::from) // make each slice into a string
            .collect(); // gather them together into a vector

        self.source = others.get(0).cloned().unwrap_or_default();
        self.task = others.get(1).cloned().unwrap_or_default();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::remove_dir_all;
    use std::path::{Path, PathBuf};

    use crate::tester::generate::sample::dicom;
    struct TestContext {
        output_dir: String,
    }

    impl TestContext {
        // PRE-TEST PROCESS (Setup)
        fn setup() -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
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
            .join("series_001")
            .join("instance_001.dcm");

        let mut dicom = Dicom::default();
        dicom.index(&file_path);

        assert_eq!(dicom.subject, "subject_001");
        assert_eq!(dicom.study, "study_001");
        assert_eq!(dicom.series, "series_001");
        assert_eq!(dicom.instance, "instance_001.dcm");
        assert_eq!(dicom.source, "clinic");
        assert_eq!(dicom.task, "data");
    }

    #[test]
    fn test_view() {
        let ctx = TestContext::setup();

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
}

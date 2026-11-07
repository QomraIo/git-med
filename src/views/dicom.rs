use super::traits::View;
use super::types::ViewAttributes;
use super::utils::get_name;
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;

pub struct Dicom<'a> {
    subject: &'a str,
    study: &'a str,
    series: &'a str,
    instance: &'a str,

    source: String,
    task: String,
}

impl<'a> View for Dicom<'a> {
    fn index(&mut self, path: &Path) {
        let ancestors = path.ancestors();

        self.instance = get_name(ancestors.next());
        self.series = get_name(ancestors.next());
        self.study = get_name(ancestors.next());
        self.subject = get_name(ancestors.next());

        let others = read_to_string(path)
            .unwrap() // panic on possible file-reading errors
            .lines() // split the string into an iterator of string slices
            .map(String::from) // make each slice into a string
            .collect(); // gather them together into a vector

        self.source = others[0];
        self.task = others[1];
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

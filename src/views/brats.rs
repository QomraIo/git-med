use crate::views::traits::View;
use crate::views::types::ViewAttributes;
use crate::views::utils::get_name;
use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;

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

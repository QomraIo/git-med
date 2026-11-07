use crate::views::traits::View;
use crate::views::types::ViewAttributes;
use crate::views::utils::get_name;
use std::path::Path;
use std::path::PathBuf;

pub struct Qomra {
    pub subject: String,
    pub study: String,
    pub series: String,
    pub instance: String,

    pub source: String,
    pub task: String,
}

impl View for Qomra {
    fn index(&mut self, path: &Path) {
        let mut ancestors = path.ancestors();

        self.instance = get_name(&ancestors.next());
        self.series = get_name(&ancestors.next());
        self.study = get_name(&ancestors.next());
        self.subject = get_name(&ancestors.next());
        self.source = get_name(&ancestors.next());
        self.task = get_name(&ancestors.next());
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
}

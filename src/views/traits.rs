use super::types::ViewAttributes;
use std::path::Path;
use std::path::PathBuf;

pub trait View {
    fn index(&mut self, path: &Path);
    fn view(&self, view_attributes: &ViewAttributes) -> (PathBuf, String);
}

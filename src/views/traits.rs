use super::types::{ViewAttributes, ViewAttributesIndex};
use std::path::Path;
use std::path::PathBuf;

pub trait View {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex;
    fn view(&self, view_attributes: &ViewAttributes) -> (PathBuf, String);
}

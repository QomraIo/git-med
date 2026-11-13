use crate::core::tree::NodeRef;

use super::types::{ViewAttributes, ViewAttributesIndex};
use std::path::Path;
use std::path::PathBuf;

pub trait View {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex;
    fn view(&self, view_attributes: &ViewAttributes) -> (PathBuf, String);

    fn index_root(&self, path: &Path, node: &NodeRef) -> Result<(), ()>;

    fn index_task(&self, path: &Path, node: &NodeRef) -> Result<(), ()>;

    fn index_source(&self, path: &Path, node: &NodeRef, task_id: &str) -> Result<(), ()>;

    fn index_subject(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &str,
        source_id: &str,
    ) -> Result<(), ()>;

    fn index_study(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &str,
        source_id: &str,
        subject_id: &str,
    ) -> Result<(), ()>;

    fn index_series(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &str,
        source_id: &str,
        subject_id: &str,
        study_id: &str,
    ) -> Result<(), ()>;

    fn index_instance(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &str,
        source_id: &str,
        subject_id: &str,
        study_id: &str,
        series_id: &str,
    ) -> Result<(), ()>;

    fn write(&self, root: &NodeRef) -> Result<(), String>;
}

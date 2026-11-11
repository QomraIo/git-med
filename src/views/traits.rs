use crate::core::tree::NodeRef;

use super::types::{ViewAttributes, ViewAttributesIndex};
use std::path::Path;
use std::path::PathBuf;

pub trait View {
    fn index(&mut self, path: &Path) -> ViewAttributesIndex;
    fn view(&self, view_attributes: &ViewAttributes) -> (PathBuf, String);

    fn index_root(&self, path: &Path, node: &NodeRef) -> Result<(), ()>;

    fn index_task(&self, path: &Path, node: &NodeRef) -> Result<(), ()>;

    fn index_source(&self, path: &Path, node: &NodeRef, task_id: &[u8; 16]) -> Result<(), ()>;

    fn index_subject(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
    ) -> Result<(), ()>;

    fn index_study(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
    ) -> Result<(), ()>;

    fn index_series(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
        study_id: &[u8; 16],
    ) -> Result<(), ()>;

    fn index_instance(
        &self,
        path: &Path,
        node: &NodeRef,
        task_id: &[u8; 16],
        source_id: &[u8; 16],
        subject_id: &[u8; 16],
        study_id: &[u8; 16],
        series_id: &[u8; 16],
    ) -> Result<(), ()>;
}

pub mod brats;
pub mod dicom;
pub mod qomra;
pub mod traits;
pub mod types;
pub mod utils;
use std::path::Path;

use crate::core::tree::NodeRef;
use crate::views::brats::Brats;
use crate::views::dicom::Dicom;
use crate::views::traits::View;
use crate::views::types::FileMetadata;

pub fn get_view(view: &str) -> Option<Box<dyn View>> {
    match view {
        "brats" => Some(Box::new(Brats::default())),
        "dicom" => Some(Box::new(Dicom::default())),
        _ => None,
    }
}

pub fn should_hash(node: &NodeRef, filemetadata: &FileMetadata) -> bool {
    if node.borrow().meta_metadata <= filemetadata || node.borrow().node_metadata <= filemetadata {
        true
    }
    false
}

pub fn index_root(view: &str, path: &Path, node: &NodeRef) -> Option<Box<dyn View>> {
    get_view(view)
}

pub fn index_task(view: &str, path: &Path, node: &NodeRef) -> Option<Box<dyn View>> {
    get_view(view)
}

pub fn index_source(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
) -> Option<Box<dyn View>> {
    get_view(view)
}

pub fn index_subject(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
) -> Option<Box<dyn View>> {
    get_view(view)
}

pub fn index_study(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
    subject_id: &[u8; 16],
) -> Option<Box<dyn View>> {
    get_view(view)
}

pub fn index_series(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
    subject_id: &[u8; 16],
    study_id: &[u8; 16],
) -> Option<Box<dyn View>> {
    get_view(view)
}

pub fn index_instance(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
    subject_id: &[u8; 16],
    study_id: &[u8; 16],
) -> Option<Box<dyn View>> {
    get_view(view)
}

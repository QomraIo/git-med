pub mod brats;
pub mod dicom;
pub mod qomra;
pub mod traits;
pub mod types;
pub mod utils;
use std::path::Path;

use crate::core::tree::NodeRef;
use crate::files::types::FileMetadata;
use crate::files::utils::get_file_metadata;
use crate::views::brats::Brats;
use crate::views::dicom::Dicom;
use crate::views::qomra::Qomra;
use crate::views::traits::View;

pub fn get_view(view: &str) -> Option<Box<dyn View>> {
    match view {
        "brats" => Some(Box::new(Brats::default())),
        "dicom" => Some(Box::new(Dicom::default())),
        "qomra" => Some(Box::new(Qomra::default())),
        _ => None,
    }
}

pub fn should_hash(node: &NodeRef, filemetadata: &FileMetadata) -> bool {
    let node = node.borrow();
    if node.meta_metadata <= *filemetadata || node.node_metadata <= *filemetadata {
        return true;
    }

    if node.node_metadata.size.is_some() {
        return node.node_metadata.size != filemetadata.size;
    }
    if node.meta_metadata.size.is_some() {
        return node.meta_metadata.size != filemetadata.size;
    }

    false
}

pub fn index_root(view: &str, path: &Path, node: &NodeRef) -> Result<(), ()> {
    let file_metadata = get_file_metadata(path);
    match should_hash(node, &file_metadata) {
        true => {
            let v = get_view(view).ok_or(())?;
            v.index_root(path, node)
        }
        false => Ok(()),
    }
}

pub fn index_task(view: &str, path: &Path, node: &NodeRef) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    v.index_task(path, node)
}

pub fn index_source(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    v.index_source(path, node, task_id)
}

pub fn index_subject(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    v.index_subject(path, node, task_id, source_id)
}

pub fn index_study(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
    subject_id: &[u8; 16],
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    v.index_study(path, node, task_id, source_id, subject_id)
}

pub fn index_series(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
    subject_id: &[u8; 16],
    study_id: &[u8; 16],
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    v.index_series(path, node, task_id, source_id, subject_id, study_id)
}

pub fn index_instance(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &[u8; 16],
    source_id: &[u8; 16],
    subject_id: &[u8; 16],
    study_id: &[u8; 16],
    series_id: &[u8; 16],
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    v.index_instance(
        path, node, task_id, source_id, subject_id, study_id, series_id,
    )
}

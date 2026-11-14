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
    get_view_in(view, Path::new("."))
}

/// Resolve a [`View`] implementation configured to operate under `repo_root`.
pub fn get_view_in(view: &str, repo_root: &Path) -> Option<Box<dyn View>> {
    match view {
        "brats" => Some(Box::new(Brats::new(repo_root))),
        "dicom" => Some(Box::new(Dicom::new(repo_root))),
        "qomra" => Some(Box::new(Qomra::new(repo_root))),
        _ => None,
    }
}

/// Return `true` when `node` has no stored hash or its cached file metadata differs from `filemetadata`.
pub fn should_hash(node: &NodeRef, filemetadata: &FileMetadata) -> bool {
    let node = node.borrow();
    if node.self_hash.is_none() {
        return true;
    }

    (
        node.node_metadata.atime,
        node.node_metadata.mtime,
        node.node_metadata.size,
    ) != (filemetadata.atime, filemetadata.mtime, filemetadata.size)
}

fn index_if_needed<F>(
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    index_fn: F,
) -> Result<(), ()>
where
    F: FnOnce() -> Result<(), ()>,
{
    let disk_path = repo_root.join(rel_path);
    let file_metadata = get_file_metadata(&disk_path);
    if should_hash(node, &file_metadata) {
        index_fn()?;
    }
    node.borrow_mut().visited = true;
    Ok(())
}

pub fn index_root(view: &str, path: &Path, node: &NodeRef) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_root_in(v.as_ref(), Path::new("."), path, node)
}

pub fn index_root_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || view.index_root(rel_path, node))
}

pub fn index_task(view: &str, path: &Path, node: &NodeRef) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_task_in(v.as_ref(), Path::new("."), path, node)
}

pub fn index_task_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || view.index_task(rel_path, node))
}

pub fn index_source(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &str,
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_source_in(v.as_ref(), Path::new("."), path, node, task_id)
}

pub fn index_source_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    task_id: &str,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || {
        view.index_source(rel_path, node, task_id)
    })
}

pub fn index_subject(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_subject_in(v.as_ref(), Path::new("."), path, node, task_id, source_id)
}

pub fn index_subject_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || {
        view.index_subject(rel_path, node, task_id, source_id)
    })
}

pub fn index_study(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
    subject_id: &str,
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_study_in(
        v.as_ref(),
        Path::new("."),
        path,
        node,
        task_id,
        source_id,
        subject_id,
    )
}

pub fn index_study_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
    subject_id: &str,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || {
        view.index_study(rel_path, node, task_id, source_id, subject_id)
    })
}

pub fn index_series(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
    subject_id: &str,
    study_id: &str,
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_series_in(
        v.as_ref(),
        Path::new("."),
        path,
        node,
        task_id,
        source_id,
        subject_id,
        study_id,
    )
}

pub fn index_series_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
    subject_id: &str,
    study_id: &str,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || {
        view.index_series(rel_path, node, task_id, source_id, subject_id, study_id)
    })
}

pub fn index_instance(
    view: &str,
    path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
    subject_id: &str,
    study_id: &str,
    series_id: &str,
) -> Result<(), ()> {
    let v = get_view(view).ok_or(())?;
    index_instance_in(
        v.as_ref(),
        Path::new("."),
        path,
        node,
        task_id,
        source_id,
        subject_id,
        study_id,
        series_id,
    )
}

pub fn index_instance_in(
    view: &dyn View,
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    task_id: &str,
    source_id: &str,
    subject_id: &str,
    study_id: &str,
    series_id: &str,
) -> Result<(), ()> {
    index_if_needed(repo_root, rel_path, node, || {
        view.index_instance(
            rel_path, node, task_id, source_id, subject_id, study_id, series_id,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tree::{ChildrenType, Node};
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn test_should_hash_detects_missing_hash_and_metadata_changes() {
        let node = Node::new("1", ChildrenType::None);
        let t1 = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
        let t2 = UNIX_EPOCH + Duration::new(1_700_000_001, 0);
        let meta = FileMetadata {
            atime: t1,
            mtime: t1,
            size: Some(100),
        };

        // New node with self_hash == None must be hashed.
        assert!(should_hash(&node, &meta));

        {
            let mut n = node.borrow_mut();
            n.self_hash = Some(blake3::hash(b"data"));
            n.node_metadata = meta;
        }
        // Unchanged metadata should skip re-hashing.
        assert!(!should_hash(&node, &meta));

        // Changed mtime, atime, or size must trigger re-hashing.
        assert!(should_hash(
            &node,
            &FileMetadata {
                mtime: t2,
                ..meta
            }
        ));
        assert!(should_hash(
            &node,
            &FileMetadata {
                atime: t2,
                ..meta
            }
        ));
        assert!(should_hash(
            &node,
            &FileMetadata {
                size: Some(101),
                ..meta
            }
        ));
    }

    #[test]
    fn test_index_if_needed_marks_visited_when_unchanged() {
        let temp_dir = std::env::temp_dir().join(format!("git_med_views_test_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let file_path = temp_dir.join("sample.bin");
        std::fs::write(&file_path, b"hello").unwrap();

        let meta = get_file_metadata(&file_path);
        let node = Node::new("2", ChildrenType::None);
        {
            let mut n = node.borrow_mut();
            n.self_hash = Some(blake3::hash(b"hello"));
            n.node_metadata = meta;
            n.visited = false;
        }

        let view = Qomra::new(&temp_dir);
        index_root_in(&view, &temp_dir, Path::new("sample.bin"), &node).unwrap();
        assert!(node.borrow().visited);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}

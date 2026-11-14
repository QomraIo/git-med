use std::path::Path;

use crate::core::tree::NodeRef;
use crate::files::types::ObjectType;
use crate::files::utils::{hash, make_object_in};
use crate::views::get_file_metadata;

pub fn add_node_object_in(
    repo_root: &Path,
    rel_path: &Path,
    node: &NodeRef,
    obj_type: ObjectType,
) -> Result<(), ()> {
    let disk_path = repo_root.join(rel_path);
    let digest = hash(&disk_path, None, obj_type);
    let mut node_mut = node.borrow_mut();

    if node_mut.self_hash != Some(digest) {
        make_object_in(&disk_path, digest, obj_type, repo_root).map_err(|_| ())?;
        node_mut.self_hash = Some(digest);
        if obj_type == ObjectType::Meta {
            if let Some(parent_ref) = node_mut.parent() {
                parent_ref.borrow_mut().valid_self_hash = false;
                parent_ref.borrow().invalidate_ancestors_hash();
            }
        } else {
            node_mut.invalidate_ancestors_hash();
        }
    }

    node_mut.node_metadata = get_file_metadata(&disk_path);
    node_mut.visited = true;
    Ok(())
}

pub fn git_add_file(path: &Path, node: &NodeRef) -> Result<(), ()> {
    git_add_file_in(Path::new("."), path, node)
}

pub fn git_add_metafile(path: &Path, node: &NodeRef) -> Result<(), ()> {
    git_add_metafile_in(Path::new("."), path, node)
}

pub fn git_add_file_in(repo_root: &Path, rel_path: &Path, node: &NodeRef) -> Result<(), ()> {
    add_node_object_in(repo_root, rel_path, node, ObjectType::Blob)
}

pub fn git_add_metafile_in(repo_root: &Path, rel_path: &Path, node: &NodeRef) -> Result<(), ()> {
    add_node_object_in(repo_root, rel_path, node, ObjectType::Meta)
}

pub fn get_name(opt_path: &Option<&Path>) -> Option<String> {
    opt_path
        .and_then(|p| p.file_name())
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

#[allow(dead_code)]
pub fn string2buf128(text: &str) -> [u8; 16] {
    let bytes = text.as_bytes();
    bytes2buf128(bytes)
}

#[allow(dead_code)]
pub fn bytes2buf128(bytes: &[u8]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let len = bytes.len().min(16);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

#[allow(dead_code)]
pub fn buf128_to_string(buf: &[u8; 16]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(16);
    String::from_utf8_lossy(&buf[..len]).to_string()
}

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::tester::cli_runner::GitMedCli;
use crate::tester::compare::{assert_tree_hashes_eq, assert_tree_shape_eq, compare_tree_shapes};
use crate::tester::generate::sample::{BRATS_SAMPLE, DICOM_SAMPLE};

static NEXT_E2E_ID: AtomicUsize = AtomicUsize::new(0);

struct TempE2ERepo {
    path: PathBuf,
}

impl TempE2ERepo {
    fn new(prefix: &str) -> Self {
        let id = NEXT_E2E_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "git_med_e2e_{}_{}_{}",
            prefix,
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("failed to create temp e2e dir");
        Self { path }
    }
}

impl Drop for TempE2ERepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Recursively count leaf instances in the in-memory tree.
fn count_tree_instances(node_ref: &crate::core::tree::NodeRef) -> usize {
    let node = node_ref.borrow();
    match &node.children {
        crate::core::tree::Children::None => 1,
        crate::core::tree::Children::Attributes(map) | crate::core::tree::Children::Files(map) => {
            map.values().map(count_tree_instances).sum()
        }
    }
}

/// Find a tracked data file on disk (excluding `.git-med`) to delete in the test.
fn find_first_data_file(dir: &Path) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != ".git-med" {
                    if let Some(f) = find_first_data_file(&path) {
                        return Some(f);
                    }
                }
            } else {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(".view.")
                    && !name.starts_with("meta.")
                    && (name.ends_with(".dcm")
                        || name.ends_with(".nii")
                        || name.ends_with(".nii.gz"))
                {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Verify files materialized under Qomra layout: `<task>/<source>/<subject>/<study>/<series>/<instance>`.
fn verify_qomra_files(repo_root: &Path) -> Result<usize, String> {
    let mut count = 0;
    for task_name in ["data", "segmentation", "localisation"] {
        let task_dir = repo_root.join(task_name);
        if task_dir.is_dir() {
            collect_qomra_instances(&task_dir, 1, &mut count);
        }
    }
    if count == 0 {
        return Err("no valid Qomra instances found in expected hierarchy".to_string());
    }
    Ok(count)
}

fn collect_qomra_instances(dir: &Path, depth: usize, count: &mut usize) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_qomra_instances(&path, depth + 1, count);
            } else if depth >= 5 {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(".view.")
                    && !name.starts_with("meta.")
                    && (name.ends_with(".dcm")
                        || name.ends_with(".nii")
                        || name.ends_with(".nii.gz"))
                {
                    *count += 1;
                }
            }
        }
    }
}

/// Verify files materialized under DICOM layout: `<subject>/<study>/<series>/<instance>.dcm` with sidecars.
fn verify_dicom_files(repo_root: &Path) -> Result<usize, String> {
    let mut count = 0;
    scan_dicom_rec(repo_root, &mut count);
    if count == 0 {
        return Err("no valid DICOM instances with sidecars found".to_string());
    }
    Ok(count)
}

fn scan_dicom_rec(dir: &Path, count: &mut usize) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != ".git-med" {
                    scan_dicom_rec(&path, count);
                }
            } else {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(".view.") && name.ends_with(".dcm") {
                    if let Some(parent) = path.parent() {
                        let sidecar = parent.join(format!(".view.{}", name));
                        if sidecar.is_file() {
                            if let Ok(content) = fs::read_to_string(&sidecar) {
                                if !content.trim().is_empty() {
                                    *count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Verify files materialized under BraTs layout: `<subject>/[<study>/]<instance>.nii.gz` with sidecars.
fn verify_brats_files(repo_root: &Path) -> Result<usize, String> {
    let mut count = 0;
    scan_brats_rec(repo_root, &mut count);
    if count == 0 {
        return Err("no valid BraTs instances with sidecars found".to_string());
    }
    Ok(count)
}

fn scan_brats_rec(dir: &Path, count: &mut usize) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != ".git-med" {
                    scan_brats_rec(&path, count);
                }
            } else {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(".view.") && (name.ends_with(".nii") || name.ends_with(".nii.gz")) {
                    if let Some(parent) = path.parent() {
                        let sidecar = parent.join(format!(".view.{}", name));
                        if sidecar.is_file() {
                            if let Ok(content) = fs::read_to_string(&sidecar) {
                                if !content.trim().is_empty() {
                                    *count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Verify that files on disk conform to the target view's directory structure and sidecars.
fn verify_view_layout(dir: &Path, view: &str) -> Result<usize, String> {
    match view {
        "qomra" => verify_qomra_files(dir),
        "dicom" => verify_dicom_files(dir),
        "brats" => verify_brats_files(dir),
        _ => Err(format!("unknown view: {}", view)),
    }
}

/// Verify that a specific named instance exists on disk in the target view layout.
fn verify_instance_exists_in_view(dir: &Path, view: &str, instance_name: &str) -> bool {
    let mut found = false;
    scan_for_instance_rec(dir, view, instance_name, &mut found);
    found
}

fn scan_for_instance_rec(dir: &Path, view: &str, target_name: &str, found: &mut bool) {
    if *found {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != ".git-med" {
                    scan_for_instance_rec(&path, view, target_name, found);
                }
            } else {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(".view.") && name.contains(target_name) {
                    match view {
                        "dicom" => {
                            if name.ends_with(".dcm") {
                                if let Some(parent) = path.parent() {
                                    if parent.join(format!(".view.{}", name)).is_file() {
                                        *found = true;
                                        return;
                                    }
                                }
                            }
                        }
                        "brats" => {
                            if name.ends_with(".nii") || name.ends_with(".nii.gz") {
                                if let Some(parent) = path.parent() {
                                    if parent.join(format!(".view.{}", name)).is_file() {
                                        *found = true;
                                        return;
                                    }
                                }
                            }
                        }
                        "qomra" => {
                            let path_str = path.to_string_lossy();
                            if path_str.contains("data") || path_str.contains("segmentation") {
                                *found = true;
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Find a suitable directory on disk to place a new instance file and sidecar.
fn create_new_instance_in_view(repo_root: &Path, view: &str, instance_name: &str) -> PathBuf {
    match view {
        "brats" => {
            let mut target_dir = repo_root.to_path_buf();
            if let Ok(entries) = fs::read_dir(repo_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && entry.file_name() != ".git-med" {
                        if let Ok(sub_entries) = fs::read_dir(&path) {
                            for sub in sub_entries.flatten() {
                                if sub.path().is_dir() {
                                    target_dir = sub.path();
                                    break;
                                }
                            }
                        }
                        if target_dir == repo_root {
                            target_dir = path;
                        }
                        break;
                    }
                }
            }
            let inst_file = target_dir.join(format!("{}.nii.gz", instance_name));
            fs::write(&inst_file, BRATS_SAMPLE).expect("write new brats instance");
            let sidecar_file = target_dir.join(format!(".view.{}.nii.gz", instance_name));
            fs::write(&sidecar_file, "clinic\ndata\n").expect("write new brats sidecar");
            inst_file
        }
        "dicom" => {
            let series_dir = repo_root
                .join("subject_001")
                .join("study_001")
                .join("series_001");
            fs::create_dir_all(&series_dir).expect("create series dir");
            let inst_file = series_dir.join(format!("{}.dcm", instance_name));
            fs::write(&inst_file, DICOM_SAMPLE).expect("write new dicom instance");
            let sidecar_file = series_dir.join(format!(".view.{}.dcm", instance_name));
            fs::write(&sidecar_file, "clinic\ndata\n").expect("write new dicom sidecar");
            inst_file
        }
        "qomra" => {
            let series_dir = repo_root
                .join("data")
                .join("clinic")
                .join("subject_001")
                .join("study_001")
                .join("series_001");
            fs::create_dir_all(&series_dir).expect("create qomra series dir");
            let inst_file = series_dir.join(format!("{}.nii.gz", instance_name));
            fs::write(&inst_file, BRATS_SAMPLE).expect("write new qomra instance");
            inst_file
        }
        _ => panic!("unsupported view {}", view),
    }
}

/// Generic E2E workflow runner executing strictly `git-med` CLI subprocess commands.
/// Sequence: Init V1 -> Add -> Commit -> Branch feature -> Delete -> Add -> Commit
///           -> Switch V2 & verify files -> Switch V3 & verify files
///           -> Create instance + .view in V3 -> Add -> Commit
///           -> Switch V1 & verify files + new instance
pub fn run_e2e_workflow(v1: &str, v2: &str, v3: &str) {
    let repo_temp = TempE2ERepo::new(&format!("{}_{}_{}", v1, v2, v3));
    let cli = GitMedCli::new(&repo_temp.path);

    // 1. Initialize repository
    cli.init(".").expect("git-med init failed");
    if v1 != "qomra" {
        cli.view(Some(v1)).expect("switch initial view failed");
    }
    assert_eq!(cli.read_current_view().expect("read VIEW"), v1);
    assert_eq!(cli.read_head().expect("read HEAD"), "ref: refs/heads/main");

    // 2. Generate initial sample data (2 files per series so 1 remains after deleting 1)
    cli.generate_for_view(v1, ".", 1, 1, 1, 2)
        .expect("git-med generate failed");

    // For Qomra, also add a metadata file to exercise metafile indexing and object verification
    if v1 == "qomra" {
        let meta_path = repo_temp.path.join("data/clinic/meta.clinic.json");
        fs::write(&meta_path, r#"{"dept":"radiology","test":"e2e"}"#)
            .expect("write qomra clinic meta");
    }

    cli.add().expect("git-med add failed");

    // Verify objects and index file
    assert!(cli.index_file_exists(), ".git-med/index must exist after add");
    assert!(cli.index_file_size() > 0, ".git-med/index must be non-empty");
    assert!(cli.count_objects() > 0, "objects must exist after initial add");

    let tree_init = cli.load_index_tree().expect("load initial index tree");
    let init_instances = count_tree_instances(&tree_init.root);
    assert!(init_instances >= 2, "initial tree must contain at least 2 instances");

    // Verify that all tree nodes and meta nodes point to existing content-addressed objects
    let verified_objs_1 = cli
        .verify_tree_objects_exist(&tree_init)
        .expect("all tree node objects must exist in object stores");
    assert!(verified_objs_1 > 0);

    // 3. Commit initial state
    cli.commit("initial commit").expect("git-med commit failed");

    let commit_hash_1 = cli
        .read_branch_ref("main")
        .expect("read refs/heads/main");
    assert_eq!(commit_hash_1.len(), 64, "commit hash must be 64-char hex");
    assert!(
        cli.object_exists(&commit_hash_1),
        "commit object {} must exist in objects/",
        commit_hash_1
    );

    // Verify that the tree saved in the commit matches the index tree shape
    let committed_tree_1 = cli
        .load_commit_tree(&commit_hash_1)
        .expect("load committed tree from commit object");
    assert_tree_shape_eq(&tree_init, &committed_tree_1);

    // After commit, index on disk was updated with computed hashes
    let index_tree_post_commit_1 = cli.load_index_tree().expect("load post-commit index tree");
    assert_tree_shape_eq(&index_tree_post_commit_1, &committed_tree_1);
    assert_tree_hashes_eq(&index_tree_post_commit_1, &committed_tree_1);

    // Verify objects and meta files again after commit
    cli.verify_tree_objects_exist(&committed_tree_1)
        .expect("all committed tree objects and meta objects must exist");

    // 4. Switch branch and verify refs
    cli.branch("feature").expect("git-med branch feature failed");
    assert_eq!(
        cli.read_head().expect("read HEAD"),
        "ref: refs/heads/feature",
        "HEAD must point to feature branch"
    );
    let feature_ref = cli
        .read_branch_ref("feature")
        .expect("read refs/heads/feature");
    assert_eq!(
        feature_ref, commit_hash_1,
        "feature branch ref must point to initial commit"
    );

    // 5. Delete a file in the working directory and add
    let file_to_delete = find_first_data_file(&repo_temp.path)
        .expect("should find a data file to delete");
    fs::remove_file(&file_to_delete).expect("failed to remove data file");

    cli.add().expect("git-med add after delete failed");

    // Verify objects and index file after delete
    assert!(cli.index_file_exists(), ".git-med/index must exist after delete add");
    let tree_after_delete = cli
        .load_index_tree()
        .expect("load index tree after delete");
    let del_instances = count_tree_instances(&tree_after_delete.root);
    assert_eq!(
        del_instances,
        init_instances - 1,
        "instance count in index tree must decrease by exactly 1"
    );

    // Verify that the tree shape changed
    let shape_diff = compare_tree_shapes(&tree_init, &tree_after_delete);
    assert!(
        shape_diff.is_err(),
        "tree shape must differ after deleting a file"
    );

    cli.verify_tree_objects_exist(&tree_after_delete)
        .expect("objects for tree after delete must exist");

    // 6. Commit on feature branch and verify objects and meta files
    cli.commit("delete file on feature branch")
        .expect("commit after delete failed");
    let commit_hash_2 = cli
        .read_branch_ref("feature")
        .expect("read refs/heads/feature after 2nd commit");
    assert_ne!(
        commit_hash_1, commit_hash_2,
        "second commit hash must differ from first"
    );
    assert!(
        cli.object_exists(&commit_hash_2),
        "second commit object {} must exist",
        commit_hash_2
    );

    let committed_tree_2 = cli
        .load_commit_tree(&commit_hash_2)
        .expect("load committed tree 2");
    assert_tree_shape_eq(&tree_after_delete, &committed_tree_2);

    let index_tree_post_commit_2 = cli.load_index_tree().expect("load post-commit index tree 2");
    assert_tree_shape_eq(&index_tree_post_commit_2, &committed_tree_2);
    assert_tree_hashes_eq(&index_tree_post_commit_2, &committed_tree_2);
    cli.verify_tree_objects_exist(&committed_tree_2)
        .expect("objects and meta objects for commit 2 must exist");

    // 7. Switch view to V2 and verify files
    cli.view(Some(v2)).expect("switch to V2 failed");
    assert_eq!(cli.read_current_view().expect("read VIEW"), v2);
    let v2_instance_count = verify_view_layout(&repo_temp.path, v2)
        .unwrap_or_else(|e| panic!("failed to verify files in view {}: {}", v2, e));
    assert!(v2_instance_count > 0, "view {} must materialize instances", v2);

    // 8. Switch view to V3 and verify files
    cli.view(Some(v3)).expect("switch to V3 failed");
    assert_eq!(cli.read_current_view().expect("read VIEW"), v3);
    let v3_instance_count = verify_view_layout(&repo_temp.path, v3)
        .unwrap_or_else(|e| panic!("failed to verify files in view {}: {}", v3, e));
    assert!(v3_instance_count > 0, "view {} must materialize instances", v3);

    // 9. Create a new instance and .view file, add, commit and verify
    let count_before = count_tree_instances(&cli.load_index_tree().expect("load tree before new instance").root);
    let new_instance_file =
        create_new_instance_in_view(&repo_temp.path, v3, "e2e_extra_instance");
    assert!(
        new_instance_file.exists(),
        "new instance file must exist on disk: {}",
        new_instance_file.display()
    );

    cli.add().expect("git-med add after new instance failed");
    let tree_after_new_instance = cli
        .load_index_tree()
        .expect("load index tree after new instance");
    let new_inst_count = count_tree_instances(&tree_after_new_instance.root);
    assert_eq!(
        new_inst_count,
        count_before + 1,
        "tree instance count must increase by 1 after adding new instance"
    );

    // Verify tree shape differs from post-delete tree
    assert!(
        compare_tree_shapes(&tree_after_delete, &tree_after_new_instance).is_err(),
        "tree shape must change after adding new instance"
    );

    cli.verify_tree_objects_exist(&tree_after_new_instance)
        .expect("objects for tree after new instance must exist");

    cli.commit("add new instance in V3")
        .expect("commit after new instance failed");
    let commit_hash_3 = cli
        .read_branch_ref("feature")
        .expect("read refs/heads/feature after 3rd commit");
    assert_ne!(
        commit_hash_2, commit_hash_3,
        "third commit hash must differ from second"
    );
    assert!(
        cli.object_exists(&commit_hash_3),
        "commit object {} must exist",
        commit_hash_3
    );

    let committed_tree_3 = cli
        .load_commit_tree(&commit_hash_3)
        .expect("load committed tree 3");
    assert_tree_shape_eq(&tree_after_new_instance, &committed_tree_3);

    let index_tree_post_commit_3 = cli.load_index_tree().expect("load post-commit index tree 3");
    assert_tree_shape_eq(&index_tree_post_commit_3, &committed_tree_3);
    assert_tree_hashes_eq(&index_tree_post_commit_3, &committed_tree_3);
    cli.verify_tree_objects_exist(&committed_tree_3)
        .expect("objects and meta objects for commit 3 must exist");

    // 10. Switch view back to V1 and verify files
    cli.view(Some(v1)).expect("switch back to V1 failed");
    assert_eq!(cli.read_current_view().expect("read VIEW"), v1);

    let v1_final_count = verify_view_layout(&repo_temp.path, v1)
        .unwrap_or_else(|e| panic!("failed to verify files in view {} after switch-back: {}", v1, e));
    assert!(v1_final_count > 0, "view {} must materialize instances after switch back", v1);

    // Crucially verify that the new instance created in V3 is now materialized in V1's layout
    assert!(
        verify_instance_exists_in_view(&repo_temp.path, v1, "e2e_extra_instance"),
        "newly added instance 'e2e_extra_instance' must be materialized on disk in view {}",
        v1
    );

    // Final Tree comparison: verify index tree and commit tree match in both shape and hashes
    assert_tree_shape_eq(&index_tree_post_commit_3, &committed_tree_3);
    assert_tree_hashes_eq(&index_tree_post_commit_3, &committed_tree_3);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_e2e_qomra_dicom_brats_qomra() {
        run_e2e_workflow("qomra", "dicom", "brats");
    }

    #[test]
    fn test_e2e_brats_qomra_dicom_brats() {
        run_e2e_workflow("brats", "qomra", "dicom");
    }

    #[test]
    fn test_e2e_dicom_brats_qomra_dicom() {
        run_e2e_workflow("dicom", "brats", "qomra");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_e2e_cli_clone_and_pull() {
        use crate::files::types::ObjectType;
        use crate::remote::packfile::create_packfile_bytes;
        use crate::remote::pkt_line::{self, encode_str};
        use crate::remote::traversal::collect_objects_to_pack;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let src_repo = TempE2ERepo::new("cli_src");
        crate::core::init::init_at(&src_repo.path);
        let cli_src = GitMedCli::new(&src_repo.path);

        // Generate data in src
        crate::tester::generate::sample::qomra(&src_repo.path, &["data"], 1, 1, 1, 1);
        cli_src.add().expect("add");
        cli_src.commit("commit 1").expect("commit");

        let head_hash_str = cli_src.read_branch_ref("main").expect("read main ref");
        let head_hash = crate::files::hash::Hash::from_hex(&head_hash_str).expect("parse hash");

        let objects = collect_objects_to_pack(&src_repo.path, &head_hash, &[]).expect("collect");
        let obj_slices: Vec<(ObjectType, &[u8])> = objects.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack_bytes = create_packfile_bytes(&obj_slices).expect("pack bytes");

        // Spawn mock git HTTP server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_url = format!("http://127.0.0.1:{}", port);
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept_res = listener.accept() => {
                        let Ok((mut socket, _)) = accept_res else { break; };
                        let head_hash = head_hash;
                        let pack_bytes = pack_bytes.clone();
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 8192];
                            let n = match socket.read(&mut buf).await {
                                Ok(n) if n > 0 => n,
                                _ => return,
                            };
                            let req = String::from_utf8_lossy(&buf[..n]);

                            if req.contains("GET /info/refs?service=git-upload-pack") {
                                let mut adv = Vec::new();
                                adv.extend_from_slice(&encode_str("version 2\n").unwrap());
                                adv.extend_from_slice(&encode_str("ls-refs\n").unwrap());
                                adv.extend_from_slice(&encode_str("fetch=shallow\n").unwrap());
                                adv.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-advertisement\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    adv.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&adv).await;
                            } else if req.contains("POST /git-upload-pack") {
                                if req.contains("command=ls-refs") {
                                    let mut ls = Vec::new();
                                    let l1 = format!("{} HEAD symref-target:refs/heads/main\n", head_hash.to_hex());
                                    let l2 = format!("{} refs/heads/main\n", head_hash.to_hex());
                                    ls.extend_from_slice(&encode_str(&l1).unwrap());
                                    ls.extend_from_slice(&encode_str(&l2).unwrap());
                                    ls.extend_from_slice(pkt_line::encode_flush());

                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        ls.len()
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.write_all(&ls).await;
                                } else if req.contains("command=fetch") {
                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        pack_bytes.len()
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.write_all(&pack_bytes).await;
                                }
                            } else if req.contains("POST /lfs/download-urls") {
                                let body_start = req.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
                                let body_bytes = &buf[body_start..n];
                                if let Ok(req_json) = serde_json::from_slice::<crate::remote::types::LfsDownloadUrlsReq>(body_bytes) {
                                    let base_url = format!("http://127.0.0.1:{}", port);
                                    let mut urls = std::collections::HashMap::new();
                                    for h in req_json.hashes {
                                        urls.insert(h.clone(), format!("{}/s3-download/{}", base_url, h));
                                    }
                                    let resp_json = crate::remote::types::LfsUrlsResp { urls };
                                    let resp_bytes = serde_json::to_vec(&resp_json).unwrap();
                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        resp_bytes.len()
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.write_all(&resp_bytes).await;
                                }
                            } else if req.contains("GET /s3-download/") {
                                let raw_bytes = crate::tester::generate::sample::DICOM_SAMPLE.to_vec();
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    raw_bytes.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&raw_bytes).await;
                            }
                        });
                    }
                }
            }
        });

        // Test git-med clone <url> <dest>
        let dest_parent = TempE2ERepo::new("cli_dest_parent");
        let dest_path = dest_parent.path.join("cloned_repo");
        let cli_parent = GitMedCli::new(&dest_parent.path);
        cli_parent
            .clone_cmd(&server_url, Some(&dest_path.display().to_string()))
            .expect("cli clone");

        let cli_cloned = GitMedCli::new(&dest_path);

        // Verify config
        let config_path = dest_path.join(".git-med/config");
        assert!(config_path.is_file());
        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("[remote \"origin\"]"));
        assert!(config_content.contains(&server_url));

        // Verify files materialized
        let dcm_file = dest_path.join("data/clinic/subject_001/study_001/series_001/instance_001.dcm");
        assert!(dcm_file.is_file());

        // Verify index populated
        assert!(cli_cloned.index_file_size() > 0);

        // Test git-med pull (already up to date)
        let pull_out = cli_cloned.pull_cmd(None, None).expect("cli pull");
        let stdout = String::from_utf8_lossy(&pull_out.stdout);
        assert!(stdout.contains("Already up to date."));
    }
}

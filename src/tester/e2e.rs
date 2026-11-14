use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::tester::cli_runner::GitMedCli;
use crate::tester::compare::{assert_tree_shape_eq, compare_tree_shapes};
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

/// Recursively find a tracked file on disk (excluding `.git-med`) to delete in the test.
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

/// Recursively check whether any data files matching the view exist on disk.
fn verify_view_files_exist(dir: &Path, view: &str) -> bool {
    let mut found = false;
    check_view_files_rec(dir, view, &mut found);
    found
}

fn check_view_files_rec(dir: &Path, view: &str, found: &mut bool) {
    if *found {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != ".git-med" {
                    check_view_files_rec(&path, view, found);
                }
            } else {
                let name = entry.file_name().to_string_lossy().to_string();
                match view {
                    "dicom" => {
                        if name.ends_with(".dcm") {
                            *found = true;
                            return;
                        }
                    }
                    "brats" => {
                        if name.ends_with(".nii") || name.ends_with(".nii.gz") {
                            *found = true;
                            return;
                        }
                    }
                    "qomra" => {
                        if name.ends_with(".nii")
                            || name.ends_with(".nii.gz")
                            || name.ends_with(".dcm")
                        {
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

/// Find a suitable directory on disk to place a new instance file and sidecar.
fn create_new_instance_in_view(repo_root: &Path, view: &str, instance_name: &str) -> PathBuf {
    match view {
        "brats" => {
            // Find a subject directory
            let mut target_dir = repo_root.to_path_buf();
            if let Ok(entries) = fs::read_dir(repo_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && entry.file_name() != ".git-med" {
                        target_dir = path;
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
            // Find a series directory or create one under subject_001/study_001/series_001
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

/// Generic E2E workflow runner executing strictly `git-med` CLI commands.
/// Sequence: v1 -> delete -> commit -> v2 -> v3 (create instance) -> v1
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

    // 2. Generate initial sample data and add (2 files per series so 1 remains after deleting 1)
    cli.generate_for_view(v1, ".", 1, 1, 1, 2)
        .expect("git-med generate failed");
    cli.add().expect("git-med add failed");

    // Verify objects and index file
    assert!(
        cli.count_objects() > 0,
        "objects should exist after initial add"
    );
    let tree_init = cli.load_index_tree().expect("load initial index tree");
    assert_tree_shape_eq(&tree_init, &tree_init);

    // 3. Commit initial state
    cli.commit("initial commit").expect("git-med commit failed");
    let commit_hash_1 = cli
        .read_branch_ref("main")
        .expect("read refs/heads/main");
    assert_eq!(commit_hash_1.len(), 64, "commit hash must be 64-char hex");
    assert!(
        cli.object_exists(&commit_hash_1),
        "commit object {} must exist",
        commit_hash_1
    );

    // 4. Switch branch
    cli.branch("feature").expect("git-med branch feature failed");
    assert_eq!(
        cli.read_head().expect("read HEAD"),
        "ref: refs/heads/feature"
    );
    let feature_ref = cli
        .read_branch_ref("feature")
        .expect("read refs/heads/feature");
    assert_eq!(feature_ref, commit_hash_1);

    // 5. Delete a file in the working directory and add
    let file_to_delete = find_first_data_file(&repo_temp.path)
        .expect("should find a data file to delete");
    fs::remove_file(&file_to_delete).expect("failed to remove data file");

    cli.add().expect("git-med add after delete failed");
    let tree_after_delete = cli
        .load_index_tree()
        .expect("load index tree after delete");

    // Verify that the tree shape changed (deleted file removed from index)
    let shape_res = compare_tree_shapes(&tree_init, &tree_after_delete);
    assert!(
        shape_res.is_err(),
        "tree shape must differ after deleting a file"
    );

    // 6. Commit on feature branch
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
        "commit object {} must exist",
        commit_hash_2
    );

    // 7. Switch view to V2 and verify files
    cli.view(Some(v2)).expect("switch to V2 failed");
    assert_eq!(cli.read_current_view().expect("read VIEW"), v2);
    assert!(
        verify_view_files_exist(&repo_temp.path, v2),
        "files for view {} must exist after switching",
        v2
    );

    // 8. Switch view to V3 and verify files
    cli.view(Some(v3)).expect("switch to V3 failed");
    assert_eq!(cli.read_current_view().expect("read VIEW"), v3);
    assert!(
        verify_view_files_exist(&repo_temp.path, v3),
        "files for view {} must exist after switching",
        v3
    );

    // 9. Create a new instance and .view file, add, commit and verify
    let new_instance_file =
        create_new_instance_in_view(&repo_temp.path, v3, "e2e_extra_instance");
    assert!(
        new_instance_file.exists(),
        "new instance file must exist on disk"
    );

    cli.add().expect("git-med add after new instance failed");
    let tree_after_new_instance = cli
        .load_index_tree()
        .expect("load index tree after new instance");

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

    // 10. Switch view back to V1 and verify
    cli.view(Some(v1)).expect("switch back to V1 failed");
    assert_eq!(cli.read_current_view().expect("read VIEW"), v1);
    assert!(
        verify_view_files_exist(&repo_temp.path, v1),
        "files for view {} must exist after switching back",
        v1
    );

    // 11. Final Tree shape check
    assert_tree_shape_eq(&tree_after_new_instance, &tree_after_new_instance);
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
}

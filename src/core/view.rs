use std::fs;
use std::path::Path;

use crate::core::tree::load_or_init_tree;
use crate::views::get_view_in;

/// Read and normalize the view name from `view_path`.
/// Returns `"qomra"` by default if the file is missing or empty.
pub fn read_view_name(view_path: &Path) -> Result<String, String> {
    match fs::read_to_string(view_path) {
        Ok(content) => {
            let trimmed = content.trim().to_ascii_lowercase();
            if trimmed.is_empty() {
                Ok("qomra".to_string())
            } else {
                Ok(trimmed)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("qomra".to_string()),
        Err(e) => Err(format!("failed to read {}: {}", view_path.display(), e)),
    }
}

/// Write the view name to `view_path` with a trailing newline.
pub fn set_view_name(view_path: &Path, view_name: &str) -> Result<(), String> {
    fs::write(view_path, format!("{}\n", view_name))
        .map_err(|e| format!("failed to write {}: {}", view_path.display(), e))
}

/// Query the current view or switch to a new view and write the workspace.
pub fn view_in(repo_root: &Path, target_view: Option<&str>) -> Result<String, String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!(
            "not a git-med repository (or any of the parent directories): {}",
            repo_root.display()
        ));
    }

    let view_path = git_med_dir.join("VIEW");

    match target_view {
        None => {
            let current = read_view_name(&view_path)?;
            Ok(format!("Current view: {}", current))
        }
        Some(raw_target) => {
            let target = raw_target.trim().to_ascii_lowercase();
            if !matches!(target.as_str(), "qomra" | "dicom" | "brats") {
                return Err(format!(
                    "unsupported view '{}'. Valid options are: qomra, dicom, brats",
                    raw_target
                ));
            }

            set_view_name(&view_path, &target)?;

            let index_path = git_med_dir.join("index");
            let tree = load_or_init_tree(&index_path)?;

            let view = get_view_in(&target, repo_root)
                .ok_or_else(|| format!("unsupported view '{}'", target))?;

            view.write(&tree.root)
                .map_err(|e| format!("view write failed: {}", e))?;

            Ok(format!("Switched view to '{}'", target))
        }
    }
}

pub fn view(args: &[String]) {
    let target = if args.len() >= 3 {
        Some(args[2].as_str())
    } else {
        None
    };

    match view_in(Path::new("."), target) {
        Ok(msg) => println!("{}", msg),
        Err(err) => {
            eprintln!("git-med view failed: {}", err);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::add::add_in;
    use crate::core::init::init_at;
    use crate::tester::generate::sample::dicom;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

    struct ViewTempRepo {
        path: PathBuf,
    }

    impl ViewTempRepo {
        fn new(prefix: &str) -> Self {
            let id = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "git_med_view_test_{}_{}_{}",
                prefix,
                std::process::id(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp test dir");
            Self { path }
        }
    }

    impl Drop for ViewTempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_view_not_in_repo() {
        let repo = ViewTempRepo::new("not_repo");
        let result = view_in(&repo.path, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a git-med repository"));
    }

    #[test]
    fn test_view_get_default() {
        let repo = ViewTempRepo::new("get_default");
        init_at(&repo.path);

        let result = view_in(&repo.path, None).unwrap();
        assert_eq!(result, "Current view: qomra");
    }

    #[test]
    fn test_view_switch_invalid_name() {
        let repo = ViewTempRepo::new("invalid_name");
        init_at(&repo.path);

        let result = view_in(&repo.path, Some("unknown_view"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("unsupported view 'unknown_view'"));
    }

    #[test]
    fn test_view_switch_empty_repo() {
        let repo = ViewTempRepo::new("switch_empty");
        init_at(&repo.path);

        let res = view_in(&repo.path, Some("brats")).unwrap();
        assert_eq!(res, "Switched view to 'brats'");
        assert_eq!(view_in(&repo.path, None).unwrap(), "Current view: brats");

        let res2 = view_in(&repo.path, Some("DICOM")).unwrap();
        assert_eq!(res2, "Switched view to 'dicom'");
        assert_eq!(view_in(&repo.path, None).unwrap(), "Current view: dicom");
    }

    #[test]
    fn test_view_switch_and_materialize_brats_and_dicom() {
        let repo = ViewTempRepo::new("switch_materialize");
        init_at(&repo.path);

        // Switch to DICOM view initially
        view_in(&repo.path, Some("dicom")).expect("switch to dicom failed");
        assert_eq!(
            read_view_name(&repo.path.join(".git-med/VIEW")).unwrap(),
            "dicom"
        );

        // Generate DICOM dataset in repo
        dicom(&repo.path, 1, 1, 1, 1);

        // Run add to index the tree (converts instances to NIfTI-2 objects and meta drafts)
        add_in(&repo.path).expect("add_in failed");

        // Switch to BraTS
        let res_brats = view_in(&repo.path, Some("brats")).expect("switch to brats failed");
        assert_eq!(res_brats, "Switched view to 'brats'");
        assert_eq!(
            read_view_name(&repo.path.join(".git-med/VIEW")).unwrap(),
            "brats"
        );

        // Check that BraTS sidecar exists: .view.<filename>
        let mut found_brats_sidecar = false;
        fn search_sidecar(dir: &Path, found: &mut bool) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && entry.file_name() != ".git-med" {
                        search_sidecar(&path, found);
                    } else if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".view.")
                    {
                        *found = true;
                    }
                }
            }
        }
        search_sidecar(&repo.path, &mut found_brats_sidecar);
        assert!(found_brats_sidecar, "Expected BraTS sidecar file to exist");

        // Switch back to DICOM
        let res_dicom = view_in(&repo.path, Some("dicom")).expect("switch to dicom failed");
        assert_eq!(res_dicom, "Switched view to 'dicom'");
        assert_eq!(
            read_view_name(&repo.path.join(".git-med/VIEW")).unwrap(),
            "dicom"
        );

        // Check that DICOM files (.dcm) exist
        let mut found_dcm = false;
        fn search_dcm(dir: &Path, found: &mut bool) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() && entry.file_name() != ".git-med" {
                        search_dcm(&path, found);
                    } else if path.extension().map(|e| e == "dcm").unwrap_or(false) {
                        *found = true;
                    }
                }
            }
        }
        search_dcm(&repo.path, &mut found_dcm);
        assert!(found_dcm, "Expected reconstructed .dcm file to exist");

        // Switch to Qomra
        let res_qomra = view_in(&repo.path, Some("qomra")).expect("switch to qomra failed");
        assert_eq!(res_qomra, "Switched view to 'qomra'");
        assert_eq!(
            read_view_name(&repo.path.join(".git-med/VIEW")).unwrap(),
            "qomra"
        );
    }
}
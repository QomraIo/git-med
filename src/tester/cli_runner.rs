use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::core::tree::Tree;

#[allow(dead_code)]
pub struct GitMedCli {
    pub repo_root: PathBuf,
    pub bin_path: PathBuf,
}

#[allow(dead_code)]
impl GitMedCli {
    /// Create a new CLI harness operating inside `repo_root`.
    pub fn new(repo_root: &Path) -> Self {
        let bin_path = find_git_med_bin();
        Self {
            repo_root: repo_root.to_path_buf(),
            bin_path,
        }
    }

    /// Execute a git-med command with arguments and verify successful exit.
    pub fn run_cmd(&self, args: &[&str]) -> Result<Output, String> {
        let output = Command::new(&self.bin_path)
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .map_err(|e| {
                format!(
                    "failed to execute {} {:?}: {}",
                    self.bin_path.display(),
                    args,
                    e
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "git-med {:?} exited with code {:?}\nstdout: {}\nstderr: {}",
                args,
                output.status.code(),
                stdout,
                stderr
            ));
        }

        Ok(output)
    }

    /// git-med init <name>
    pub fn init(&self, name: &str) -> Result<Output, String> {
        self.run_cmd(&["init", name])
    }

    /// git-med add
    pub fn add(&self) -> Result<Output, String> {
        self.run_cmd(&["add"])
    }

    /// git-med commit -m "<message>"
    pub fn commit(&self, message: &str) -> Result<Output, String> {
        self.run_cmd(&["commit", "-m", message])
    }

    /// git-med branch <branch_name>
    pub fn branch(&self, branch_name: &str) -> Result<Output, String> {
        self.run_cmd(&["branch", branch_name])
    }

    /// git-med clone <url> [target_dir]
    pub fn clone_cmd(&self, url: &str, target_dir: Option<&str>) -> Result<Output, String> {
        if let Some(dir) = target_dir {
            self.run_cmd(&["clone", url, dir])
        } else {
            self.run_cmd(&["clone", url])
        }
    }

    /// git-med push [remote] [branch]
    pub fn push_cmd(&self, remote: Option<&str>, branch: Option<&str>) -> Result<Output, String> {
        let mut args = vec!["push"];
        if let Some(r) = remote {
            args.push(r);
        }
        if let Some(b) = branch {
            args.push(b);
        }
        self.run_cmd(&args)
    }

    /// git-med pull [remote] [branch]
    pub fn pull_cmd(&self, remote: Option<&str>, branch: Option<&str>) -> Result<Output, String> {
        let mut args = vec!["pull"];
        if let Some(r) = remote {
            args.push(r);
        }
        if let Some(b) = branch {
            args.push(b);
        }
        self.run_cmd(&args)
    }

    /// git-med view [target_view]
    pub fn view(&self, target_view: Option<&str>) -> Result<String, String> {
        let output = match target_view {
            Some(v) => self.run_cmd(&["view", v])?,
            None => self.run_cmd(&["view"])?,
        };
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// git-med generate <view> <output_dir> ...
    pub fn generate_for_view(
        &self,
        view: &str,
        output_dir: &str,
        n_subjects: usize,
        study_per_subjects: usize,
        series_per_study: usize,
        file_per_series: usize,
    ) -> Result<Output, String> {
        match view {
            "brats" => self.run_cmd(&[
                "generate",
                "brats",
                output_dir,
                &n_subjects.to_string(),
                &study_per_subjects.to_string(),
                &file_per_series.to_string(),
            ]),
            "dicom" => self.run_cmd(&[
                "generate",
                "dicom",
                output_dir,
                &n_subjects.to_string(),
                &study_per_subjects.to_string(),
                &series_per_study.to_string(),
                &file_per_series.to_string(),
            ]),
            "qomra" => self.run_cmd(&[
                "generate",
                "qomra",
                output_dir,
                &n_subjects.to_string(),
                &study_per_subjects.to_string(),
                &series_per_study.to_string(),
                &file_per_series.to_string(),
                "clinic",
            ]),
            _ => Err(format!("unknown view for generate: {}", view)),
        }
    }

    // ========================================================================
    // Repository Inspection Helpers
    // ========================================================================

    pub fn git_med_dir(&self) -> PathBuf {
        self.repo_root.join(".git-med")
    }

    pub fn read_current_view(&self) -> Result<String, String> {
        let path = self.git_med_dir().join("VIEW");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read VIEW file {}: {}", path.display(), e))?;
        Ok(content.trim().to_ascii_lowercase())
    }

    pub fn read_head(&self) -> Result<String, String> {
        let path = self.git_med_dir().join("HEAD");
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read HEAD file {}: {}", path.display(), e))?;
        Ok(content.trim().to_string())
    }

    pub fn read_branch_ref(&self, branch: &str) -> Result<String, String> {
        let path = self.git_med_dir().join("refs").join("heads").join(branch);
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read ref {}: {}", path.display(), e))?;
        Ok(content.trim().to_string())
    }

    pub fn load_index_tree(&self) -> Result<Tree, String> {
        let path = self.git_med_dir().join("index");
        Tree::load(&path)
            .map_err(|e| format!("failed to load index tree {}: {}", path.display(), e))
    }

    pub fn count_objects(&self) -> usize {
        let dir = self.git_med_dir().join("objects");
        count_objects_recursive(&dir)
    }

    pub fn object_exists(&self, hash_hex: &str) -> bool {
        if hash_hex.len() < 3 {
            return false;
        }
        let dir = &hash_hex[..2];
        let file = &hash_hex[2..];
        self.git_med_dir()
            .join("objects")
            .join(dir)
            .join(file)
            .is_file()
    }

    pub fn index_file_exists(&self) -> bool {
        self.git_med_dir().join("index").is_file()
    }

    pub fn index_file_size(&self) -> u64 {
        fs::metadata(self.git_med_dir().join("index"))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    pub fn load_commit_tree(&self, commit_hash_hex: &str) -> Result<Tree, String> {
        let hash = crate::files::hash::Hash::from_hex(commit_hash_hex)
            .map_err(|e| format!("invalid commit hash {}: {}", commit_hash_hex, e))?;
        crate::files::commit::load_tree_from_commit_in(&self.repo_root, &hash)
    }

    pub fn verify_tree_objects_exist(&self, tree: &Tree) -> Result<usize, String> {
        let mut count = 0;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(std::rc::Rc::clone(&tree.root));

        while let Some(node_ref) = queue.pop_front() {
            let node = node_ref.borrow();
            if let Some(h) = node.self_hash {
                let hex = h.to_hex();
                if !self.object_exists(hex.as_str()) {
                    return Err(format!("node '{}' object {} does not exist", node.id, hex));
                }
                count += 1;
            }
            for (meta_name, meta_ref) in &node.meta {
                if let Some(mh) = meta_ref.borrow().self_hash {
                    let hex = mh.to_hex();
                    if !self.object_exists(hex.as_str()) {
                        return Err(format!(
                            "node '{}' meta '{}' object {} does not exist",
                            node.id, meta_name, hex
                        ));
                    }
                    count += 1;
                }
            }
            match &node.children {
                crate::core::tree::Children::Attributes(map)
                | crate::core::tree::Children::Files(map) => {
                    for child in map.values() {
                        queue.push_back(std::rc::Rc::clone(child));
                    }
                }
                crate::core::tree::Children::None => {}
            }
        }
        Ok(count)
    }
}

fn count_objects_recursive(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(sub_entries) = fs::read_dir(path) {
                    count += sub_entries
                        .flatten()
                        .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
                        .count();
                }
            } else if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                count += 1;
            }
        }
    }
    count
}

fn find_git_med_bin() -> PathBuf {
    // 1. Check CARGO_BIN_EXE_git-med (available when running cargo test)
    if let Ok(bin) = std::env::var("CARGO_BIN_EXE_git-med") {
        let p = PathBuf::from(bin);
        if p.exists() {
            return p;
        }
    }

    // 2. Check target/debug/git-med relative to CARGO_MANIFEST_DIR
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest_dir).join("target/debug/git-med");
        if p.exists() {
            return p;
        }
    }

    // 3. Fallback to current dir target/debug/git-med
    let fallback = PathBuf::from("target/debug/git-med");
    if fallback.exists() {
        return fallback;
    }

    // 4. Default to git-med on PATH
    PathBuf::from("git-med")
}

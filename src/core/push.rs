use std::fs;
use std::path::Path;
use crate::remote::{GitHttpClient, PushResult};

/// Resolve the current active branch name from `.git-med/HEAD`.
pub fn get_current_branch_in(repo_root: &Path) -> Result<String, String> {
    let head_path = repo_root.join(".git-med/HEAD");
    if !head_path.is_file() {
        return Err(format!("HEAD not found at {}", head_path.display()));
    }
    let content = fs::read_to_string(&head_path)
        .map_err(|e| format!("failed to read HEAD: {}", e))?;
    let trimmed = content.trim();
    if let Some(stripped) = trimmed.strip_prefix("ref: refs/heads/") {
        let branch = stripped.trim();
        if branch.is_empty() {
            Err("HEAD ref is empty".to_string())
        } else {
            Ok(branch.to_string())
        }
    } else {
        Err(format!("HEAD is detached or invalid: '{}'", trimmed))
    }
}

/// Push `branch_opt` to `remote_opt` under `repo_root`.
pub fn push_in(
    repo_root: &Path,
    remote_opt: Option<&str>,
    branch_opt: Option<&str>,
) -> Result<PushResult, String> {
    let git_med = repo_root.join(".git-med");
    if !git_med.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    let remote_name = remote_opt.unwrap_or("origin");
    let remote_entry = crate::core::remote::get_remote_in(repo_root, remote_name)?
        .ok_or_else(|| format!("fatal: remote '{}' not found in config", remote_name))?;

    let branch = match branch_opt {
        Some(b) => b.trim().to_string(),
        None => get_current_branch_in(repo_root)?,
    };

    if branch.is_empty() {
        return Err("branch name cannot be empty".to_string());
    }

    let client = GitHttpClient::new();
    crate::core::block_on(client.push_branch(&remote_entry.url, repo_root, &branch))
        .map_err(|e| format!("push failed: {}", e))
}

/// CLI handler for `git-med push [remote] [branch]`.
pub fn push(args: &[String]) {
    let remote_opt = args.get(2).map(|s| s.as_str());
    let branch_opt = args.get(3).map(|s| s.as_str());

    match push_in(Path::new("."), remote_opt, branch_opt) {
        Ok(res) => {
            let old_str = res
                .old_hash
                .map(|h| {
                    let hex = h.to_hex();
                    hex[..7.min(hex.len())].to_string()
                })
                .unwrap_or_else(|| "[new branch]".to_string());
            let new_hex = res.new_hash.to_hex();
            let new_str = &new_hex[..7.min(new_hex.len())];
            println!("To remote");
            println!("   {}..{}  {} -> {}", old_str, new_str, res.branch, res.branch);
        }
        Err(err) => {
            eprintln!("git-med push failed: {}", err);
            std::process::exit(1);
        }
    }
}

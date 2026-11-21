use std::path::Path;
use crate::core::push::get_current_branch_in;
use crate::remote::{GitHttpClient, PullResult};

/// Pull `branch_opt` from `remote_opt` into `repo_root`:
/// 1. Fetches remote packfile.
/// 2. Fast-forwards the branch ref.
/// 3. Updates HEAD.
/// 4. Materializes working tree via view.write.
pub fn pull_in(
    repo_root: &Path,
    remote_opt: Option<&str>,
    branch_opt: Option<&str>,
) -> Result<PullResult, String> {
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
    crate::core::block_on(client.pull_branch(&remote_entry.url, repo_root, &branch))
        .map_err(|e| format!("pull failed: {}", e))
}

/// CLI handler for `git-med pull [remote] [branch]`.
pub fn pull(args: &[String]) {
    let remote_opt = args.get(2).map(|s| s.as_str());
    let branch_opt = args.get(3).map(|s| s.as_str());

    match pull_in(Path::new("."), remote_opt, branch_opt) {
        Ok(res) => {
            if res.already_up_to_date {
                println!("Already up to date.");
            } else {
                let old_str = res
                    .old_hash
                    .map(|h| {
                        let hex = h.to_hex();
                        hex[..7.min(hex.len())].to_string()
                    })
                    .unwrap_or_else(|| "[new branch]".to_string());
                let new_hex = res.new_hash.to_hex();
                let new_str = &new_hex[..7.min(new_hex.len())];
                println!("Updating {}..{}", old_str, new_str);
                println!("Fast-forward (unpacked {} objects)", res.objects_unpacked);
            }
        }
        Err(err) => {
            eprintln!("git-med pull failed: {}", err);
            std::process::exit(1);
        }
    }
}

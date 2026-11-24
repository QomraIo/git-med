use std::path::{Path, PathBuf};
use crate::remote::GitHttpClient;

/// Deduces a local directory name from a remote repository URL.
pub fn derive_repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    let last_segment = trimmed
        .split('/')
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("repo");
    let clean = last_segment.strip_suffix(".git").unwrap_or(last_segment);
    if clean.is_empty() {
        "repo".to_string()
    } else {
        clean.to_string()
    }
}

/// Clone a remote repository into `target_dir` (or derive directory name from URL).
pub fn clone_in(url: &str, target_dir: Option<&Path>) -> Result<PathBuf, String> {
    let clean_url = url.trim();
    if clean_url.is_empty() {
        return Err("repository url cannot be empty".to_string());
    }

    let normalized_url = if !clean_url.contains("://") {
        format!("https://{}", clean_url)
    } else {
        clean_url.to_string()
    };

    let dest = if let Some(dir) = target_dir {
        dir.to_path_buf()
    } else {
        PathBuf::from(derive_repo_name_from_url(&normalized_url))
    };

    if dest.join(".git-med").is_dir() {
        return Err(format!(
            "destination path '{}' already exists and is a git-med repository",
            dest.display()
        ));
    }

    let client = GitHttpClient::new();
    crate::core::block_on(client.clone_repo(&normalized_url, &dest))
        .map_err(|e| format!("clone failed: {}", e))?;

    Ok(dest)
}

/// CLI handler for `git-med clone <url> [target_dir]`.
pub fn clone(args: &[String]) {
    if args.len() < 3 {
        eprintln!("git-med clone failed: please provide a repository url");
        std::process::exit(1);
    }

    let url = &args[2];
    let target_dir = args.get(3).map(Path::new);

    let display_dest = target_dir
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| derive_repo_name_from_url(url));

    println!("Cloning into '{}'...", display_dest);

    match clone_in(url, target_dir) {
        Ok(dir) => {
            println!("Done. Cloned into {}", dir.display());
        }
        Err(err) => {
            eprintln!("git-med clone failed: {}", err);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_repo_name_from_url() {
        assert_eq!(
            derive_repo_name_from_url("https://github.com/example/my-repo.git"),
            "my-repo"
        );
        assert_eq!(
            derive_repo_name_from_url("https://github.com/example/my-repo"),
            "my-repo"
        );
        assert_eq!(
            derive_repo_name_from_url("http://localhost:8080/foo/bar/"),
            "bar"
        );
        assert_eq!(derive_repo_name_from_url(""), "repo");
    }
}

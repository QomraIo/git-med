use std::fs;
use std::path::Path;

/// A configured remote repository entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub url: String,
    pub fetch: Option<String>,
}

/// Validate a remote name against standard Git rules.
fn validate_remote_name(name: &str) -> Result<(), String> {
    let clean = name.trim();
    if clean.is_empty() {
        return Err("remote name cannot be empty".to_string());
    }
    if clean == "." || clean == ".." {
        return Err(format!("invalid remote name '{}'", name));
    }
    if clean.contains(char::is_whitespace) {
        return Err("remote name cannot contain whitespace".to_string());
    }
    if clean.contains('/') || clean.contains('\\') {
        return Err("remote name cannot contain slashes".to_string());
    }
    for c in [':', '?', '*', '[', '~', '^', '@', '{'] {
        if clean.contains(c) {
            return Err(format!("remote name cannot contain '{}'", c));
        }
    }
    Ok(())
}

/// Validate a remote URL.
fn validate_remote_url(url: &str) -> Result<(), String> {
    let clean = url.trim();
    if clean.is_empty() {
        return Err("remote url cannot be empty".to_string());
    }
    Ok(())
}

/// Parse all `[remote "<name>"]` sections from a Git INI config string.
pub fn parse_remotes_from_ini(content: &str) -> Vec<RemoteEntry> {
    let mut remotes = Vec::new();
    let mut current_remote_name: Option<String> = None;
    let mut current_url: Option<String> = None;
    let mut current_fetch: Option<String> = None;

    let flush = |name_opt: &mut Option<String>,
                     url_opt: &mut Option<String>,
                     fetch_opt: &mut Option<String>,
                     out: &mut Vec<RemoteEntry>| {
        if let Some(name) = name_opt.take() {
            if let Some(url) = url_opt.take() {
                out.push(RemoteEntry {
                    name,
                    url,
                    fetch: fetch_opt.take(),
                });
            }
        }
        *name_opt = None;
        *url_opt = None;
        *fetch_opt = None;
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            flush(
                &mut current_remote_name,
                &mut current_url,
                &mut current_fetch,
                &mut remotes,
            );

            let section = trimmed[1..trimmed.len() - 1].trim();
            if let Some(rest) = section.strip_prefix("remote") {
                let rest = rest.trim();
                if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
                    let remote_name = &rest[1..rest.len() - 1];
                    if !remote_name.is_empty() {
                        current_remote_name = Some(remote_name.to_string());
                    }
                }
            }
            continue;
        }

        if current_remote_name.is_some() {
            if let Some((key, val)) = trimmed.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                let val = val.trim();
                let val = if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                    &val[1..val.len() - 1]
                } else {
                    val
                };

                if key == "url" {
                    current_url = Some(val.to_string());
                } else if key == "fetch" {
                    current_fetch = Some(val.to_string());
                }
            }
        }
    }

    flush(
        &mut current_remote_name,
        &mut current_url,
        &mut current_fetch,
        &mut remotes,
    );
    remotes
}

/// Format a `[remote "<name>"]` INI section string.
pub fn format_remote_ini_section(name: &str, url: &str) -> String {
    format!(
        "[remote \"{}\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/{}/*\n",
        name, url, name
    )
}

/// List all configured remotes for the repository at `repo_root`.
pub fn list_remotes_in(repo_root: &Path) -> Result<Vec<RemoteEntry>, String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    let config_path = git_med_dir.join("config");
    if !config_path.is_file() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("failed to read config {}: {}", config_path.display(), e))?;

    Ok(parse_remotes_from_ini(&content))
}

/// Find a specific remote by name for the repository at `repo_root`.
pub fn get_remote_in(repo_root: &Path, name: &str) -> Result<Option<RemoteEntry>, String> {
    let remotes = list_remotes_in(repo_root)?;
    Ok(remotes.into_iter().find(|r| r.name == name))
}

/// Add a new remote named `name` with URL `url` to the repository at `repo_root`.
/// Also creates `.git-med/refs/remotes/<name>` on disk.
pub fn remote_add_in(repo_root: &Path, name: &str, url: &str) -> Result<(), String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    validate_remote_name(name)?;
    validate_remote_url(url)?;

    let clean_name = name.trim();
    let clean_url = url.trim();

    let config_path = git_med_dir.join("config");
    let mut content = if config_path.is_file() {
        fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config {}: {}", config_path.display(), e))?
    } else {
        String::new()
    };

    let existing_remotes = parse_remotes_from_ini(&content);
    if existing_remotes.iter().any(|r| r.name == clean_name) {
        return Err(format!("remote '{}' already exists", clean_name));
    }

    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format_remote_ini_section(clean_name, clean_url));

    fs::write(&config_path, content)
        .map_err(|e| format!("failed to write config {}: {}", config_path.display(), e))?;

    // Create .git-med/refs/remotes/<name> directory
    let remote_refs_dir = git_med_dir.join("refs/remotes").join(clean_name);
    fs::create_dir_all(&remote_refs_dir).map_err(|e| {
        format!(
            "failed to create remote ref dir {}: {}",
            remote_refs_dir.display(),
            e
        )
    })?;

    Ok(())
}

/// CLI handler for `git-med remote`.
pub fn remote(args: &[String]) {
    if args.len() == 2 {
        // `git-med remote` -> list all remote names
        match list_remotes_in(Path::new(".")) {
            Ok(remotes) => {
                for r in remotes {
                    println!("{}", r.name);
                }
            }
            Err(err) => {
                eprintln!("git-med remote failed: {}", err);
                std::process::exit(1);
            }
        }
        return;
    }

    let subcmd = args[2].as_str();
    match subcmd {
        "-v" | "--verbose" => match list_remotes_in(Path::new(".")) {
            Ok(remotes) => {
                for r in remotes {
                    println!("{}\t{} (fetch)", r.name, r.url);
                    println!("{}\t{} (push)", r.name, r.url);
                }
            }
            Err(err) => {
                eprintln!("git-med remote failed: {}", err);
                std::process::exit(1);
            }
        },
        "add" => {
            if args.len() < 5 {
                eprintln!("git-med remote add failed: please provide a remote name and url");
                std::process::exit(1);
            }
            let name = &args[3];
            let url = &args[4];
            if let Err(err) = remote_add_in(Path::new("."), name, url) {
                eprintln!("git-med remote add failed: {}", err);
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("git-med remote failed: unknown subcommand or argument '{}'", other);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::init::init_at;
    use std::path::PathBuf;

    struct TempRepo {
        path: PathBuf,
    }

    impl TempRepo {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "git_med_remote_test_{}_{}_{}",
                name,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create repo dir");
            init_at(&path);
            Self { path }
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_remote_add_success() {
        let repo = TempRepo::new("add_success");
        let remote_name = "upstream";
        let remote_url = "https://github.com/example/repo.git";

        remote_add_in(&repo.path, remote_name, remote_url).expect("remote_add_in should succeed");

        // Verify config file was written
        let config_path = repo.path.join(".git-med/config");
        assert!(config_path.is_file());
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("[remote \"upstream\"]"));
        assert!(content.contains("url = https://github.com/example/repo.git"));
        assert!(content.contains("fetch = +refs/heads/*:refs/remotes/upstream/*"));

        // Verify directory was created
        let remote_dir = repo.path.join(".git-med/refs/remotes/upstream");
        assert!(remote_dir.is_dir());

        // Verify list_remotes_in and get_remote_in
        let remotes = list_remotes_in(&repo.path).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "upstream");
        assert_eq!(remotes[0].url, remote_url);
        assert_eq!(
            remotes[0].fetch.as_deref(),
            Some("+refs/heads/*:refs/remotes/upstream/*")
        );

        let retrieved = get_remote_in(&repo.path, "upstream").unwrap();
        assert_eq!(retrieved, Some(remotes[0].clone()));
    }

    #[test]
    fn test_remote_add_duplicate_fails() {
        let repo = TempRepo::new("add_duplicate");
        remote_add_in(&repo.path, "origin", "https://github.com/origin/repo.git").unwrap();

        let err = remote_add_in(&repo.path, "origin", "https://github.com/another/repo.git")
            .unwrap_err();
        assert!(err.contains("already exists"));
    }

    #[test]
    fn test_remote_add_multiple() {
        let repo = TempRepo::new("add_multiple");
        remote_add_in(&repo.path, "origin", "https://github.com/origin/repo.git").unwrap();
        remote_add_in(&repo.path, "upstream", "https://github.com/upstream/repo.git").unwrap();

        let remotes = list_remotes_in(&repo.path).unwrap();
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[1].name, "upstream");

        assert!(repo.path.join(".git-med/refs/remotes/origin").is_dir());
        assert!(repo.path.join(".git-med/refs/remotes/upstream").is_dir());
    }

    #[test]
    fn test_remote_add_validation() {
        let repo = TempRepo::new("validation");

        assert!(remote_add_in(&repo.path, "", "https://example.com").is_err());
        assert!(remote_add_in(&repo.path, "bad name", "https://example.com").is_err());
        assert!(remote_add_in(&repo.path, "bad/name", "https://example.com").is_err());
        assert!(remote_add_in(&repo.path, "bad:name", "https://example.com").is_err());
        assert!(remote_add_in(&repo.path, ".", "https://example.com").is_err());
        assert!(remote_add_in(&repo.path, "..", "https://example.com").is_err());
        assert!(remote_add_in(&repo.path, "origin", "").is_err());
        assert!(remote_add_in(&repo.path, "origin", "   ").is_err());
    }

    #[test]
    fn test_remote_not_in_repo() {
        let non_repo = std::env::temp_dir().join(format!(
            "git_med_non_repo_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&non_repo).unwrap();

        let err = remote_add_in(&non_repo, "origin", "https://example.com").unwrap_err();
        assert!(err.contains("not a git-med repository"));

        let _ = fs::remove_dir_all(&non_repo);
    }

    #[test]
    fn test_remote_list_empty() {
        let repo = TempRepo::new("list_empty");
        let remotes = list_remotes_in(&repo.path).unwrap();
        assert!(remotes.is_empty());
        assert_eq!(get_remote_in(&repo.path, "origin").unwrap(), None);
    }

    #[test]
    fn test_remote_ini_preserves_other_sections() {
        let repo = TempRepo::new("preserve_sections");
        let config_path = repo.path.join(".git-med/config");
        fs::write(&config_path, "[core]\n\tbare = false\n").unwrap();

        remote_add_in(&repo.path, "origin", "https://github.com/example/repo.git").unwrap();

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("[core]\n\tbare = false\n"));
        assert!(content.contains("[remote \"origin\"]"));
    }
}

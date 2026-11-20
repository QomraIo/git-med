use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::branch::load_ref_in;
use crate::core::commit::Commit;
use crate::core::merge::{find_merge_base, merge_node_3way, read_view_name, ConflictBreaker};
use crate::core::remote::get_remote_in;
use crate::core::tree::{ChildrenType, Node, Tree};
use crate::files::commit::load_tree_from_commit_in;
use crate::files::hash::Hash;
use crate::files::types::ObjectType;
use crate::files::utils::make_object_from_bytes_in;
use crate::remote::client::GitHttpClient;
use crate::remote::traversal::collect_objects_to_pack;
use crate::views::get_view_in;

/// Representation of a parsed repository branch specifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchSource {
    SelfRepo { branch: String },
    LocalRepo { path: PathBuf, branch: String },
    RemoteRepo { url: String, branch: String },
}

/// Parse a raw repository-branch specifier string into a `BranchSource`.
///
/// Rules:
/// - `<repo_spec>::<branch>`: split by `::`. If branch is empty or omitted, defaults to "main".
/// - A specifier without `::` defaults to branch "main".
/// - If repo is "self", represents the active current repository (`self::<branch>`).
/// - If repo starts with "http://" or "https://", represents a remote repository URL.
/// - If repo matches a configured remote name in `.git-med/config`, uses the remote's URL.
/// - If repo points to an existing directory containing `.git-med`, represents a local repository.
pub fn parse_spec(repo_root: &Path, raw: &str) -> Result<BranchSource, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("branch specification cannot be empty".to_string());
    }

    let (repo_part, branch_part) = match trimmed.split_once("::") {
        Some((r, b)) => {
            let clean_b = b.trim();
            let b_name = if clean_b.is_empty() { "main" } else { clean_b };
            (r.trim(), b_name.to_string())
        }
        None => (trimmed, "main".to_string()),
    };

    if repo_part.is_empty() {
        return Err(format!("missing repository specifier in '{}'", raw));
    }

    // 1. self::<branch> or self
    if repo_part == "self" {
        return Ok(BranchSource::SelfRepo { branch: branch_part });
    }

    // 2. Remote URL: http:// or https://
    if repo_part.starts_with("http://") || repo_part.starts_with("https://") {
        return Ok(BranchSource::RemoteRepo {
            url: repo_part.to_string(),
            branch: branch_part,
        });
    }

    // 3. Configured remote name in .git-med/config (e.g. origin)
    if let Ok(Some(remote_entry)) = get_remote_in(repo_root, repo_part) {
        return Ok(BranchSource::RemoteRepo {
            url: remote_entry.url,
            branch: branch_part,
        });
    }

    // 4. Local repository path (relative or absolute)
    let local_path = PathBuf::from(repo_part);
    let resolved_path = if local_path.is_absolute() {
        local_path.clone()
    } else {
        repo_root.join(&local_path)
    };

    if resolved_path.join(".git-med").is_dir() {
        return Ok(BranchSource::LocalRepo {
            path: local_path,
            branch: branch_part,
        });
    }

    // If it looks like a URL with another scheme or fallback
    if repo_part.contains("://") {
        return Ok(BranchSource::RemoteRepo {
            url: repo_part.to_string(),
            branch: branch_part,
        });
    }

    Err(format!(
        "cannot locate repository '{}': not a valid URL, configured remote, or local git-med repository",
        repo_part
    ))
}

/// Copy all reachable content-addressed objects and link LFS files from a local repository.
///
/// For LFS objects:
/// - Links or references the raw files directly from the source repository (zero-copy),
///   updating the destination's `.git-med/lfs/objects/` with symlinks or direct references.
pub fn copy_objects_from_local_repo(
    src_repo: &Path,
    dest_repo: &Path,
    start_commit: &Hash,
) -> Result<(), String> {
    let objects = collect_objects_to_pack(src_repo, start_commit, &[])
        .map_err(|e| format!("failed to collect objects from local repo {}: {}", src_repo.display(), e))?;

    for (hash, obj_type, body) in &objects {
        make_object_from_bytes_in(body, *hash, *obj_type, dest_repo)
            .map_err(|e| format!("failed to copy object {} to {}: {}", hash.to_hex(), dest_repo.display(), e))?;

        if *obj_type == ObjectType::Lfs {
            if let Ok(pointer) = crate::files::lfs::LfsPointer::parse(body) {
                let src_raw_path = if pointer.local_path != "self" && Path::new(&pointer.local_path).is_file() {
                    PathBuf::from(&pointer.local_path)
                } else {
                    crate::files::lfs::lfs_raw_object_path(src_repo, &pointer.hash)
                };

                if src_raw_path.is_file() {
                    let canonical_src = fs::canonicalize(&src_raw_path).unwrap_or(src_raw_path);
                    let dest_raw_path = crate::files::lfs::lfs_raw_object_path(dest_repo, &pointer.hash);
                    if !dest_raw_path.exists() {
                        if let Some(parent) = dest_raw_path.parent() {
                            fs::create_dir_all(parent)
                                .map_err(|e| format!("failed to create lfs directory: {}", e))?;
                        }
                        #[cfg(unix)]
                        {
                            if std::os::unix::fs::symlink(&canonical_src, &dest_raw_path).is_err() {
                                let _ = fs::copy(&canonical_src, &dest_raw_path);
                            }
                        }
                        #[cfg(not(unix))]
                        {
                            let _ = fs::copy(&canonical_src, &dest_raw_path);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Parse command-line arguments for `git-med mix`.
///
/// Syntax:
/// `git-med mix <result_branch> <spec1> [<spec2>...] [--left|--right] [-m <message>]`
pub fn parse_mix_args(
    args: &[String],
) -> Result<(String, Vec<String>, ConflictBreaker, Option<String>), String> {
    if args.len() < 3 {
        return Err("usage: git-med mix <result_branch> <spec1> [<spec2>...] [--left|--right] [-m <message>]".to_string());
    }

    let mut result_branch = String::new();
    let mut specs = Vec::new();
    let mut breaker_left = false;
    let mut breaker_right = false;
    let mut custom_message = None;

    let mut i = 2;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--left" || arg == "–left" || arg == "—left" || arg == "-left" {
            breaker_left = true;
            i += 1;
        } else if arg == "--right" || arg == "–right" || arg == "—right" || arg == "-right" {
            breaker_right = true;
            i += 1;
        } else if arg == "-m" || arg == "--message" {
            if i + 1 < args.len() {
                custom_message = Some(args[i + 1].clone());
                i += 2;
            } else {
                return Err("-m requires a commit message".to_string());
            }
        } else if !arg.starts_with('-') && !arg.starts_with('–') {
            if result_branch.is_empty() {
                result_branch = arg.clone();
            } else {
                specs.push(arg.clone());
            }
            i += 1;
        } else {
            return Err(format!("unknown option '{}'", arg));
        }
    }

    if breaker_left && breaker_right {
        return Err("cannot specify both --left and --right".to_string());
    }

    let breaker = if breaker_left {
        ConflictBreaker::Left
    } else if breaker_right {
        ConflictBreaker::Right
    } else {
        ConflictBreaker::None
    };

    if result_branch.is_empty() {
        return Err("please provide a result branch name".to_string());
    }

    if specs.is_empty() {
        return Err("please provide at least one branch specification to mix".to_string());
    }

    Ok((result_branch, specs, breaker, custom_message))
}

/// Execute the mix command inside `repo_root`:
/// 1. Ingests all specified branches (downloading from remotes, copying objects and linking LFS from local repos, or resolving from self).
/// 2. If `result_branch` already exists, merges all incoming branches into its existing commit head.
///    Otherwise, creates `result_branch` fresh by merging all incoming branches together.
/// 3. Generates a multi-parent commit object.
/// 4. Updates `result_branch` reference and switches `HEAD` to it.
/// 5. Materializes the combined working tree using the repository's active view.
pub fn mix_in(
    repo_root: &Path,
    result_branch_raw: &str,
    spec_strs: &[String],
    breaker: ConflictBreaker,
    custom_message: Option<String>,
) -> Result<Hash, String> {
    let git_med_dir = repo_root.join(".git-med");
    if !git_med_dir.is_dir() {
        return Err(format!("not a git-med repository: {}", repo_root.display()));
    }

    let clean_name = result_branch_raw.trim();
    if clean_name.is_empty() {
        return Err("result branch name cannot be empty".to_string());
    }

    let result_branch = if let Some(stripped) = clean_name.strip_prefix("refs/heads/") {
        stripped
    } else {
        clean_name
    };

    if spec_strs.is_empty() {
        return Err("please provide at least one branch specification to mix".to_string());
    }

    // 1. Resolve and ingest each incoming branch
    let mut incoming_commits: Vec<(String, Hash)> = Vec::new();

    for s in spec_strs {
        let parsed = parse_spec(repo_root, s)?;
        match parsed {
            BranchSource::SelfRepo { branch } => {
                let hash = load_ref_in(repo_root, &branch)?
                    .ok_or_else(|| format!("branch '{}' not found in self repository", branch))?;
                incoming_commits.push((format!("self::{}", branch), hash));
            }
            BranchSource::LocalRepo { path, branch } => {
                let full_path = if path.is_absolute() {
                    path.clone()
                } else {
                    repo_root.join(&path)
                };

                if !full_path.join(".git-med").is_dir() {
                    return Err(format!("not a git-med repository: {}", path.display()));
                }

                let hash = load_ref_in(&full_path, &branch)?
                    .ok_or_else(|| format!("branch '{}' not found in local repository '{}'", branch, path.display()))?;

                copy_objects_from_local_repo(&full_path, repo_root, &hash)?;
                incoming_commits.push((format!("{}::{}", path.display(), branch), hash));
            }
            BranchSource::RemoteRepo { url, branch } => {
                let client = GitHttpClient::new();
                let hash = crate::core::block_on(client.fetch_branch(&url, repo_root, &branch))
                    .map_err(|e| format!("failed to fetch remote branch '{}::{}': {}", url, branch, e))?;
                incoming_commits.push((format!("{}::{}", url, branch), hash));
            }
        }
    }

    // Deduplicate incoming commits while preserving specification order
    let mut seen_hashes: HashSet<Hash> = HashSet::new();
    let mut unique_incoming: Vec<(String, Hash)> = Vec::new();
    for (label, hash) in incoming_commits {
        if seen_hashes.insert(hash) {
            unique_incoming.push((label, hash));
        }
    }

    if unique_incoming.is_empty() {
        return Err("no valid commits found to mix".to_string());
    }

    // 2. Determine base tree and parent list
    let existing_branch_hash = load_ref_in(repo_root, result_branch)?;

    let (evolving_tree, parents) = if let Some(base_hash) = existing_branch_hash {
        // Result branch exists: use its current tree as base, and merge incoming branches into it
        let mut tree = load_tree_from_commit_in(repo_root, &base_hash)?;
        tree.hash_in(repo_root)?;

        let mut parents = vec![base_hash.to_hex()];

        for (_label, inc_hash) in &unique_incoming {
            if *inc_hash == base_hash {
                continue;
            }
            parents.push(inc_hash.to_hex());

            let base_commit_opt = find_merge_base(repo_root, &base_hash, inc_hash)?;
            let base_tree = if let Some(bh) = base_commit_opt {
                Some(load_tree_from_commit_in(repo_root, &bh)?)
            } else {
                None
            };
            let inc_tree = load_tree_from_commit_in(repo_root, inc_hash)?;

            let base_root = base_tree.as_ref().map(|t| &t.root);
            let left_root = Some(&tree.root);
            let right_root = Some(&inc_tree.root);

            let merged_root = merge_node_3way(base_root, left_root, right_root, breaker, "")?
                .unwrap_or_else(|| Node::new("", ChildrenType::Attributes));

            tree = Tree::from_root(merged_root);
            tree.hash_in(repo_root)?;
        }

        (tree, parents)
    } else {
        // Result branch is new: initialize with first incoming branch, then merge remaining
        let (_first_label, first_hash) = &unique_incoming[0];
        let mut tree = load_tree_from_commit_in(repo_root, first_hash)?;
        tree.hash_in(repo_root)?;

        let mut parents = vec![first_hash.to_hex()];

        for (_label, inc_hash) in &unique_incoming[1..] {
            parents.push(inc_hash.to_hex());

            let base_commit_opt = find_merge_base(repo_root, first_hash, inc_hash)?;
            let base_tree = if let Some(bh) = base_commit_opt {
                Some(load_tree_from_commit_in(repo_root, &bh)?)
            } else {
                None
            };
            let inc_tree = load_tree_from_commit_in(repo_root, inc_hash)?;

            let base_root = base_tree.as_ref().map(|t| &t.root);
            let left_root = Some(&tree.root);
            let right_root = Some(&inc_tree.root);

            let merged_root = merge_node_3way(base_root, left_root, right_root, breaker, "")?
                .unwrap_or_else(|| Node::new("", ChildrenType::Attributes));

            tree = Tree::from_root(merged_root);
            tree.hash_in(repo_root)?;
        }

        (tree, parents)
    };

    // 3. Create commit object
    let final_tree_hash = evolving_tree.hash_in(repo_root)?;

    let message = if let Some(m) = custom_message {
        m
    } else {
        let labels: Vec<&str> = unique_incoming.iter().map(|(l, _)| l.as_str()).collect();
        format!("Mix branches {} into {}", labels.join(", "), result_branch)
    };

    let commit = Commit::new_with_parents(final_tree_hash, parents, message);
    let body = crate::core::commit::format_commit_body_multi(&commit.tree, &commit.parents, &commit.message);
    let commit_hash = crate::files::utils::hash_bytes(&body, ObjectType::Commit);
    make_object_from_bytes_in(&body, commit_hash, ObjectType::Commit, repo_root)
        .map_err(|e| format!("failed to write commit object: {}", e))?;

    // 4. Save index
    let index_path = git_med_dir.join("index");
    evolving_tree
        .save(&index_path)
        .map_err(|e| format!("failed to save index {}: {}", index_path.display(), e))?;

    // 5. Update result_branch ref
    let branch_ref_path = git_med_dir.join("refs/heads").join(result_branch);
    if let Some(parent) = branch_ref_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create refs dir: {}", e))?;
    }
    fs::write(&branch_ref_path, format!("{}\n", commit_hash.to_hex()))
        .map_err(|e| format!("failed to write branch ref {}: {}", branch_ref_path.display(), e))?;

    // 6. Switch HEAD to result_branch
    let head_path = git_med_dir.join("HEAD");
    fs::write(&head_path, format!("ref: refs/heads/{}\n", result_branch))
        .map_err(|e| format!("failed to write HEAD: {}", e))?;

    // 7. Materialize working tree using active view
    let view_name = read_view_name(&git_med_dir.join("VIEW"))?;
    let view = get_view_in(&view_name, repo_root)
        .ok_or_else(|| format!("unsupported view '{}'", view_name))?;
    view.write(&evolving_tree.root)
        .map_err(|e| format!("view write failed: {}", e))?;

    Ok(commit_hash)
}

/// CLI handler for `git-med mix <result_branch> <spec1> [<spec2>...]`.
pub fn mix(args: &[String]) {
    let (result_branch, specs, breaker, custom_message) = match parse_mix_args(args) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("git-med mix failed: {}", err);
            std::process::exit(1);
        }
    };

    match mix_in(Path::new("."), &result_branch, &specs, breaker, custom_message) {
        Ok(commit_hash) => {
            println!("Mixed into branch '{}': {}", result_branch, commit_hash.to_hex());
        }
        Err(err) => {
            eprintln!("git-med mix failed: {}", err);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::add::add_in;
    use crate::core::branch::branch_in;
    use crate::core::commit::commit_with_message_in;
    use crate::core::init::init_at;
    use crate::tester::generate::sample::qomra;

    struct TempMixRepo {
        pub path: PathBuf,
    }

    impl TempMixRepo {
        fn new(name: &str) -> Self {
            let unique = format!("git_med_mix_test_{}_{}", name, std::process::id());
            let path = std::env::temp_dir().join(unique);
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            fs::create_dir_all(path.join("data")).unwrap();
            init_at(&path);
            Self { path }
        }
    }

    impl Drop for TempMixRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_parse_mix_args() {
        let args = vec![
            "git-med".to_string(),
            "mix".to_string(),
            "target".to_string(),
            "../repo1::feat".to_string(),
            "self::main".to_string(),
            "--right".to_string(),
            "-m".to_string(),
            "my mix message".to_string(),
        ];

        let (result_branch, specs, breaker, msg) = parse_mix_args(&args).unwrap();
        assert_eq!(result_branch, "target");
        assert_eq!(specs, vec!["../repo1::feat", "self::main"]);
        assert_eq!(breaker, ConflictBreaker::Right);
        assert_eq!(msg, Some("my mix message".to_string()));
    }

    #[test]
    fn test_parse_spec_rules() {
        let repo = TempMixRepo::new("parse_spec");
        let r_path = &repo.path;

        // self with branch
        let s1 = parse_spec(r_path, "self::experiment").unwrap();
        assert_eq!(s1, BranchSource::SelfRepo { branch: "experiment".to_string() });

        // self without branch -> main
        let s2 = parse_spec(r_path, "self").unwrap();
        assert_eq!(s2, BranchSource::SelfRepo { branch: "main".to_string() });

        // Remote URL with branch
        let s3 = parse_spec(r_path, "http://localhost:8080/data/repo.git::cohort-a").unwrap();
        assert_eq!(s3, BranchSource::RemoteRepo {
            url: "http://localhost:8080/data/repo.git".to_string(),
            branch: "cohort-a".to_string(),
        });

        // Remote URL without branch -> main
        let s4 = parse_spec(r_path, "https://example.com/repo").unwrap();
        assert_eq!(s4, BranchSource::RemoteRepo {
            url: "https://example.com/repo".to_string(),
            branch: "main".to_string(),
        });

        // Local repo with branch
        let other_repo = TempMixRepo::new("other");
        let s5 = parse_spec(r_path, &format!("{}::feature-b", other_repo.path.display())).unwrap();
        assert_eq!(s5, BranchSource::LocalRepo {
            path: other_repo.path.clone(),
            branch: "feature-b".to_string(),
        });

        // Local repo without branch -> main
        let s6 = parse_spec(r_path, &other_repo.path.display().to_string()).unwrap();
        assert_eq!(s6, BranchSource::LocalRepo {
            path: other_repo.path.clone(),
            branch: "main".to_string(),
        });
    }

    #[test]
    fn test_mix_local_and_self_branches_into_new_branch() {
        let repo_main = TempMixRepo::new("mix_main");
        let repo_external = TempMixRepo::new("mix_ext");

        // Set up repo_main: self::main with dataset A
        qomra(&repo_main.path, &["data/task_a"], 1, 1, 1, 1);
        add_in(&repo_main.path).unwrap();
        commit_with_message_in(&repo_main.path, "Commit Main Dataset A").unwrap();

        // Also create a second branch in repo_main: self::b1
        branch_in(&repo_main.path, "b1").unwrap();
        fs::write(repo_main.path.join("data/extra_main.txt"), b"from self b1").unwrap();
        add_in(&repo_main.path).unwrap();
        commit_with_message_in(&repo_main.path, "Commit Self B1").unwrap();

        // Switch back to main in repo_main
        branch_in(&repo_main.path, "main").unwrap();

        // Set up repo_external: branch feat_ext with dataset B
        qomra(&repo_external.path, &["data/task_b"], 1, 1, 1, 1);
        fs::write(repo_external.path.join("data/external_unique.txt"), b"from external repo").unwrap();
        add_in(&repo_external.path).unwrap();
        commit_with_message_in(&repo_external.path, "Commit Ext Base").unwrap();

        branch_in(&repo_external.path, "feat_ext").unwrap();
        fs::write(repo_external.path.join("data/feat_ext.txt"), b"from feat_ext").unwrap();
        add_in(&repo_external.path).unwrap();
        commit_with_message_in(&repo_external.path, "Commit Ext Feat").unwrap();

        // Mix: create "result_mix" from self::b1 and repo_external::feat_ext
        let ext_spec = format!("{}::feat_ext", repo_external.path.display());
        let specs = vec!["self::b1".to_string(), ext_spec];

        let commit_hash = mix_in(
            &repo_main.path,
            "result_mix",
            &specs,
            ConflictBreaker::None,
            Some("Mixed result_mix commit".to_string()),
        )
        .expect("mix_in should succeed");

        // Verify HEAD is at result_mix
        let head_content = fs::read_to_string(repo_main.path.join(".git-med/HEAD")).unwrap();
        assert_eq!(head_content.trim(), "ref: refs/heads/result_mix");

        // Verify result_mix ref matches commit_hash
        let ref_hash = load_ref_in(&repo_main.path, "result_mix").unwrap().unwrap();
        assert_eq!(ref_hash, commit_hash);

        // Verify files from both branches are materialized
        assert!(repo_main.path.join("data/extra_main.txt").is_file());
        assert!(repo_main.path.join("data/external_unique.txt").is_file());
        assert!(repo_main.path.join("data/feat_ext.txt").is_file());

        // Verify parents in commit object
        let parents = crate::core::merge::load_commit_parents(&repo_main.path, &commit_hash).unwrap();
        assert_eq!(parents.len(), 2);
    }

    #[test]
    fn test_mix_into_existing_branch() {
        let repo_main = TempMixRepo::new("mix_existing");
        let repo_external = TempMixRepo::new("mix_ext2");

        // repo_main on main
        fs::write(repo_main.path.join("data/base.txt"), b"base").unwrap();
        add_in(&repo_main.path).unwrap();
        commit_with_message_in(&repo_main.path, "Initial Base").unwrap();

        // Create target branch "cohort" on repo_main
        branch_in(&repo_main.path, "cohort").unwrap();
        fs::write(repo_main.path.join("data/cohort_orig.txt"), b"cohort original").unwrap();
        add_in(&repo_main.path).unwrap();
        commit_with_message_in(&repo_main.path, "Cohort Base").unwrap();
        let cohort_orig_hash = load_ref_in(&repo_main.path, "cohort").unwrap().unwrap();

        // External repo with branch "feat"
        fs::write(repo_external.path.join("data/ext.txt"), b"from ext").unwrap();
        add_in(&repo_external.path).unwrap();
        commit_with_message_in(&repo_external.path, "Ext Base").unwrap();

        branch_in(&repo_external.path, "feat").unwrap();
        fs::write(repo_external.path.join("data/ext_feat.txt"), b"from ext feat").unwrap();
        add_in(&repo_external.path).unwrap();
        commit_with_message_in(&repo_external.path, "Ext Feat").unwrap();

        // Switch repo_main to main
        branch_in(&repo_main.path, "main").unwrap();

        // Mix into existing "cohort"
        let ext_spec = format!("{}::feat", repo_external.path.display());
        let specs = vec![ext_spec];

        let commit_hash = mix_in(
            &repo_main.path,
            "cohort",
            &specs,
            ConflictBreaker::None,
            None,
        )
        .expect("mix into existing branch should succeed");

        // HEAD should be at cohort
        let head_content = fs::read_to_string(repo_main.path.join(".git-med/HEAD")).unwrap();
        assert_eq!(head_content.trim(), "ref: refs/heads/cohort");

        // cohort ref updated
        let ref_hash = load_ref_in(&repo_main.path, "cohort").unwrap().unwrap();
        assert_eq!(ref_hash, commit_hash);

        // Parents: cohort_orig_hash should be parent #1
        let parents = crate::core::merge::load_commit_parents(&repo_main.path, &commit_hash).unwrap();
        assert_eq!(parents[0], cohort_orig_hash);
        assert_eq!(parents.len(), 2);

        // Both cohort_orig and ext_feat files exist
        assert!(repo_main.path.join("data/cohort_orig.txt").is_file());
        assert!(repo_main.path.join("data/ext_feat.txt").is_file());
    }

    #[test]
    fn test_mix_conflict_and_breakers() {
        let repo_main = TempMixRepo::new("mix_conflict");
        let repo_ext = TempMixRepo::new("mix_ext_c");

        // Main has conflict.txt with "version A"
        fs::write(repo_main.path.join("data/conflict.txt"), b"version A").unwrap();
        add_in(&repo_main.path).unwrap();
        commit_with_message_in(&repo_main.path, "Commit A").unwrap();

        // External has conflict.txt with "version B"
        fs::write(repo_ext.path.join("data/conflict.txt"), b"version B").unwrap();
        add_in(&repo_ext.path).unwrap();
        commit_with_message_in(&repo_ext.path, "Commit B").unwrap();

        let ext_spec = format!("{}::main", repo_ext.path.display());

        // Without breaker: must fail with conflict error
        let err = mix_in(
            &repo_main.path,
            "res",
            &["self::main".to_string(), ext_spec.clone()],
            ConflictBreaker::None,
            None,
        )
        .expect_err("mix should fail on conflict without breaker");
        assert!(err.contains("conflict"));

        // With --left: keeps version A
        mix_in(
            &repo_main.path,
            "res_left",
            &["self::main".to_string(), ext_spec.clone()],
            ConflictBreaker::Left,
            None,
        )
        .expect("mix with --left should succeed");
        assert_eq!(
            fs::read_to_string(repo_main.path.join("data/conflict.txt")).unwrap(),
            "version A"
        );

        // With --right: takes version B
        mix_in(
            &repo_main.path,
            "res_right",
            &["self::main".to_string(), ext_spec],
            ConflictBreaker::Right,
            None,
        )
        .expect("mix with --right should succeed");
        assert_eq!(
            fs::read_to_string(repo_main.path.join("data/conflict.txt")).unwrap(),
            "version B"
        );
    }
}

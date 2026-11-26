use std::path::Path;

use crate::remote::discovery::{
    DiscoveryHeader, build_v2_ls_refs_request, parse_initial_advertisement,
    parse_v2_ls_refs_response,
};
use crate::remote::negotiation;
use crate::remote::packfile;
use crate::remote::pkt_line;
use crate::remote::push;
use crate::remote::sideband;
use crate::remote::traversal;
use crate::remote::types::{FetchResult, PullResult, PushResult, RefAdvertisement, RemoteError};

/// HTTP transport client for interacting with remote Git servers.
#[derive(Debug, Clone)]
pub struct GitHttpClient {
    client: reqwest::Client,
    token: Option<String>,
}

impl Default for GitHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHttpClient {
    /// Create a new `GitHttpClient` using the default TLS configuration.
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, token: None }
    }

    /// Create a `GitHttpClient` with a custom configured `reqwest::Client`.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client, token: None }
    }

    /// Create a `GitHttpClient` with an explicit bearer token.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Create a `GitHttpClient` initialized with stored credentials for `repo_url`.
    pub fn for_url(repo_url: &str) -> Self {
        let token = crate::core::auth::get_token_for_url(repo_url);
        Self {
            client: reqwest::Client::builder()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            token,
        }
    }

    fn resolve_token(&self, url: &str) -> Option<String> {
        self.token.clone().or_else(|| crate::core::auth::get_token_for_url(url))
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder, url: &str) -> reqwest::RequestBuilder {
        if let Some(token) = self.resolve_token(url) {
            req.header("Authorization", format!("Bearer {}", token))
        } else {
            req
        }
    }

    fn check_auth_status(status: reqwest::StatusCode, url: &str, msg: String) -> RemoteError {
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            RemoteError::Protocol(format!(
                "Authentication failed (HTTP {}). Run 'git-med login --server {}' to authenticate. Details: {}",
                status,
                crate::core::auth::extract_server_origin(url),
                msg
            ))
        } else {
            RemoteError::HttpStatus {
                status: status.as_u16(),
                message: msg,
            }
        }
    }

    /// Discover references from a remote repository via Git Smart HTTP.
    ///
    /// 1. Sends `GET $REPO_URL/info/refs?service=git-upload-pack` requesting Protocol v2.
    /// 2. If the server supports Protocol v2, executes a `POST $REPO_URL/git-upload-pack`
    ///    with `command=ls-refs` to list all references, symrefs, and peeled tags.
    /// 3. If the server falls back to Protocol v1, parses references from the advertisement.
    pub async fn discover_refs(&self, repo_url: &str) -> Result<RefAdvertisement, RemoteError> {
        let trimmed_url = repo_url.trim_end_matches('/');
        let info_refs_url = format!("{}/info/refs?service=git-upload-pack", trimmed_url);

        let req = self
            .client
            .get(&info_refs_url)
            .header("Git-Protocol", "version=2")
            .header("Accept", "application/x-git-upload-pack-advertisement")
            .header("User-Agent", "git-med/0.1.0");

        let response = self.apply_auth(req, repo_url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let msg = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(Self::check_auth_status(status, repo_url, msg));
        }

        let bytes = response.bytes().await?;
        let packets = pkt_line::decode_pkt_lines(&bytes)
            .map_err(|e| RemoteError::PktLine(e.to_string()))?;

        match parse_initial_advertisement(&packets)? {
            DiscoveryHeader::Version2(caps) => {
                let upload_pack_url = format!("{}/git-upload-pack", trimmed_url);
                let ls_refs_payload = build_v2_ls_refs_request()?;

                let post_req = self
                    .client
                    .post(&upload_pack_url)
                    .header("Git-Protocol", "version=2")
                    .header("Content-Type", "application/x-git-upload-pack-request")
                    .header("Accept", "application/x-git-upload-pack-result")
                    .header("User-Agent", "git-med/0.1.0")
                    .body(ls_refs_payload);

                let post_response = self.apply_auth(post_req, repo_url).send().await?;

                let post_status = post_response.status();
                if !post_status.is_success() {
                    let msg = post_response
                        .text()
                        .await
                        .unwrap_or_else(|_| "unknown error".into());
                    return Err(Self::check_auth_status(post_status, repo_url, msg));
                }

                let post_bytes = post_response.bytes().await?;
                let post_packets = pkt_line::decode_pkt_lines(&post_bytes)
                    .map_err(|e| RemoteError::PktLine(e.to_string()))?;

                parse_v2_ls_refs_response(caps, &post_packets)
            }
            DiscoveryHeader::Version1(advertisement) => Ok(advertisement),
        }
    }

    /// Internal helper to negotiate and download a packfile, unpack loose objects into
    /// `<repo_root>/.git-med/objects/`, and update remote tracking references.
    async fn fetch_packfile_and_unpack_internal(
        &self,
        repo_url: &str,
        repo_root: &Path,
        advertisement: &RefAdvertisement,
        wants: &[crate::files::hash::Hash],
    ) -> Result<usize, RemoteError> {
        if wants.is_empty() {
            return Ok(0);
        }

        let trimmed_url = repo_url.trim_end_matches('/');

        // Build negotiation payload without requesting delta compression
        let payload = if advertisement.protocol_version == 2 {
            negotiation::build_v2_fetch_request(wants)?
        } else {
            negotiation::build_v1_fetch_request(wants)?
        };

        // POST /git-upload-pack
        let upload_pack_url = format!("{}/git-upload-pack", trimmed_url);
        let mut request = self
            .client
            .post(&upload_pack_url)
            .header("Content-Type", "application/x-git-upload-pack-request")
            .header("Accept", "application/x-git-upload-pack-result")
            .header("User-Agent", "git-med/0.1.0")
            .body(payload);

        if advertisement.protocol_version == 2 {
            request = request.header("Git-Protocol", "version=2");
        }

        request = self.apply_auth(request, repo_url);

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let msg = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(Self::check_auth_status(status, repo_url, msg));
        }

        // Demultiplex side-band 64k response and extract packfile bytes
        let body_bytes = response.bytes().await?;
        let pack_bytes = sideband::extract_packfile_bytes(&body_bytes)?;

        // Unpack all base objects directly into loose objects in .git-med/objects/
        let objects_unpacked = packfile::unpack_packfile_into(&pack_bytes, repo_root)?;

        // Write remote tracking branches: .git-med/refs/remotes/origin/*
        let remotes_origin_dir = repo_root.join(".git-med/refs/remotes/origin");
        std::fs::create_dir_all(&remotes_origin_dir)?;

        for r in &advertisement.refs {
            if let Some(branch_rel) = r.name.strip_prefix("refs/heads/") {
                let remote_branch_path = remotes_origin_dir.join(branch_rel);
                if let Some(parent) = remote_branch_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&remote_branch_path, format!("{}\n", r.hash.to_hex()))?;
            }
        }

        Ok(objects_unpacked)
    }

    /// Negotiate and download objects from the remote repository, unpack them directly into
    /// `<repo_root>/.git-med/objects/`, and materialize references on disk.
    pub async fn fetch_and_unpack(
        &self,
        repo_url: &str,
        repo_root: &Path,
        advertisement: &RefAdvertisement,
    ) -> Result<FetchResult, RemoteError> {
        // 1. Determine wanted object hashes from advertised references
        let mut wants = Vec::new();
        for r in &advertisement.refs {
            if !wants.contains(&r.hash) {
                wants.push(r.hash);
            }
        }

        let default_branch_clean = advertisement
            .default_branch
            .as_deref()
            .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b).to_string());

        let default_branch_name = default_branch_clean
            .clone()
            .unwrap_or_else(|| "main".to_string());

        // Always write .git-med/HEAD pointing to local default branch
        let head_file = repo_root.join(".git-med/HEAD");
        std::fs::write(
            &head_file,
            format!("ref: refs/heads/{}\n", default_branch_name),
        )?;

        if wants.is_empty() {
            let local_branch_path = repo_root
                .join(".git-med/refs/heads")
                .join(&default_branch_name);
            if let Some(parent) = local_branch_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if !local_branch_path.exists() {
                std::fs::File::create(&local_branch_path)?;
            }
            return Ok(FetchResult {
                objects_unpacked: 0,
                default_branch: Some(default_branch_name),
                head_hash: None,
            });
        }

        let objects_unpacked = self
            .fetch_packfile_and_unpack_internal(repo_url, repo_root, advertisement, &wants)
            .await?;

        // 6. Materialize branch references on disk

        let head_hash = advertisement.head_hash().or_else(|| {
            if let Some(ref branch) = default_branch_clean {
                let full_ref = format!("refs/heads/{}", branch);
                advertisement.get_ref(&full_ref).map(|r| r.hash)
            } else {
                advertisement.refs.first().map(|r| r.hash)
            }
        });

        // Write local default branch: .git-med/refs/heads/<default_branch>
        let default_branch_name = default_branch_clean
            .clone()
            .unwrap_or_else(|| "main".to_string());

        if let Some(hash) = head_hash {
            let local_branch_path = repo_root
                .join(".git-med/refs/heads")
                .join(&default_branch_name);
            if let Some(parent) = local_branch_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&local_branch_path, format!("{}\n", hash.to_hex()))?;
        }

        // Write .git-med/HEAD pointing to local default branch
        let head_file = repo_root.join(".git-med/HEAD");
        std::fs::write(
            &head_file,
            format!("ref: refs/heads/{}\n", default_branch_name),
        )?;

        Ok(FetchResult {
            objects_unpacked,
            default_branch: Some(default_branch_name),
            head_hash,
        })
    }

    /// Clone a remote repository into `target_dir`:
    /// 1. Initializes `.git-med/` repository directory structure.
    /// 2. Populates `.git-med/config` with `[remote "origin"]`.
    /// 3. Performs reference discovery (`discover_refs`).
    /// 4. Negotiates and downloads the packfile, unpacking objects into `.git-med/objects/`.
    /// 5. Materializes local branches, remote tracking branches, and `HEAD`.
    /// 6. Materializes files onto disk using the active view.
    pub async fn clone_repo(
        &self,
        repo_url: &str,
        target_dir: &Path,
    ) -> Result<FetchResult, RemoteError> {
        let git_med = target_dir.join(".git-med");
        std::fs::create_dir_all(git_med.join("refs/heads"))?;
        std::fs::create_dir_all(git_med.join("refs/remotes/origin"))?;
        std::fs::create_dir_all(git_med.join("refs/tags"))?;
        std::fs::create_dir_all(git_med.join("objects"))?;

        let index_file = git_med.join("index");
        if !index_file.exists() {
            std::fs::File::create(&index_file)?;
        }

        let view_file = git_med.join("VIEW");
        if !view_file.exists() {
            std::fs::write(&view_file, "SMART\n")?;
        }

        let lfs_track_file = git_med.join("lfs_track");
        if !lfs_track_file.exists() {
            let _ = std::fs::write(&lfs_track_file, crate::files::lfs::DEFAULT_LFS_PATTERNS);
        }

        // Populate .git-med/config with [remote "origin"]
        let config_path = git_med.join("config");
        let config_content = crate::core::remote::format_remote_ini_section("origin", repo_url);
        std::fs::write(&config_path, config_content)?;

        // Ensure default HEAD and default branch ref exist as baseline
        let head_file = git_med.join("HEAD");
        if !head_file.exists() {
            std::fs::write(&head_file, "ref: refs/heads/main\n")?;
        }
        let default_branch_ref = git_med.join("refs/heads/main");
        if !default_branch_ref.exists() {
            std::fs::File::create(&default_branch_ref)?;
        }

        let advertisement = self.discover_refs(repo_url).await?;
        let result = self.fetch_and_unpack(repo_url, target_dir, &advertisement).await?;

        // Materialize files onto disk using the active view
        if let Some(ref head_hash) = result.head_hash {
            let tree = crate::files::commit::load_tree_from_commit_in(target_dir, head_hash)
                .map_err(|e| RemoteError::Protocol(format!("failed to load tree from commit {}: {}", head_hash.to_hex(), e)))?;
            tree.save(&git_med.join("index"))
                .map_err(|e| RemoteError::Protocol(format!("failed to save index: {}", e)))?;

            // Download any missing LFS objects before materializing
            self.download_lfs_objects_for_tree(repo_url, target_dir, &tree.root).await?;

            let view_name = crate::core::view::read_view_name(&git_med.join("VIEW"))
                .unwrap_or_else(|_| "smart".to_string());
            let view = crate::views::get_view_in(&view_name, target_dir)
                .ok_or_else(|| RemoteError::Protocol(format!("unsupported view '{}'", view_name)))?;
            view.write(&tree.root)
                .map_err(|e| RemoteError::Protocol(format!("view write failed: {}", e)))?;
        }

        Ok(result)
    }

    /// Pull remote changes for `branch_name` from `repo_url` into `repo_root`:
    /// 1. Fetches remote packfile and unpacks objects into `.git-med/objects/`.
    /// 2. Validates fast-forward reachability (rejecting non-fast-forward diverged histories).
    /// 3. Fast-forwards the local branch reference.
    /// 4. Updates `.git-med/HEAD`.
    /// 5. Materializes the updated working tree via `view.write`.
    pub async fn pull_branch(
        &self,
        repo_url: &str,
        repo_root: &Path,
        branch_name: &str,
    ) -> Result<PullResult, RemoteError> {
        let clean_branch = branch_name.trim();
        if clean_branch.is_empty() {
            return Err(RemoteError::Protocol("branch name cannot be empty".into()));
        }
        let short_branch = clean_branch.strip_prefix("refs/heads/").unwrap_or(clean_branch);
        let full_ref_name = format!("refs/heads/{}", short_branch);

        // 1. Resolve local branch commit hash (if branch exists)
        let local_commit_opt = crate::core::branch::load_ref_in(repo_root, short_branch)
            .map_err(|e| RemoteError::Protocol(format!("failed to load local branch {}: {}", short_branch, e)))?;

        // 2. Discover references on remote server
        let advertisement = self.discover_refs(repo_url).await?;

        // 3. Find target remote commit hash for this ref
        let remote_ref = advertisement
            .get_ref(&full_ref_name)
            .or_else(|| {
                if short_branch == "HEAD" {
                    advertisement.get_ref("HEAD")
                } else if advertisement.default_branch.as_deref().map(|b| b.strip_prefix("refs/heads/").unwrap_or(b)) == Some(short_branch) {
                    advertisement.get_ref("HEAD")
                } else {
                    None
                }
            })
            .ok_or_else(|| RemoteError::Protocol(format!("branch '{}' not found on remote", short_branch)))?;
        let remote_hash = remote_ref.hash;

        // 4. If already at the remote commit, nothing to pull
        if local_commit_opt == Some(remote_hash) {
            let remote_tracking_path = repo_root.join(".git-med/refs/remotes/origin").join(short_branch);
            if let Some(parent) = remote_tracking_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&remote_tracking_path, format!("{}\n", remote_hash.to_hex()))?;

            return Ok(PullResult {
                branch: short_branch.to_string(),
                old_hash: local_commit_opt,
                new_hash: remote_hash,
                objects_unpacked: 0,
                already_up_to_date: true,
            });
        }

        // 5. Fetch packfile containing remote objects and unpack into .git-med/objects/
        let mut wants = Vec::new();
        for r in &advertisement.refs {
            if !wants.contains(&r.hash) {
                wants.push(r.hash);
            }
        }
        if !wants.contains(&remote_hash) {
            wants.push(remote_hash);
        }

        let objects_unpacked = self
            .fetch_packfile_and_unpack_internal(repo_url, repo_root, &advertisement, &wants)
            .await?;

        // 6. Validate fast-forward: if local commit exists, ensure it is an ancestor of remote commit
        if let Some(local_hash) = local_commit_opt {
            let is_ff = traversal::is_ancestor(repo_root, &local_hash, &remote_hash)?;
            if !is_ff {
                return Err(RemoteError::Protocol(format!(
                    "non-fast-forward update to branch '{}' rejected: remote commit {} is not a descendant of local commit {}",
                    short_branch,
                    remote_hash.to_hex(),
                    local_hash.to_hex()
                )));
            }
        }

        // 7. Fast-forward local branch ref
        let local_branch_path = repo_root.join(".git-med/refs/heads").join(short_branch);
        if let Some(parent) = local_branch_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&local_branch_path, format!("{}\n", remote_hash.to_hex()))?;

        // 8. Update HEAD to point to this branch
        let head_file = repo_root.join(".git-med/HEAD");
        std::fs::write(
            &head_file,
            format!("ref: refs/heads/{}\n", short_branch),
        )?;

        // 9. Materialize working tree via active view
        let git_med = repo_root.join(".git-med");
        let tree = crate::files::commit::load_tree_from_commit_in(repo_root, &remote_hash)
            .map_err(|e| RemoteError::Protocol(format!("failed to load tree from commit {}: {}", remote_hash.to_hex(), e)))?;
        tree.save(&git_med.join("index"))
            .map_err(|e| RemoteError::Protocol(format!("failed to save index: {}", e)))?;

        // Download any missing LFS objects before materializing
        self.download_lfs_objects_for_tree(repo_url, repo_root, &tree.root).await?;

        let view_name = crate::core::view::read_view_name(&git_med.join("VIEW"))
            .unwrap_or_else(|_| "smart".to_string());
        let view = crate::views::get_view_in(&view_name, repo_root)
            .ok_or_else(|| RemoteError::Protocol(format!("unsupported view '{}'", view_name)))?;
        view.write(&tree.root)
            .map_err(|e| RemoteError::Protocol(format!("view write failed: {}", e)))?;

        Ok(PullResult {
            branch: short_branch.to_string(),
            old_hash: local_commit_opt,
            new_hash: remote_hash,
            objects_unpacked,
            already_up_to_date: false,
        })
    }

    /// Fetch a branch's commit and objects from a remote repository into `repo_root/.git-med/objects/`,
    /// including any missing LFS objects, without switching HEAD or materializing the working tree.
    pub async fn fetch_branch(
        &self,
        repo_url: &str,
        repo_root: &Path,
        branch_name: &str,
    ) -> Result<crate::files::hash::Hash, RemoteError> {
        let clean_branch = branch_name.trim();
        if clean_branch.is_empty() {
            return Err(RemoteError::Protocol("branch name cannot be empty".into()));
        }
        let short_branch = clean_branch.strip_prefix("refs/heads/").unwrap_or(clean_branch);
        let full_ref_name = format!("refs/heads/{}", short_branch);

        // 1. Discover references on remote server
        let advertisement = self.discover_refs(repo_url).await?;

        // 2. Find target remote commit hash for this ref
        let remote_ref = advertisement
            .get_ref(&full_ref_name)
            .or_else(|| {
                if short_branch == "HEAD" {
                    advertisement.get_ref("HEAD")
                } else if advertisement.default_branch.as_deref().map(|b| b.strip_prefix("refs/heads/").unwrap_or(b)) == Some(short_branch) {
                    advertisement.get_ref("HEAD")
                } else {
                    None
                }
            })
            .ok_or_else(|| RemoteError::Protocol(format!("branch '{}' not found on remote", short_branch)))?;
        let remote_hash = remote_ref.hash;

        // 3. If commit is already present locally, check LFS, otherwise fetch packfile
        let has_commit = crate::files::utils::read_object_bytes_in(repo_root, &remote_hash).is_ok();
        if !has_commit {
            let mut wants = Vec::new();
            for r in &advertisement.refs {
                if !wants.contains(&r.hash) {
                    wants.push(r.hash);
                }
            }
            if !wants.contains(&remote_hash) {
                wants.push(remote_hash);
            }

            self.fetch_packfile_and_unpack_internal(repo_url, repo_root, &advertisement, &wants)
                .await?;
        }

        // 4. Download any missing LFS objects for the fetched commit's tree
        let tree = crate::files::commit::load_tree_from_commit_in(repo_root, &remote_hash)
            .map_err(|e| RemoteError::Protocol(format!("failed to load tree from commit {}: {}", remote_hash.to_hex(), e)))?;

        self.download_lfs_objects_for_tree(repo_url, repo_root, &tree.root).await?;

        Ok(remote_hash)
    }

    /// Discover remote references for pushing via `GET $REPO_URL/info/refs?service=git-receive-pack`.
    pub async fn discover_push_refs(&self, repo_url: &str) -> Result<RefAdvertisement, RemoteError> {
        let trimmed_url = repo_url.trim_end_matches('/');
        let info_refs_url = format!("{}/info/refs?service=git-receive-pack", trimmed_url);

        let req = self
            .client
            .get(&info_refs_url)
            .header("Accept", "application/x-git-receive-pack-advertisement")
            .header("User-Agent", "git-med/0.1.0");

        let response = self.apply_auth(req, repo_url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let msg = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(Self::check_auth_status(status, repo_url, msg));
        }

        let bytes = response.bytes().await?;
        let packets = pkt_line::decode_pkt_lines(&bytes)
            .map_err(|e| RemoteError::PktLine(e.to_string()))?;

        match parse_initial_advertisement(&packets)? {
            DiscoveryHeader::Version1(adv) => Ok(adv),
            DiscoveryHeader::Version2(caps) => {
                Ok(RefAdvertisement {
                    protocol_version: 2,
                    service: "git-receive-pack".to_string(),
                    capabilities: caps,
                    default_branch: None,
                    refs: Vec::new(),
                })
            }
        }
    }

    /// Push a local branch to a remote repository via Git Smart HTTP (`POST /git-receive-pack`).
    ///
    /// 1. Resolves local commit hash for `branch_name` in `<repo_root>/.git-med/refs/heads/<branch_name>`.
    /// 2. Performs reference discovery via `discover_push_refs`.
    /// 3. Traverses commit and tree reachability to determine objects missing on the remote.
    /// 4. Generates a Git Packfile v2 containing the missing objects (or omits it if up to date).
    /// 5. Posts the push request (`<old_id> <new_id> <ref>\0report-status...`) to `$REPO_URL/git-receive-pack`.
    /// 6. Parses the `report-status` response; returns an error if any ref update failed (e.g. non-fast-forward).
    /// 7. Upon success, updates the local remote-tracking reference at `.git-med/refs/remotes/origin/<branch>`.
    pub async fn push_branch(
        &self,
        repo_url: &str,
        repo_root: &Path,
        branch_name: &str,
    ) -> Result<PushResult, RemoteError> {
        let clean_branch = branch_name.trim();
        if clean_branch.is_empty() {
            return Err(RemoteError::Protocol("branch name cannot be empty".into()));
        }
        let short_branch = clean_branch.strip_prefix("refs/heads/").unwrap_or(clean_branch);
        let full_ref_name = format!("refs/heads/{}", short_branch);

        // 1. Resolve local branch commit hash
        let local_commit = crate::core::branch::load_ref_in(repo_root, short_branch)
            .map_err(|e| RemoteError::Protocol(format!("failed to load local branch {}: {}", short_branch, e)))?
            .ok_or_else(|| RemoteError::Protocol(format!("branch '{}' not found locally under .git-med/refs/heads/", short_branch)))?;

        // 2. Discover references on remote server
        let push_adv = self.discover_push_refs(repo_url).await?;

        // 3. Find existing remote commit hash for this ref (if present)
        let remote_ref = push_adv.get_ref(&full_ref_name);
        let old_hash = remote_ref.map(|r| r.hash);

        // Collect all remote known commit hashes to bound our reachability traversal
        let remote_known_hashes: Vec<crate::files::hash::Hash> =
            push_adv.refs.iter().map(|r| r.hash).collect();

        // 4. Collect objects to pack (C_new .. C_old)
        let objects_to_pack =
            traversal::collect_objects_to_pack(repo_root, &local_commit, &remote_known_hashes)?;

        // Upload any LFS objects to S3 before pushing packfile
        self.upload_lfs_objects(repo_url, repo_root, &objects_to_pack).await?;

        // 5. Generate packfile bytes (if objects need to be uploaded)
        let packfile_bytes = if !objects_to_pack.is_empty() {
            let objects_slices: Vec<(crate::files::types::ObjectType, &[u8])> = objects_to_pack
                .iter()
                .map(|(_, obj_type, body)| (*obj_type, body.as_slice()))
                .collect();
            packfile::create_packfile_bytes(&objects_slices)?
        } else {
            Vec::new()
        };

        // 6. Build push request body
        let request_body = push::build_push_request(
            old_hash.as_ref(),
            &local_commit,
            &full_ref_name,
            &packfile_bytes,
        )?;

        // 7. POST /git-receive-pack
        let trimmed_url = repo_url.trim_end_matches('/');
        let receive_pack_url = format!("{}/git-receive-pack", trimmed_url);

        let post_req = self
            .client
            .post(&receive_pack_url)
            .header("Content-Type", "application/x-git-receive-pack-request")
            .header("Accept", "application/x-git-receive-pack-result")
            .header("User-Agent", "git-med/0.1.0")
            .body(request_body);

        let response = self.apply_auth(post_req, repo_url).send().await?;

        let status = response.status();
        if !status.is_success() {
            let msg = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(Self::check_auth_status(status, repo_url, msg));
        }

        let response_bytes = response.bytes().await?;
        let ref_updates = push::parse_push_response(&response_bytes)?;

        // Check if the ref update was rejected (e.g. non-fast-forward)
        if let Some(failed) = ref_updates.iter().find(|r| !r.success) {
            return Err(RemoteError::Protocol(format!(
                "push to ref '{}' rejected by server: {}",
                failed.ref_name,
                failed.error.as_deref().unwrap_or("unknown error")
            )));
        }

        // 8. Update local remote-tracking branch under .git-med/refs/remotes/origin/<branch>
        let remote_tracking_dir = repo_root.join(".git-med/refs/remotes/origin");
        std::fs::create_dir_all(&remote_tracking_dir)?;
        let remote_tracking_path = remote_tracking_dir.join(short_branch);
        if let Some(parent) = remote_tracking_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&remote_tracking_path, format!("{}\n", local_commit.to_hex()))?;

        Ok(PushResult {
            branch: short_branch.to_string(),
            old_hash,
            new_hash: local_commit,
            objects_pushed: objects_to_pack.len(),
            ref_updates,
        })
    }

    /// Upload all raw LFS objects to S3 via presigned PUT URLs requested from the remote server.
    pub async fn upload_lfs_objects(
        &self,
        repo_url: &str,
        repo_root: &Path,
        objects_to_pack: &[(crate::files::hash::Hash, crate::files::types::ObjectType, Vec<u8>)],
    ) -> Result<(), RemoteError> {
        let mut lfs_uploads = Vec::new();
        let mut raw_paths = Vec::new();

        for (_, obj_type, body) in objects_to_pack {
            if *obj_type == crate::files::types::ObjectType::Lfs {
                let pointer = crate::files::lfs::LfsPointer::parse(body)
                    .map_err(|e| RemoteError::Protocol(format!("invalid LFS pointer object: {}", e)))?;
                let raw_path = if pointer.local_path != "self" && std::path::Path::new(&pointer.local_path).is_file() {
                    std::path::PathBuf::from(&pointer.local_path)
                } else {
                    crate::files::lfs::lfs_raw_object_path(repo_root, &pointer.hash)
                };
                if !raw_path.is_file() {
                    return Err(RemoteError::Protocol(format!(
                        "missing raw LFS file on disk at {} for hash {}",
                        raw_path.display(),
                        pointer.hash.to_hex()
                    )));
                }
                let meta = std::fs::metadata(&raw_path)?;
                lfs_uploads.push(crate::remote::types::LfsUploadObjectReq {
                    hash: pointer.hash.to_hex(),
                    remote_path: pointer.remote_path,
                    size: meta.len(),
                });
                raw_paths.push((pointer.hash.to_hex(), raw_path));
            }
        }

        if lfs_uploads.is_empty() {
            return Ok(());
        }

        let trimmed_url = repo_url.trim_end_matches('/');
        let upload_urls_endpoint = format!("{}/lfs/upload-urls", trimmed_url);

        let req_payload = crate::remote::types::LfsUploadUrlsReq {
            objects: lfs_uploads,
        };
        let body_bytes = serde_json::to_vec(&req_payload)
            .map_err(|e| RemoteError::Protocol(format!("failed to serialize JSON: {}", e)))?;

        let post_req = self
            .client
            .post(&upload_urls_endpoint)
            .header("Content-Type", "application/json")
            .body(body_bytes);

        let resp = self.apply_auth(post_req, repo_url).send().await?;

        let status = resp.status();
        if status.as_u16() == 501 || status.as_u16() == 503 {
            return Err(RemoteError::Protocol(
                "S3 storage is disabled on remote server".to_string(),
            ));
        }
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_else(|_| "unknown error".into());
            return Err(Self::check_auth_status(
                status,
                repo_url,
                format!("LFS upload-urls failed: {}", err_text),
            ));
        }

        let resp_bytes = resp.bytes().await?;
        let resp_body: crate::remote::types::LfsUrlsResp = serde_json::from_slice(&resp_bytes)
            .map_err(|e| RemoteError::Protocol(format!("failed to parse JSON response: {}", e)))?;

        for (hex, path) in raw_paths {
            let put_url = resp_body.urls.get(&hex).ok_or_else(|| {
                RemoteError::Protocol(format!("server did not return upload url for {}", hex))
            })?;

            let bytes = std::fs::read(&path)?;
            let put_resp = self.client.put(put_url).body(bytes).send().await?;
            if !put_resp.status().is_success() {
                let err_msg = put_resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "unknown error".into());
                return Err(RemoteError::Protocol(format!(
                    "failed to upload LFS object {} to S3: {}",
                    hex, err_msg
                )));
            }
        }

        Ok(())
    }

    /// Download all missing raw LFS objects referenced in a commit tree from S3.
    pub async fn download_lfs_objects_for_tree(
        &self,
        repo_url: &str,
        repo_root: &Path,
        root: &crate::core::tree::NodeRef,
    ) -> Result<(), RemoteError> {
        let raw_hashes = collect_lfs_hashes_from_tree(repo_root, root)?;
        let mut missing_hashes = Vec::new();

        for h in raw_hashes {
            if !crate::files::lfs::has_lfs_raw(repo_root, &h) {
                missing_hashes.push(h.to_hex());
            }
        }

        if missing_hashes.is_empty() {
            return Ok(());
        }

        let trimmed_url = repo_url.trim_end_matches('/');
        let download_urls_endpoint = format!("{}/lfs/download-urls", trimmed_url);

        let req_payload = crate::remote::types::LfsDownloadUrlsReq {
            hashes: missing_hashes.clone(),
        };
        let body_bytes = serde_json::to_vec(&req_payload)
            .map_err(|e| RemoteError::Protocol(format!("failed to serialize JSON: {}", e)))?;

        let post_req = self
            .client
            .post(&download_urls_endpoint)
            .header("Content-Type", "application/json")
            .body(body_bytes);

        let resp = self.apply_auth(post_req, repo_url).send().await?;

        let status = resp.status();
        if status.as_u16() == 501 || status.as_u16() == 503 {
            return Err(RemoteError::Protocol(
                "S3 storage is disabled on remote server".to_string(),
            ));
        }
        if !status.is_success() {
            let err_text = resp.text().await.unwrap_or_else(|_| "unknown error".into());
            return Err(Self::check_auth_status(
                status,
                repo_url,
                format!("LFS download-urls failed: {}", err_text),
            ));
        }

        let resp_bytes = resp.bytes().await?;
        let resp_body: crate::remote::types::LfsUrlsResp = serde_json::from_slice(&resp_bytes)
            .map_err(|e| RemoteError::Protocol(format!("failed to parse JSON response: {}", e)))?;

        for hex in missing_hashes {
            let get_url = resp_body.urls.get(&hex).ok_or_else(|| {
                RemoteError::Protocol(format!("server did not return download url for {}", hex))
            })?;

            let get_resp = self.client.get(get_url).send().await?;
            if !get_resp.status().is_success() {
                return Err(RemoteError::Protocol(format!(
                    "failed to download LFS object {} from S3: status {}",
                    hex,
                    get_resp.status()
                )));
            }

            let bytes = get_resp.bytes().await?;
            let downloaded_hash = crate::files::hash::hash_raw(&bytes);
            if downloaded_hash.to_hex() != hex {
                return Err(RemoteError::Protocol(format!(
                    "corrupted LFS download for {}: hash mismatch (got {})",
                    hex,
                    downloaded_hash.to_hex()
                )));
            }

            crate::files::lfs::stage_lfs_bytes(repo_root, &bytes, &downloaded_hash)
                .map_err(|e| RemoteError::Protocol(format!("failed to stage downloaded LFS file: {}", e)))?;
        }

        Ok(())
    }
}

/// Helper to recursively traverse a tree root and collect all referenced raw LFS hashes.
fn collect_lfs_hashes_from_tree(
    repo_root: &Path,
    root: &crate::core::tree::NodeRef,
) -> Result<Vec<crate::files::hash::Hash>, RemoteError> {
    let mut fringe = std::collections::VecDeque::new();
    fringe.push_back(std::rc::Rc::clone(root));
    let mut lfs_raw_hashes = Vec::new();

    while let Some(curr) = fringe.pop_front() {
        let node = curr.borrow();

        if let Some(ref h) = node.self_hash {
            if let Ok((obj_type, body)) = crate::files::utils::read_object_bytes_in(repo_root, h) {
                if obj_type == crate::files::types::ObjectType::Lfs {
                    if let Ok(pointer) = crate::files::lfs::LfsPointer::parse(&body) {
                        lfs_raw_hashes.push(pointer.hash);
                    }
                }
            }
        }

        match &node.children {
            crate::core::tree::Children::Attributes(map) | crate::core::tree::Children::Files(map) => {
                for child in map.values() {
                    fringe.push_back(std::rc::Rc::clone(child));
                }
            }
            crate::core::tree::Children::None => {}
        }

        for meta_node in node.meta.values() {
            fringe.push_back(std::rc::Rc::clone(meta_node));
        }
    }

    Ok(lfs_raw_hashes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::add::add_in;
    use crate::core::commit::commit_with_message_in;
    use crate::core::init::init_at;
    use crate::core::remote::list_remotes_in;
    use crate::files::hash::Hash;
    use crate::files::types::ObjectType;
    use crate::remote::packfile::create_packfile_bytes;
    use crate::remote::pkt_line::encode_str;
    use crate::remote::traversal::collect_objects_to_pack;
    use crate::tester::generate::sample::smart;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDirRepo {
        path: PathBuf,
    }

    impl TempDirRepo {
        fn new(prefix: &str) -> Self {
            let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "git_med_client_test_{}_{}_{}_{}",
                prefix,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                id
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("failed to create temp dir");
            Self { path }
        }
    }

    impl Drop for TempDirRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// Spawns a mock Git HTTP server responding to protocol v2 upload-pack requests.
    async fn start_mock_git_server(
        head_hash: Hash,
        pack_bytes: Vec<u8>,
        lfs_raw_dir: Option<std::path::PathBuf>,
    ) -> (String, tokio::sync::oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{}", port);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept_res = listener.accept() => {
                        let Ok((mut socket, _)) = accept_res else { break; };
                        let head_hash = head_hash;
                        let pack_bytes = pack_bytes.clone();
                        let lfs_dir = lfs_raw_dir.clone();

                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 16384];
                            let n = match socket.read(&mut buf).await {
                                Ok(n) if n > 0 => n,
                                _ => return,
                            };
                            let req = String::from_utf8_lossy(&buf[..n]);

                            if req.contains("GET /info/refs?service=git-upload-pack") {
                                let mut adv_body = Vec::new();
                                adv_body.extend_from_slice(&encode_str("version 2\n").unwrap());
                                adv_body.extend_from_slice(&encode_str("ls-refs\n").unwrap());
                                adv_body.extend_from_slice(&encode_str("fetch=shallow\n").unwrap());
                                adv_body.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-advertisement\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    adv_body.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&adv_body).await;
                            } else if req.contains("POST /git-upload-pack") {
                                if req.contains("command=ls-refs") {
                                    let mut ls_body = Vec::new();
                                    let line1 = format!("{} HEAD symref-target:refs/heads/main\n", head_hash.to_hex());
                                    let line2 = format!("{} refs/heads/main\n", head_hash.to_hex());
                                    ls_body.extend_from_slice(&encode_str(&line1).unwrap());
                                    ls_body.extend_from_slice(&encode_str(&line2).unwrap());
                                    ls_body.extend_from_slice(pkt_line::encode_flush());

                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        ls_body.len()
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.write_all(&ls_body).await;
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
                                let hex = req
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|p| p.strip_prefix("/s3-download/"))
                                    .unwrap_or("");
                                let raw_bytes = if let Some(ref dir) = lfs_dir {
                                    let p = dir.join(&hex[..2.min(hex.len())]).join(&hex[2.min(hex.len())..]);
                                    fs::read(&p).unwrap_or_else(|_| crate::tester::generate::sample::DICOM_SAMPLE.to_vec())
                                } else {
                                    crate::tester::generate::sample::DICOM_SAMPLE.to_vec()
                                };
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

        (url, shutdown_tx)
    }

    #[tokio::test]
    async fn test_clone_repo_populates_config_and_materializes_view() {
        // 1. Prepare source repository with a commit and dataset
        let src = TempDirRepo::new("src_clone");
        init_at(&src.path);
        smart(&src.path, &["data"], 1, 1, 1, 1);
        add_in(&src.path).expect("add_in");
        let commit_hash = commit_with_message_in(&src.path, "commit 1").expect("commit");

        // 2. Generate packfile for source repo commit
        let objects = collect_objects_to_pack(&src.path, &commit_hash, &[]).expect("collect objects");
        let obj_slices: Vec<(ObjectType, &[u8])> = objects.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack_bytes = create_packfile_bytes(&obj_slices).expect("packfile bytes");

        // 3. Start mock server
        let (server_url, _shutdown) = start_mock_git_server(
            commit_hash,
            pack_bytes,
            Some(src.path.join(".git-med/lfs/objects")),
        )
        .await;

        // 4. Clone into destination repository
        let dest = TempDirRepo::new("dest_clone");
        let client = GitHttpClient::new();
        let fetch_res = client.clone_repo(&server_url, &dest.path).await.expect("clone_repo");

        assert_eq!(fetch_res.head_hash, Some(commit_hash));

        // Verify .git-med/config is populated with [remote "origin"]
        let config_path = dest.path.join(".git-med/config");
        assert!(config_path.is_file(), "config file must exist");
        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("[remote \"origin\"]"));
        assert!(config_content.contains(&format!("url = {}", server_url)));

        // Verify remotes listing sees "origin"
        let remotes = list_remotes_in(&dest.path).expect("list_remotes_in");
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].url, server_url);

        // Verify files materialized onto disk using active view
        let dcm_file = dest.path.join("data/clinic/subject_001/study_001/series_001/instance_001.dcm");
        assert!(dcm_file.is_file(), "instance file should be materialized on disk");

        // Verify .git-med/index is populated
        let index_path = dest.path.join(".git-med/index");
        assert!(index_path.is_file());
        assert!(fs::metadata(&index_path).unwrap().len() > 0);

        // Verify HEAD and refs
        let head_content = fs::read_to_string(dest.path.join(".git-med/HEAD")).unwrap();
        assert_eq!(head_content.trim(), "ref: refs/heads/main");
        let main_ref = fs::read_to_string(dest.path.join(".git-med/refs/heads/main")).unwrap();
        assert_eq!(main_ref.trim(), commit_hash.to_hex());
    }

    #[tokio::test]
    async fn test_pull_branch_fast_forward_and_up_to_date() {
        // 1. Prepare source repository with commit 1
        let src = TempDirRepo::new("src_pull");
        init_at(&src.path);
        smart(&src.path, &["data"], 1, 1, 1, 1);
        add_in(&src.path).expect("add_in");
        let commit1 = commit_with_message_in(&src.path, "commit 1").expect("commit 1");

        // Clone into target
        let dest = TempDirRepo::new("dest_pull");
        init_at(&dest.path);
        crate::core::remote::remote_add_in(&dest.path, "origin", "http://placeholder").unwrap();

        // Write commit 1 into dest and point main branch to it
        let objects1 = collect_objects_to_pack(&src.path, &commit1, &[]).unwrap();
        let obj_slices1: Vec<(ObjectType, &[u8])> = objects1.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack1 = create_packfile_bytes(&obj_slices1).unwrap();
        crate::remote::packfile::unpack_packfile_into(&pack1, &dest.path).unwrap();
        fs::write(dest.path.join(".git-med/refs/heads/main"), format!("{}\n", commit1.to_hex())).unwrap();

        // 2. Test already up to date
        let (server_url1, _shutdown1) =
            start_mock_git_server(commit1, pack1, Some(src.path.join(".git-med/lfs/objects"))).await;
        let client = GitHttpClient::new();
        let pull_res1 = client.pull_branch(&server_url1, &dest.path, "main").await.expect("pull 1");
        assert!(pull_res1.already_up_to_date);
        assert_eq!(pull_res1.new_hash, commit1);

        // 3. Create commit 2 on src
        let dummy_seg = src.path.join("data/clinic/subject_001/study_001/series_001/instance_002.dcm");
        fs::write(&dummy_seg, crate::tester::generate::sample::DICOM_SAMPLE).unwrap();
        add_in(&src.path).expect("add_in");
        let commit2 = commit_with_message_in(&src.path, "commit 2").expect("commit 2");

        let objects2 = collect_objects_to_pack(&src.path, &commit2, &[]).unwrap();
        let obj_slices2: Vec<(ObjectType, &[u8])> = objects2.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack2 = create_packfile_bytes(&obj_slices2).unwrap();

        // 4. Test fast-forward pull
        let (server_url2, _shutdown2) =
            start_mock_git_server(commit2, pack2, Some(src.path.join(".git-med/lfs/objects"))).await;
        let pull_res2 = client.pull_branch(&server_url2, &dest.path, "main").await.expect("pull 2");
        assert!(!pull_res2.already_up_to_date);
        assert_eq!(pull_res2.old_hash, Some(commit1));
        assert_eq!(pull_res2.new_hash, commit2);

        // Verify ref and HEAD updated
        let main_ref = fs::read_to_string(dest.path.join(".git-med/refs/heads/main")).unwrap();
        assert_eq!(main_ref.trim(), commit2.to_hex());

        // Verify materialized working tree contains instance_002
        let file2 = dest.path.join("data/clinic/subject_001/study_001/series_001/instance_002.dcm");
        assert!(file2.is_file(), "instance_002 must be materialized after fast-forward pull");
    }

    #[tokio::test]
    async fn test_pull_branch_rejects_non_fast_forward() {
        // 1. Prepare base repo
        let base = TempDirRepo::new("base_nff");
        init_at(&base.path);
        smart(&base.path, &["data"], 1, 1, 1, 1);
        add_in(&base.path).expect("add_in");
        let c_base = commit_with_message_in(&base.path, "base commit").unwrap();

        // 2. Dest repo advances with local commit
        let dest = TempDirRepo::new("dest_nff");
        init_at(&dest.path);
        let objs_base = collect_objects_to_pack(&base.path, &c_base, &[]).unwrap();
        let slices_base: Vec<(ObjectType, &[u8])> = objs_base.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack_base = create_packfile_bytes(&slices_base).unwrap();
        crate::remote::packfile::unpack_packfile_into(&pack_base, &dest.path).unwrap();
        fs::write(dest.path.join(".git-med/refs/heads/main"), format!("{}\n", c_base.to_hex())).unwrap();

        // Add local commit on dest
        let dest_extra = dest.path.join("data/clinic/subject_001/study_001/series_001/local.dcm");
        if let Some(parent) = dest_extra.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&dest_extra, crate::tester::generate::sample::DICOM_SAMPLE).unwrap();
        add_in(&dest.path).expect("add_in");
        let c_dest = commit_with_message_in(&dest.path, "dest local commit").unwrap();

        // 3. Remote repo advances with a separate diverged commit
        let remote_extra = base.path.join("data/clinic/subject_001/study_001/series_001/remote.dcm");
        if let Some(parent) = remote_extra.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&remote_extra, crate::tester::generate::sample::DICOM_SAMPLE).unwrap();
        add_in(&base.path).expect("add_in");
        let c_remote = commit_with_message_in(&base.path, "remote diverged commit").unwrap();

        let objs_remote = collect_objects_to_pack(&base.path, &c_remote, &[]).unwrap();
        let slices_remote: Vec<(ObjectType, &[u8])> = objs_remote.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack_remote = create_packfile_bytes(&slices_remote).unwrap();

        let (server_url, _shutdown) = start_mock_git_server(c_remote, pack_remote, None).await;

        let client = GitHttpClient::new();
        let err = client.pull_branch(&server_url, &dest.path, "main").await.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("non-fast-forward"),
            "expected non-fast-forward error, got: {}",
            err_msg
        );

        // Branch ref on dest should still point to local commit
        let main_ref = fs::read_to_string(dest.path.join(".git-med/refs/heads/main")).unwrap();
        assert_eq!(main_ref.trim(), c_dest.to_hex());
    }

    #[tokio::test]
    async fn test_push_branch_updates_remote_tracking_ref() {
        let repo = TempDirRepo::new("push_tracking");
        init_at(&repo.path);
        smart(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).expect("add_in");
        let commit_hash = commit_with_message_in(&repo.path, "push commit").unwrap();

        // Spawn mock git-receive-pack server
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let port = listener.local_addr().unwrap().port();
        let server_url = format!("http://127.0.0.1:{}", port);
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept_res = listener.accept() => {
                        let Ok((mut socket, _)) = accept_res else { break; };
                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 8192];
                            let n = match socket.read(&mut buf).await {
                                Ok(n) if n > 0 => n,
                                _ => return,
                            };
                            let req = String::from_utf8_lossy(&buf[..n]);

                            if req.contains("GET /info/refs?service=git-receive-pack") {
                                let mut adv = Vec::new();
                                // Version 1 advertisement for receive-pack
                                adv.extend_from_slice(&encode_str("# service=git-receive-pack\n").unwrap());
                                adv.extend_from_slice(pkt_line::encode_flush());
                                let line1 = format!(
                                    "0000000000000000000000000000000000000000 capabilities^{{}}\0report-status delete-refs\n"
                                );
                                adv.extend_from_slice(&encode_str(&line1).unwrap());
                                adv.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-receive-pack-advertisement\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    adv.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&adv).await;
                            } else if req.contains("POST /lfs/upload-urls") {
                                let body_start = req.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
                                let body_bytes = &buf[body_start..n];
                                if let Ok(req_json) = serde_json::from_slice::<crate::remote::types::LfsUploadUrlsReq>(body_bytes) {
                                    let base_url = format!("http://127.0.0.1:{}", port);
                                    let mut urls = std::collections::HashMap::new();
                                    for obj in req_json.objects {
                                        urls.insert(obj.hash.clone(), format!("{}/s3-upload/{}", base_url, obj.hash));
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
                            } else if req.contains("PUT /s3-upload/") {
                                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                                let _ = socket.write_all(resp.as_bytes()).await;
                            } else if req.contains("POST /git-receive-pack") {
                                // report-status response
                                let mut report = Vec::new();
                                report.extend_from_slice(&encode_str("unpack ok\n").unwrap());
                                report.extend_from_slice(&encode_str("ok refs/heads/main\n").unwrap());
                                report.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-receive-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    report.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&report).await;
                            }
                        });
                    }
                }
            }
        });

        let client = GitHttpClient::new();
        let push_res = client.push_branch(&server_url, &repo.path, "main").await.expect("push_branch");
        assert_eq!(push_res.branch, "main");
        assert_eq!(push_res.new_hash, commit_hash);

        // Verify remote-tracking reference was written
        let tracking_ref_path = repo.path.join(".git-med/refs/remotes/origin/main");
        assert!(tracking_ref_path.is_file());
        let tracking_ref = fs::read_to_string(&tracking_ref_path).unwrap();
        assert_eq!(tracking_ref.trim(), commit_hash.to_hex());
    }

    #[tokio::test]
    async fn test_push_branch_uploads_lfs_objects() {
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let repo = TempDirRepo::new("push_lfs");
        init_at(&repo.path);

        // Configure LFS tracking for *.dcm
        fs::write(repo.path.join(".git-med/lfs_track"), "*.dcm\n").unwrap();

        smart(&repo.path, &["data"], 1, 1, 1, 1);
        add_in(&repo.path).expect("add_in with lfs");
        let commit_hash = commit_with_message_in(&repo.path, "lfs commit").unwrap();

        let uploaded_s3_payloads: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
        let uploaded_clone = Arc::clone(&uploaded_s3_payloads);

        // Spawn mock server handling lfs upload-urls and S3 PUT
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let port = listener.local_addr().unwrap().port();
        let server_url = format!("http://127.0.0.1:{}", port);
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept_res = listener.accept() => {
                        let Ok((mut socket, _)) = accept_res else { break; };
                        let uploaded = Arc::clone(&uploaded_clone);
                        let base_url = format!("http://127.0.0.1:{}", port);

                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 16384];
                            let n = match socket.read(&mut buf).await {
                                Ok(n) if n > 0 => n,
                                _ => return,
                            };
                            let req_head = String::from_utf8_lossy(&buf[..n]);

                            if req_head.contains("GET /info/refs?service=git-receive-pack") {
                                let mut adv = Vec::new();
                                adv.extend_from_slice(&encode_str("# service=git-receive-pack\n").unwrap());
                                adv.extend_from_slice(pkt_line::encode_flush());
                                let line1 = "0000000000000000000000000000000000000000 capabilities^{}\0report-status delete-refs\n";
                                adv.extend_from_slice(&encode_str(line1).unwrap());
                                adv.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-receive-pack-advertisement\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    adv.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&adv).await;
                            } else if req_head.contains("POST /lfs/upload-urls") {
                                // Extract body to parse requested hashes
                                let body_start = req_head.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
                                let body_bytes = &buf[body_start..n];
                                let req_json: crate::remote::types::LfsUploadUrlsReq =
                                    serde_json::from_slice(body_bytes).unwrap();

                                let mut urls = std::collections::HashMap::new();
                                for obj in req_json.objects {
                                    urls.insert(obj.hash.clone(), format!("{}/s3-upload/{}", base_url, obj.hash));
                                }
                                let resp_json = crate::remote::types::LfsUrlsResp { urls };
                                let resp_bytes = serde_json::to_vec(&resp_json).unwrap();

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    resp_bytes.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&resp_bytes).await;
                            } else if req_head.contains("PUT /s3-upload/") {
                                let body_start = req_head.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
                                let payload = buf[body_start..n].to_vec();
                                uploaded.lock().await.push(payload);

                                let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                                let _ = socket.write_all(resp.as_bytes()).await;
                            } else if req_head.contains("POST /git-receive-pack") {
                                let mut report = Vec::new();
                                report.extend_from_slice(&encode_str("unpack ok\n").unwrap());
                                report.extend_from_slice(&encode_str("ok refs/heads/main\n").unwrap());
                                report.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-receive-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    report.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&report).await;
                            }
                        });
                    }
                }
            }
        });

        let client = GitHttpClient::new();
        let push_res = client.push_branch(&server_url, &repo.path, "main").await.expect("push_branch");
        assert_eq!(push_res.branch, "main");
        assert_eq!(push_res.new_hash, commit_hash);

        // Verify that the LFS object was uploaded via HTTP PUT
        let payloads = uploaded_s3_payloads.lock().await;
        assert_eq!(payloads.len(), 1, "expected 1 LFS file uploaded to S3");
        assert!(!payloads[0].is_empty());
    }

    #[tokio::test]
    async fn test_clone_repo_downloads_lfs_objects() {
        let origin_repo = TempDirRepo::new("clone_origin");
        init_at(&origin_repo.path);
        fs::write(origin_repo.path.join(".git-med/lfs_track"), "*.dcm\n").unwrap();
        smart(&origin_repo.path, &["data"], 1, 1, 1, 1);
        add_in(&origin_repo.path).expect("origin add");
        let commit_hash = commit_with_message_in(&origin_repo.path, "origin commit").unwrap();

        // Collect packfile objects from origin_repo
        let objs = collect_objects_to_pack(&origin_repo.path, &commit_hash, &[]).unwrap();
        let slices: Vec<(ObjectType, &[u8])> = objs.iter().map(|(_, t, b)| (*t, b.as_slice())).collect();
        let pack_bytes = create_packfile_bytes(&slices).unwrap();

        // Read the raw LFS file content from origin_repo
        let lfs_raw_dir = origin_repo.path.join(".git-med/lfs/objects");
        let raw_entries: Vec<_> = fs::read_dir(&lfs_raw_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .flat_map(|prefix_dir| fs::read_dir(prefix_dir.path()).unwrap().filter_map(|e| e.ok()))
            .collect();
        assert!(!raw_entries.is_empty(), "must have staged raw LFS objects");
        let raw_lfs_bytes = fs::read(raw_entries[0].path()).unwrap();
        let raw_lfs_hash = crate::files::hash::hash_raw(&raw_lfs_bytes);

        // Spawn mock server providing upload-pack, lfs download-urls, and S3 GET
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let port = listener.local_addr().unwrap().port();
        let server_url = format!("http://127.0.0.1:{}", port);
        let (_shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let raw_bytes_for_server = raw_lfs_bytes.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accept_res = listener.accept() => {
                        let Ok((mut socket, _)) = accept_res else { break; };
                        let base_url = format!("http://127.0.0.1:{}", port);
                        let pack = pack_bytes.clone();
                        let raw_bytes = raw_bytes_for_server.clone();

                        tokio::spawn(async move {
                            let mut buf = vec![0u8; 16384];
                            let n = match socket.read(&mut buf).await {
                                Ok(n) if n > 0 => n,
                                _ => return,
                            };
                            let req = String::from_utf8_lossy(&buf[..n]);

                            if req.contains("GET /info/refs?service=git-upload-pack") {
                                let mut adv_body = Vec::new();
                                adv_body.extend_from_slice(&encode_str("version 2\n").unwrap());
                                adv_body.extend_from_slice(&encode_str("ls-refs\n").unwrap());
                                adv_body.extend_from_slice(&encode_str("fetch=shallow\n").unwrap());
                                adv_body.extend_from_slice(pkt_line::encode_flush());

                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-advertisement\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    adv_body.len()
                                );
                                let _ = socket.write_all(resp.as_bytes()).await;
                                let _ = socket.write_all(&adv_body).await;
                            } else if req.contains("POST /git-upload-pack") {
                                if req.contains("command=ls-refs") {
                                    let mut ls_body = Vec::new();
                                    let line1 = format!("{} HEAD symref-target:refs/heads/main\n", commit_hash.to_hex());
                                    let line2 = format!("{} refs/heads/main\n", commit_hash.to_hex());
                                    ls_body.extend_from_slice(&encode_str(&line1).unwrap());
                                    ls_body.extend_from_slice(&encode_str(&line2).unwrap());
                                    ls_body.extend_from_slice(pkt_line::encode_flush());

                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        ls_body.len()
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.write_all(&ls_body).await;
                                } else if req.contains("command=fetch") {
                                    let resp = format!(
                                        "HTTP/1.1 200 OK\r\nContent-Type: application/x-git-upload-pack-result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                        pack.len()
                                    );
                                    let _ = socket.write_all(resp.as_bytes()).await;
                                    let _ = socket.write_all(&pack).await;
                                }
                            } else if req.contains("POST /lfs/download-urls") {
                                let body_start = req.find("\r\n\r\n").map(|i| i + 4).unwrap_or(n);
                                let body_bytes = &buf[body_start..n];
                                let req_json: crate::remote::types::LfsDownloadUrlsReq =
                                    serde_json::from_slice(body_bytes).unwrap();

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
                            } else if req.contains("GET /s3-download/") {
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

        let target_repo = TempDirRepo::new("clone_dest");
        let client = GitHttpClient::new();
        client.clone_repo(&server_url, &target_repo.path).await.expect("clone_repo");

        // Verify raw LFS object exists in cloned repo's .git-med/lfs/objects/
        assert!(crate::files::lfs::has_lfs_raw(&target_repo.path, &raw_lfs_hash));

        // Verify materialized instance file in working directory matches the raw bytes
        let materialized_file = target_repo.path.join("data/clinic/subject_001/study_001/series_001/instance_001.dcm");
        assert!(materialized_file.is_file(), "materialized instance file must exist");
        let content = fs::read(&materialized_file).unwrap();
        assert_eq!(content, raw_lfs_bytes, "materialized file content must match downloaded raw LFS bytes");
    }
}

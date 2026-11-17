use std::path::Path;

use crate::remote::discovery::{
    DiscoveryHeader, build_v2_ls_refs_request, parse_initial_advertisement,
    parse_v2_ls_refs_response,
};
use crate::remote::negotiation;
use crate::remote::packfile;
use crate::remote::pkt_line;
use crate::remote::sideband;
use crate::remote::types::{FetchResult, RefAdvertisement, RemoteError};

/// HTTP transport client for interacting with remote Git servers.
#[derive(Debug, Clone)]
pub struct GitHttpClient {
    client: reqwest::Client,
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
        Self { client }
    }

    /// Create a `GitHttpClient` with a custom configured `reqwest::Client`.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
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

        let response = self
            .client
            .get(&info_refs_url)
            .header("Git-Protocol", "version=2")
            .header("Accept", "application/x-git-upload-pack-advertisement")
            .header("User-Agent", "git-med/0.1.0")
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let msg = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(RemoteError::HttpStatus {
                status: status.as_u16(),
                message: msg,
            });
        }

        let bytes = response.bytes().await?;
        let packets = pkt_line::decode_pkt_lines(&bytes)
            .map_err(|e| RemoteError::PktLine(e.to_string()))?;

        match parse_initial_advertisement(&packets)? {
            DiscoveryHeader::Version2(caps) => {
                let upload_pack_url = format!("{}/git-upload-pack", trimmed_url);
                let ls_refs_payload = build_v2_ls_refs_request()?;

                let post_response = self
                    .client
                    .post(&upload_pack_url)
                    .header("Git-Protocol", "version=2")
                    .header("Content-Type", "application/x-git-upload-pack-request")
                    .header("Accept", "application/x-git-upload-pack-result")
                    .header("User-Agent", "git-med/0.1.0")
                    .body(ls_refs_payload)
                    .send()
                    .await?;

                let post_status = post_response.status();
                if !post_status.is_success() {
                    let msg = post_response
                        .text()
                        .await
                        .unwrap_or_else(|_| "unknown error".into());
                    return Err(RemoteError::HttpStatus {
                        status: post_status.as_u16(),
                        message: msg,
                    });
                }

                let post_bytes = post_response.bytes().await?;
                let post_packets = pkt_line::decode_pkt_lines(&post_bytes)
                    .map_err(|e| RemoteError::PktLine(e.to_string()))?;

                parse_v2_ls_refs_response(caps, &post_packets)
            }
            DiscoveryHeader::Version1(advertisement) => Ok(advertisement),
        }
    }

    /// Negotiate and download objects from the remote repository, unpack them directly into
    /// `<repo_root>/.git-med/objects/`, and materialize references on disk.
    pub async fn fetch_and_unpack(
        &self,
        repo_url: &str,
        repo_root: &Path,
        advertisement: &RefAdvertisement,
    ) -> Result<FetchResult, RemoteError> {
        let trimmed_url = repo_url.trim_end_matches('/');

        // 1. Determine wanted object hashes from advertised references
        let mut wants = Vec::new();
        for r in &advertisement.refs {
            if !wants.contains(&r.hash) {
                wants.push(r.hash);
            }
        }

        if wants.is_empty() {
            return Ok(FetchResult {
                objects_unpacked: 0,
                default_branch: advertisement.default_branch.clone(),
                head_hash: None,
            });
        }

        // 2. Build negotiation payload without requesting delta compression
        let payload = if advertisement.protocol_version == 2 {
            negotiation::build_v2_fetch_request(&wants)?
        } else {
            negotiation::build_v1_fetch_request(&wants)?
        };

        // 3. POST /git-upload-pack
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

        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let msg = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(RemoteError::HttpStatus {
                status: status.as_u16(),
                message: msg,
            });
        }

        // 4. Demultiplex side-band 64k response and extract packfile bytes
        let body_bytes = response.bytes().await?;
        let pack_bytes = sideband::extract_packfile_bytes(&body_bytes)?;

        // 5. Unpack all base objects directly into loose objects in .git-med/objects/
        let objects_unpacked = packfile::unpack_packfile_into(&pack_bytes, repo_root)?;

        // 6. Materialize branch references and HEAD on disk
        let default_branch_clean = advertisement
            .default_branch
            .as_deref()
            .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b).to_string());

        let head_hash = advertisement.head_hash().or_else(|| {
            if let Some(ref branch) = default_branch_clean {
                let full_ref = format!("refs/heads/{}", branch);
                advertisement.get_ref(&full_ref).map(|r| r.hash)
            } else {
                advertisement.refs.first().map(|r| r.hash)
            }
        });

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
    /// 2. Performs reference discovery (`discover_refs`).
    /// 3. Negotiates and downloads the packfile, unpacking objects into `.git-med/objects/`.
    /// 4. Materializes local branches, remote tracking branches, and `HEAD`.
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
            std::fs::write(&view_file, "QOMRA\n")?;
        }

        let advertisement = self.discover_refs(repo_url).await?;
        self.fetch_and_unpack(repo_url, target_dir, &advertisement).await
    }
}

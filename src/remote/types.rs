use std::collections::HashMap;
use crate::files::hash::Hash;

/// A reference advertised by a remote Git server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    pub name: String,
    pub hash: Hash,
    pub symref_target: Option<String>,
    pub peeled: Option<Hash>,
}

/// Server capabilities negotiated or advertised during reference discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerCapabilities {
    pub raw: Vec<String>,
    pub symrefs: HashMap<String, String>,
    pub agent: Option<String>,
    pub ls_refs: bool,
    pub fetch: bool,
    pub ofs_delta: bool,
    pub side_band_64k: bool,
    pub thin_pack: bool,
}

impl ServerCapabilities {
    /// Parse capabilities from Protocol v1 or Protocol v2 capability lines.
    pub fn parse(cap_entries: &[&str]) -> Self {
        let mut caps = Self::default();
        for &entry in cap_entries {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            caps.raw.push(trimmed.to_string());

            if let Some(agent) = trimmed.strip_prefix("agent=") {
                caps.agent = Some(agent.to_string());
            } else if let Some(symref) = trimmed.strip_prefix("symref=") {
                // Format: symref=<source>:<target>
                if let Some((src, target)) = symref.split_once(':') {
                    caps.symrefs.insert(src.to_string(), target.to_string());
                }
            } else if trimmed == "ls-refs" || trimmed.starts_with("ls-refs=") {
                caps.ls_refs = true;
            } else if trimmed == "fetch" || trimmed.starts_with("fetch=") {
                caps.fetch = true;
            } else if trimmed == "ofs-delta" {
                caps.ofs_delta = true;
            } else if trimmed == "side-band-64k" || trimmed == "side-band" {
                caps.side_band_64k = true;
            } else if trimmed == "thin-pack" {
                caps.thin_pack = true;
            }
        }
        caps
    }
}

/// The complete advertisement returned by reference discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefAdvertisement {
    pub protocol_version: u8,
    pub service: String,
    pub capabilities: ServerCapabilities,
    pub default_branch: Option<String>,
    pub refs: Vec<RemoteRef>,
}

impl RefAdvertisement {
    /// Lookup a remote reference by its full name (e.g. "HEAD" or "refs/heads/main").
    pub fn get_ref(&self, name: &str) -> Option<&RemoteRef> {
        self.refs.iter().find(|r| r.name == name)
    }

    /// Lookup the HEAD reference commit hash if present.
    pub fn head_hash(&self) -> Option<Hash> {
        self.get_ref("HEAD").map(|r| r.hash)
    }
}

/// Result of a remote fetch or clone operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResult {
    /// Number of objects unpacked into `.git-med/objects/`.
    pub objects_unpacked: usize,
    /// Default branch name (e.g. "main" or "master").
    pub default_branch: Option<String>,
    /// Commit hash of the HEAD / default branch.
    pub head_hash: Option<Hash>,
}

/// Errors originating from remote Git HTTP transport or protocol negotiation.
#[derive(Debug)]
pub enum RemoteError {
    Http(reqwest::Error),
    Io(std::io::Error),
    PktLine(String),
    Protocol(String),
    HttpStatus { status: u16, message: String },
    InvalidHash(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP error: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::PktLine(msg) => write!(f, "pkt-line error: {}", msg),
            Self::Protocol(msg) => write!(f, "git protocol error: {}", msg),
            Self::HttpStatus { status, message } => {
                write!(f, "HTTP status {} error: {}", status, message)
            }
            Self::InvalidHash(msg) => write!(f, "invalid hash in remote ref: {}", msg),
        }
    }
}

impl std::error::Error for RemoteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for RemoteError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<std::io::Error> for RemoteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<flate2::DecompressError> for RemoteError {
    fn from(e: flate2::DecompressError) -> Self {
        Self::Protocol(format!("decompression error: {}", e))
    }
}

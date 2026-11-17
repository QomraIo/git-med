use crate::files::hash::Hash;
use crate::remote::pkt_line::{self, PktLine};
use crate::remote::types::{RefAdvertisement, RemoteError, RemoteRef, ServerCapabilities};

/// Protocol discovery outcome indicating whether server requires v2 ls-refs command.
#[derive(Debug)]
pub enum DiscoveryHeader {
    Version2(ServerCapabilities),
    Version1(RefAdvertisement),
}

/// Parse the initial GET `/info/refs?service=git-upload-pack` response packets.
pub fn parse_initial_advertisement(packets: &[PktLine]) -> Result<DiscoveryHeader, RemoteError> {
    if packets.is_empty() {
        return Err(RemoteError::Protocol("empty response from remote git server".into()));
    }

    let first_line = packets[0]
        .as_trimmed_str()
        .ok_or_else(|| RemoteError::Protocol("initial packet is not a valid text line".into()))?;

    // Check for Protocol v2 response: "version 2"
    if first_line.starts_with("version 2") {
        return parse_v2_capabilities(&packets[1..]).map(DiscoveryHeader::Version2);
    }

    // Check for Protocol v1 service announcement: "# service=git-upload-pack" or "# service=git-receive-pack"
    if first_line.starts_with("# service=git-upload-pack") {
        return parse_v1_advertisement(packets, "git-upload-pack").map(DiscoveryHeader::Version1);
    }
    if first_line.starts_with("# service=git-receive-pack") {
        return parse_v1_advertisement(packets, "git-receive-pack").map(DiscoveryHeader::Version1);
    }

    Err(RemoteError::Protocol(format!(
        "unrecognized remote git service announcement: '{}'",
        first_line
    )))
}

/// Parse Protocol v2 capability lines following "version 2" until the first Flush packet.
fn parse_v2_capabilities(packets: &[PktLine]) -> Result<ServerCapabilities, RemoteError> {
    let mut cap_strings: Vec<String> = Vec::new();

    for pkt in packets {
        if pkt.is_flush() {
            break;
        }
        if let Some(s) = pkt.as_trimmed_str() {
            if !s.is_empty() {
                cap_strings.push(s.to_string());
            }
        }
    }

    let cap_slices: Vec<&str> = cap_strings.iter().map(|s| s.as_str()).collect();
    Ok(ServerCapabilities::parse(&cap_slices))
}

/// Construct the Protocol v2 `command=ls-refs` payload.
pub fn build_v2_ls_refs_request() -> Result<Vec<u8>, RemoteError> {
    let mut body = Vec::new();
    body.extend_from_slice(&pkt_line::encode_str("command=ls-refs\n")?);
    body.extend_from_slice(&pkt_line::encode_str("agent=git-med/0.1.0\n")?);
    body.extend_from_slice(pkt_line::encode_delim());
    body.extend_from_slice(&pkt_line::encode_str("symrefs\n")?);
    body.extend_from_slice(&pkt_line::encode_str("peel\n")?);
    body.extend_from_slice(pkt_line::encode_flush());
    Ok(body)
}

/// Parse Protocol v2 `ls-refs` response stream into `RefAdvertisement`.
pub fn parse_v2_ls_refs_response(
    capabilities: ServerCapabilities,
    packets: &[PktLine],
) -> Result<RefAdvertisement, RemoteError> {
    let mut refs: Vec<RemoteRef> = Vec::new();
    let mut default_branch: Option<String> = None;

    for pkt in packets {
        if pkt.is_flush() || pkt.is_response_end() {
            break;
        }

        let line = match pkt.as_trimmed_str() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        // Format: <hash> <ref_name>[ symref-target:<target>][ peeled:<hash>]
        let mut tokens = line.split_whitespace();
        let hash_str = tokens
            .next()
            .ok_or_else(|| RemoteError::Protocol("missing hash in ls-refs line".into()))?;
        let ref_name = tokens
            .next()
            .ok_or_else(|| RemoteError::Protocol("missing ref name in ls-refs line".into()))?;

        let hash = Hash::from_hex(hash_str)
            .map_err(|e| RemoteError::InvalidHash(format!("{}: '{}'", e, hash_str)))?;

        let mut symref_target = None;
        let mut peeled = None;

        for attr in tokens {
            if let Some(target) = attr.strip_prefix("symref-target:") {
                symref_target = Some(target.to_string());
                if ref_name == "HEAD" {
                    default_branch = Some(target.to_string());
                }
            } else if let Some(p_hash_str) = attr.strip_prefix("peeled:") {
                let p_hash = Hash::from_hex(p_hash_str)
                    .map_err(|e| RemoteError::InvalidHash(format!("{}: '{}'", e, p_hash_str)))?;
                peeled = Some(p_hash);
            }
        }

        refs.push(RemoteRef {
            name: ref_name.to_string(),
            hash,
            symref_target,
            peeled,
        });
    }

    Ok(RefAdvertisement {
        protocol_version: 2,
        service: "git-upload-pack".to_string(),
        capabilities,
        default_branch,
        refs,
    })
}

/// Parse Protocol v1 reference advertisement stream.
fn parse_v1_advertisement(
    packets: &[PktLine],
    service_name: &str,
) -> Result<RefAdvertisement, RemoteError> {
    // Protocol v1 layout:
    // Packet 0: # service=git-upload-pack\n
    // Packet 1: Flush (0000)
    // Packet 2: <hash> <name>\0<capabilities>\n
    // Following: <hash> <name>\n or <hash> <name>^{}\n
    // Terminating: Flush (0000)

    if packets.len() < 2 || !packets[1].is_flush() {
        return Err(RemoteError::Protocol(
            "expected flush packet after Protocol v1 service header".into(),
        ));
    }

    let mut refs: Vec<RemoteRef> = Vec::new();
    let mut capabilities = ServerCapabilities::default();
    let mut default_branch: Option<String> = None;
    let mut is_first_ref = true;

    for pkt in &packets[2..] {
        if pkt.is_flush() {
            break;
        }

        let line_bytes = match pkt.as_bytes() {
            Some(b) if !b.is_empty() => b,
            _ => continue,
        };

        if is_first_ref {
            is_first_ref = false;

            // First ref contains \0 followed by space-separated capabilities
            let (ref_part, cap_part) = match line_bytes.iter().position(|&b| b == 0) {
                Some(idx) => (&line_bytes[..idx], Some(&line_bytes[idx + 1..])),
                None => (line_bytes, None),
            };

            if let Some(caps_raw) = cap_part {
                if let Ok(caps_str) = std::str::from_utf8(caps_raw) {
                    let cap_entries: Vec<&str> = caps_str.split_whitespace().collect();
                    capabilities = ServerCapabilities::parse(&cap_entries);
                    if let Some(target) = capabilities.symrefs.get("HEAD") {
                        default_branch = Some(target.clone());
                    }
                }
            }

            let ref_str = std::str::from_utf8(ref_part)
                .map_err(|e| RemoteError::Protocol(format!("invalid utf-8 in first ref: {}", e)))?
                .trim();

            if let Some((hash_str, name_str)) = ref_str.split_once(' ') {
                // Check for empty repository dummy ref (e.g. "capabilities^{}")
                if name_str != "capabilities^{}" {
                    let hash = Hash::from_hex(hash_str)
                        .map_err(|e| RemoteError::InvalidHash(format!("{}: '{}'", e, hash_str)))?;
                    let symref_target = capabilities.symrefs.get(name_str).cloned();
                    refs.push(RemoteRef {
                        name: name_str.to_string(),
                        hash,
                        symref_target,
                        peeled: None,
                    });
                }
            }
            continue;
        }

        let line_str = std::str::from_utf8(line_bytes)
            .map_err(|e| RemoteError::Protocol(format!("invalid utf-8 in ref line: {}", e)))?
            .trim();

        if let Some((hash_str, name_str)) = line_str.split_once(' ') {
            let hash = Hash::from_hex(hash_str)
                .map_err(|e| RemoteError::InvalidHash(format!("{}: '{}'", e, hash_str)))?;

            // Check for peeled tag: <hash> <name>^{}
            if let Some(tag_name) = name_str.strip_suffix("^{}") {
                if let Some(last_ref) = refs.iter_mut().find(|r| r.name == tag_name) {
                    last_ref.peeled = Some(hash);
                }
            } else {
                let symref_target = capabilities.symrefs.get(name_str).cloned();
                refs.push(RemoteRef {
                    name: name_str.to_string(),
                    hash,
                    symref_target,
                    peeled: None,
                });
            }
        }
    }

    Ok(RefAdvertisement {
        protocol_version: 1,
        service: service_name.to_string(),
        capabilities,
        default_branch,
        refs,
    })
}

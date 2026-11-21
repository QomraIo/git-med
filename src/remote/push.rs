use crate::files::hash::Hash;
use crate::remote::pkt_line;
use crate::remote::types::{PushRefUpdateResult, RemoteError};

/// Construct the HTTP POST payload for `/git-receive-pack`.
///
/// Layout:
/// - `<old_hex> <new_hex> <ref_name>\0report-status side-band-64k agent=git-med/0.1.0\n`
/// - `0000` (FLUSH)
/// - (Optional) Binary packfile bytes if new objects are being pushed
pub fn build_push_request(
    old_hash: Option<&Hash>,
    new_hash: &Hash,
    ref_name: &str,
    packfile_bytes: &[u8],
) -> Result<Vec<u8>, RemoteError> {
    let old_hex = old_hash
        .map(|h| h.to_hex())
        .unwrap_or_else(|| "0".repeat(64));
    let new_hex = new_hash.to_hex();

    let cmd_line = format!(
        "{} {} {}\0report-status side-band-64k agent=git-med/0.1.0\n",
        old_hex, new_hex, ref_name
    );

    let mut body = Vec::new();
    body.extend_from_slice(&pkt_line::encode_str(&cmd_line)?);
    body.extend_from_slice(pkt_line::encode_flush());

    // If there are objects to upload, append the binary packfile after the flush packet
    if !packfile_bytes.is_empty() {
        body.extend_from_slice(packfile_bytes);
    }

    Ok(body)
}

/// Parse the `report-status` response from a `POST /git-receive-pack` call.
///
/// Handles both side-band 64k multiplexed packets and plain pkt-lines.
/// Validates `unpack ok` and returns the update status for each reference.
pub fn parse_push_response(
    response_bytes: &[u8],
) -> Result<Vec<PushRefUpdateResult>, RemoteError> {
    let packets = pkt_line::decode_pkt_lines(response_bytes)
        .map_err(|e| RemoteError::PktLine(e.to_string()))?;

    let mut text_lines: Vec<String> = Vec::new();

    for pkt in &packets {
        if let Some(bytes) = pkt.as_bytes() {
            if bytes.is_empty() {
                continue;
            }

            // Check if line is multiplexed on side-band 64k
            match bytes[0] {
                1 => {
                    // Band 1: Status output text
                    if let Ok(s) = std::str::from_utf8(&bytes[1..]) {
                        for line in s.lines() {
                            text_lines.push(line.to_string());
                        }
                    }
                }
                2 => {
                    // Band 2: Remote progress message (ignore)
                }
                3 => {
                    // Band 3: Remote error message
                    let err_msg = String::from_utf8_lossy(&bytes[1..]).trim().to_string();
                    return Err(RemoteError::Protocol(format!("remote error: {}", err_msg)));
                }
                _ => {
                    // Standard pkt-line (no side-band byte prefix)
                    if let Ok(s) = std::str::from_utf8(bytes) {
                        for line in s.lines() {
                            text_lines.push(line.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut unpack_ok = false;
    let mut ref_results = Vec::new();

    for line in &text_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "unpack ok" {
            unpack_ok = true;
        } else if let Some(err) = trimmed.strip_prefix("unpack ") {
            return Err(RemoteError::Protocol(format!(
                "remote failed to unpack: {}",
                err
            )));
        } else if let Some(ok_ref) = trimmed.strip_prefix("ok ") {
            ref_results.push(PushRefUpdateResult {
                ref_name: ok_ref.trim().to_string(),
                success: true,
                error: None,
            });
        } else if let Some(ng_rest) = trimmed.strip_prefix("ng ") {
            let (ng_ref, reason) = ng_rest.split_once(' ').unwrap_or((ng_rest, "rejected"));
            ref_results.push(PushRefUpdateResult {
                ref_name: ng_ref.trim().to_string(),
                success: false,
                error: Some(reason.trim().to_string()),
            });
        }
    }

    // If report-status gave an unpack error or no ref result was parsed
    if !unpack_ok && ref_results.is_empty() && !text_lines.is_empty() {
        return Err(RemoteError::Protocol(format!(
            "unexpected response from receive-pack: {}",
            text_lines.join("; ")
        )));
    }

    Ok(ref_results)
}

use crate::remote::pkt_line::{self, PktLine};
use crate::remote::types::RemoteError;

/// Demultiplex side-band 64k pkt-lines into a contiguous packfile binary stream.
///
/// Channels:
/// - Band 1 (`0x01`): Packfile chunk bytes (concatenated)
/// - Band 2 (`0x02`): Progress information from remote (ignored)
/// - Band 3 (`0x03`): Server error message (returns `RemoteError::Protocol`)
///
/// Delimiters, flush packets, acknowledgments, and section headers (e.g. `packfile\n`)
/// are skipped.
pub fn extract_packfile_stream(packets: &[PktLine]) -> Result<Vec<u8>, RemoteError> {
    let mut packfile_data = Vec::new();

    for pkt in packets {
        let Some(bytes) = pkt.as_bytes() else {
            continue;
        };

        if bytes.is_empty() {
            continue;
        }

        match bytes[0] {
            1 => {
                // Band 1: packfile data chunk
                packfile_data.extend_from_slice(&bytes[1..]);
            }
            2 => {
                // Band 2: progress message (ignored)
            }
            3 => {
                // Band 3: remote error message
                let err_msg = String::from_utf8_lossy(&bytes[1..]).trim().to_string();
                return Err(RemoteError::Protocol(format!("remote server error: {}", err_msg)));
            }
            _ => {
                // Non-sideband control lines (e.g., "acknowledgments\n", "packfile\n", "NAK\n")
            }
        }
    }

    Ok(packfile_data)
}

/// Extract packfile bytes from raw HTTP response bytes, whether wrapped in side-band pkt-lines
/// or received as a raw `PACK` stream.
pub fn extract_packfile_bytes(raw_bytes: &[u8]) -> Result<Vec<u8>, RemoteError> {
    // If raw_bytes directly starts with "PACK", it is an unmultiplexed packfile.
    if raw_bytes.starts_with(b"PACK") {
        return Ok(raw_bytes.to_vec());
    }

    // Attempt to decode pkt-lines and demux sideband channels.
    if let Ok(packets) = pkt_line::decode_pkt_lines(raw_bytes) {
        let stream = extract_packfile_stream(&packets)?;
        if !stream.is_empty() {
            return Ok(stream);
        }
    }

    // Fallback: search for "PACK" signature anywhere in the byte stream.
    if let Some(pos) = raw_bytes.windows(4).position(|w| w == b"PACK") {
        return Ok(raw_bytes[pos..].to_vec());
    }

    Err(RemoteError::Protocol(
        "no packfile data found in server response".into(),
    ))
}

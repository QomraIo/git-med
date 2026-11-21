use crate::files::hash::Hash;
use crate::remote::pkt_line;
use crate::remote::types::RemoteError;

/// Build Git Protocol v2 fetch negotiation request.
///
/// Format:
/// - `command=fetch\n`
/// - `0001` (DELIM)
/// - `side-band-64k\n`
/// - `thin-pack\n`
/// - `want <hash>\n` (for each requested commit/object)
/// - `done\n`
/// - `0000` (FLUSH)
pub fn build_v2_fetch_request(wants: &[Hash]) -> Result<Vec<u8>, RemoteError> {
    let mut body = Vec::new();
    body.extend_from_slice(&pkt_line::encode_str("command=fetch\n")?);
    body.extend_from_slice(pkt_line::encode_delim());
    body.extend_from_slice(&pkt_line::encode_str("side-band-64k\n")?);
    body.extend_from_slice(&pkt_line::encode_str("thin-pack\n")?);

    for want in wants {
        let want_line = format!("want {}\n", want.to_hex());
        body.extend_from_slice(&pkt_line::encode_str(&want_line)?);
    }

    body.extend_from_slice(&pkt_line::encode_str("done\n")?);
    body.extend_from_slice(pkt_line::encode_flush());
    Ok(body)
}

/// Build Git Protocol v1 fetch negotiation request.
///
/// Format:
/// - `want <hash_0> side-band-64k thin-pack agent=git-med/0.1.0\n`
/// - `want <hash_i>\n`
/// - `0000` (FLUSH)
/// - `done\n`
pub fn build_v1_fetch_request(wants: &[Hash]) -> Result<Vec<u8>, RemoteError> {
    if wants.is_empty() {
        return Ok(Vec::new());
    }

    let mut body = Vec::new();
    let first_line = format!(
        "want {} side-band-64k thin-pack agent=git-med/0.1.0\n",
        wants[0].to_hex()
    );
    body.extend_from_slice(&pkt_line::encode_str(&first_line)?);

    for want in &wants[1..] {
        let want_line = format!("want {}\n", want.to_hex());
        body.extend_from_slice(&pkt_line::encode_str(&want_line)?);
    }

    body.extend_from_slice(pkt_line::encode_flush());
    body.extend_from_slice(&pkt_line::encode_str("done\n")?);
    Ok(body)
}

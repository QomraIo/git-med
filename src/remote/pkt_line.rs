use std::io::{self, Read, Write};

pub const MAX_PKT_LEN: usize = 65520;
pub const MAX_PAYLOAD_LEN: usize = MAX_PKT_LEN - 4; // 65516

/// A decoded packet-line from the Git transport layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PktLine {
    /// Standard payload packet: `<4-hex-len><data>`.
    Data(Vec<u8>),
    /// `0000` (FLUSH-PKT): Signals the end of a section or conversation.
    Flush,
    /// `0001` (DELIM-PKT): Used in Git Protocol v2 to separate sections.
    Delimiter,
    /// `0002` (RESPONSE-END-PKT): Used in Git Protocol v2 to signal end of response.
    ResponseEnd,
}

impl PktLine {
    /// Access payload bytes if this is a `Data` packet.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Data(d) => Some(d),
            _ => None,
        }
    }

    /// Access payload as a UTF-8 string slice if valid UTF-8.
    pub fn as_str(&self) -> Option<&str> {
        self.as_bytes().and_then(|b| std::str::from_utf8(b).ok())
    }

    /// Access payload as a trimmed UTF-8 string slice.
    pub fn as_trimmed_str(&self) -> Option<&str> {
        self.as_str().map(|s| s.trim())
    }

    /// Returns `true` if this is a Flush packet (`0000`).
    pub fn is_flush(&self) -> bool {
        matches!(self, Self::Flush)
    }

    /// Returns `true` if this is a Delimiter packet (`0001`).
    pub fn is_delimiter(&self) -> bool {
        matches!(self, Self::Delimiter)
    }

    /// Returns `true` if this is a ResponseEnd packet (`0002`).
    pub fn is_response_end(&self) -> bool {
        matches!(self, Self::ResponseEnd)
    }
}

// ============================================================================
// Encoding Functions
// ============================================================================

/// Encode binary payload into a complete pkt-line (`<4-hex-len><data>`).
pub fn encode_data(payload: &[u8]) -> io::Result<Vec<u8>> {
    if payload.len() > MAX_PAYLOAD_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "pkt-line payload length {} exceeds maximum {}",
                payload.len(),
                MAX_PAYLOAD_LEN
            ),
        ));
    }
    let total_len = payload.len() + 4;
    let mut out = Vec::with_capacity(total_len);
    let _ = write!(&mut out, "{:04x}", total_len);
    out.extend_from_slice(payload);
    Ok(out)
}

/// Encode a UTF-8 string into a complete pkt-line.
pub fn encode_str(s: &str) -> io::Result<Vec<u8>> {
    encode_data(s.as_bytes())
}

/// Flush packet raw bytes (`0000`).
pub fn encode_flush() -> &'static [u8; 4] {
    b"0000"
}

/// Delimiter packet raw bytes (`0001`).
pub fn encode_delim() -> &'static [u8; 4] {
    b"0001"
}

/// ResponseEnd packet raw bytes (`0002`).
pub fn encode_response_end() -> &'static [u8; 4] {
    b"0002"
}

/// Write a data pkt-line into a writer.
pub fn write_pkt_line<W: Write>(writer: &mut W, data: &[u8]) -> io::Result<()> {
    let encoded = encode_data(data)?;
    writer.write_all(&encoded)
}

/// Write a flush packet (`0000`) into a writer.
pub fn write_flush<W: Write>(writer: &mut W) -> io::Result<()> {
    writer.write_all(encode_flush())
}

/// Write a delimiter packet (`0001`) into a writer.
pub fn write_delim<W: Write>(writer: &mut W) -> io::Result<()> {
    writer.write_all(encode_delim())
}

// ============================================================================
// Decoding Functions
// ============================================================================

/// Read a single pkt-line from a `Read` stream.
/// Returns:
/// - `Ok(Some(PktLine))` on a successfully parsed packet
/// - `Ok(None)` on clean EOF at packet boundary
/// - `Err` on malformed length prefix or unexpected EOF
pub fn read_pkt_line<R: Read>(reader: &mut R) -> io::Result<Option<PktLine>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    match &len_buf {
        b"0000" => return Ok(Some(PktLine::Flush)),
        b"0001" => return Ok(Some(PktLine::Delimiter)),
        b"0002" => return Ok(Some(PktLine::ResponseEnd)),
        _ => {}
    }

    let hex_str = std::str::from_utf8(&len_buf).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "Invalid hex length header in pkt-line")
    })?;

    let len = usize::from_str_radix(hex_str, 16).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, format!("Invalid hex digits in header '{}'", hex_str))
    })?;

    if len < 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid pkt-line total length {} < 4", len),
        ));
    }
    if len > MAX_PKT_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pkt-line total length {} exceeds maximum {}", len, MAX_PKT_LEN),
        ));
    }

    let payload_len = len - 4;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;

    Ok(Some(PktLine::Data(payload)))
}

/// Decode all pkt-lines from an in-memory byte buffer.
pub fn decode_pkt_lines(bytes: &[u8]) -> io::Result<Vec<PktLine>> {
    let mut cursor = io::Cursor::new(bytes);
    let mut packets = Vec::new();
    while let Some(pkt) = read_pkt_line(&mut cursor)? {
        packets.push(pkt);
    }
    Ok(packets)
}

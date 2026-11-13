use std::path::Path;

pub fn get_name(opt_path: &Option<&Path>) -> Option<String> {
    opt_path
        .and_then(|p| p.file_name())
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

#[allow(dead_code)]
pub fn string2buf128(text: &str) -> [u8; 16] {
    let bytes = text.as_bytes();
    bytes2buf128(bytes)
}

#[allow(dead_code)]
pub fn bytes2buf128(bytes: &[u8]) -> [u8; 16] {
    let mut buf = [0u8; 16];
    let len = bytes.len().min(16);
    buf[..len].copy_from_slice(&bytes[..len]);
    buf
}

#[allow(dead_code)]
pub fn buf128_to_string(buf: &[u8; 16]) -> String {
    let len = buf.iter().position(|&b| b == 0).unwrap_or(16);
    String::from_utf8_lossy(&buf[..len]).to_string()
}

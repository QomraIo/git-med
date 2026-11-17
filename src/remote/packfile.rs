use std::path::Path;
use flate2::{Decompress, FlushDecompress, Status};

use crate::files::hash::hash_bytes;
use crate::files::types::ObjectType;
use crate::files::utils::make_object_from_bytes_in;
use crate::remote::types::RemoteError;

/// Unpack all base objects from a Git Packfile v2 byte stream directly into
/// `<repo_root>/.git-med/objects/` as loose Git-med objects.
///
/// Returns the number of objects unpacked.
/// Delta objects (`OBJ_OFS_DELTA` and `OBJ_REF_DELTA`) are rejected with `RemoteError::Protocol`.
pub fn unpack_packfile_into(pack_bytes: &[u8], repo_root: &Path) -> Result<usize, RemoteError> {
    if pack_bytes.len() < 12 {
        return Err(RemoteError::Protocol(
            "packfile payload is too short (< 12 bytes)".into(),
        ));
    }

    // 1. Verify 4-byte magic "PACK"
    if &pack_bytes[0..4] != b"PACK" {
        return Err(RemoteError::Protocol(format!(
            "invalid packfile magic: expected 'PACK', got {:?}",
            &pack_bytes[0..4]
        )));
    }

    // 2. Verify 4-byte version (must be 2)
    let version = u32::from_be_bytes(
        pack_bytes[4..8]
            .try_into()
            .map_err(|_| RemoteError::Protocol("failed to read packfile version".into()))?,
    );
    if version != 2 {
        return Err(RemoteError::Protocol(format!(
            "unsupported packfile version: expected 2, got {}",
            version
        )));
    }

    // 3. Number of objects
    let num_objects = u32::from_be_bytes(
        pack_bytes[8..12]
            .try_into()
            .map_err(|_| RemoteError::Protocol("failed to read object count".into()))?,
    ) as usize;

    let mut offset = 12;
    let mut unpacked_count = 0;

    for _ in 0..num_objects {
        if offset >= pack_bytes.len() {
            return Err(RemoteError::Protocol(
                "unexpected end of packfile data while reading object header".into(),
            ));
        }

        // 4. Decode LEB128 object header (type + uncompressed size)
        let first_byte = pack_bytes[offset];
        offset += 1;

        let obj_type_raw = (first_byte >> 4) & 0x07;
        let mut size = (first_byte & 0x0F) as usize;
        let mut shift = 4;

        let mut has_continuation = (first_byte & 0x80) != 0;
        while has_continuation {
            if offset >= pack_bytes.len() {
                return Err(RemoteError::Protocol(
                    "unexpected end of packfile data while decoding variable-length size".into(),
                ));
            }
            let next_byte = pack_bytes[offset];
            offset += 1;

            size |= ((next_byte & 0x7F) as usize) << shift;
            shift += 7;
            has_continuation = (next_byte & 0x80) != 0;
        }

        // Map object type
        let obj_type = match obj_type_raw {
            1 => ObjectType::Commit,
            2 => ObjectType::Tree,
            3 => ObjectType::Blob,
            6 | 7 => {
                return Err(RemoteError::Protocol(
                    "delta objects (OBJ_OFS_DELTA / OBJ_REF_DELTA) are not supported; packfile must contain base objects only".into(),
                ));
            }
            4 => {
                return Err(RemoteError::Protocol(
                    "OBJ_TAG packfile objects are not currently supported".into(),
                ));
            }
            other => {
                return Err(RemoteError::Protocol(format!(
                    "unknown packfile object type: {}",
                    other
                )));
            }
        };

        // 5. Decompress object payload via flate2
        let mut decompress = Decompress::new(true);
        let mut decompressed = Vec::with_capacity(size);
        let mut chunk_buf = [0u8; 64 * 1024];

        loop {
            let before_in = decompress.total_in() as usize;
            let before_out = decompress.total_out();

            if offset + before_in > pack_bytes.len() {
                return Err(RemoteError::Protocol(
                    "unexpected end of packfile data during zlib decompression".into(),
                ));
            }

            let status = decompress.decompress(
                &pack_bytes[offset + before_in..],
                &mut chunk_buf,
                FlushDecompress::Finish,
            )?;

            let written = (decompress.total_out() - before_out) as usize;
            decompressed.extend_from_slice(&chunk_buf[..written]);

            match status {
                Status::StreamEnd => break,
                Status::Ok | Status::BufError => {
                    if decompress.total_in() as usize == before_in && written == 0 {
                        return Err(RemoteError::Protocol(
                            "stalled decompression in packfile object stream".into(),
                        ));
                    }
                }
            }
        }

        offset += decompress.total_in() as usize;

        if decompressed.len() != size {
            return Err(RemoteError::Protocol(format!(
                "decompressed object size mismatch: expected {} bytes, got {}",
                size,
                decompressed.len()
            )));
        }

        // 6. Compute object hash and write directly into .git-med/objects/
        let digest = hash_bytes(&decompressed, obj_type);
        make_object_from_bytes_in(&decompressed, digest, obj_type, repo_root)?;

        unpacked_count += 1;
    }

    Ok(unpacked_count)
}

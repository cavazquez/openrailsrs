//! Decode MSTS on-disk files to UTF-8 S-expression text (UTF-16, SIMISA, binary tokens).

use std::path::Path;

use crate::encoding::{decode_msts_bytes, utf16le_msts_to_latin_bytes};
use crate::error::FormatError;
use crate::msts_simisa::decode_simisa_container;
use crate::shape_binary::binary_shape_to_ascii;

/// Decode raw MSTS file bytes to parseable text.
///
/// Handles UTF-16-LE wrappers, uncompressed SIMISA text, zlib-compressed SIMISA,
/// and binary JINX token streams (shapes and world tiles).
pub fn decode_msts_file_bytes(bytes: &[u8]) -> Result<String, FormatError> {
    let raw = utf16le_msts_to_latin_bytes(bytes).unwrap_or_else(|| bytes.to_vec());
    if raw.len() >= 6 && raw.starts_with(b"SIMISA") {
        let payload = decode_simisa_container(&raw)?;
        if payload.is_text {
            return Ok(decode_msts_bytes(&payload.bytes[payload.data_offset..]));
        }
        if is_terrain_binary_subheader(&payload.bytes) {
            return Err(FormatError::UnexpectedToken {
                offset: 0,
                message: "terrain binary (JINX0t*b) must be loaded via TerrainFile::from_path_with_coords"
                    .into(),
            });
        }
        return binary_shape_to_ascii(&payload);
    }
    Ok(decode_msts_bytes(bytes))
}

fn is_terrain_binary_subheader(body: &[u8]) -> bool {
    body.len() >= 8 && body.starts_with(b"JINX0t") && body.get(7) == Some(&b'b')
}

/// Read a file from disk and decode it with [`decode_msts_file_bytes`].
pub fn read_msts_file_decoded(path: &Path) -> Result<String, FormatError> {
    let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedToken {
        offset: 0,
        message: format!("failed to read {}: {e}", path.display()),
    })?;
    decode_msts_file_bytes(&bytes)
}

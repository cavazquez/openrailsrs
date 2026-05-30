//! SIMISA container handling for MSTS files (compressed and uncompressed).

use std::io::Read;

use crate::error::FormatError;

const HEADER_LEN: usize = 16;

/// Payload after the SIMISA header.  `bytes` starts just after the 16-byte
/// SIMISA header and includes the JINX sub-header.  `data_offset` points past
/// the JINX sub-header and any padding to the first data block.
pub struct SimisaPayload {
    pub bytes: Vec<u8>,
    /// `true` when sub-header ends with `t` (unicode text); `false` for `b` (binary).
    pub is_text: bool,
    /// Byte offset where content data starts (after JINX sub-header + padding).
    /// Binary shapes have `JINX0s1b_____\\r\\n\\r` (8+8=16 bytes) before the
    /// first block; text shapes have `JINX0s1t______\\r\\n` (8+8=16 bytes) before
    /// the S-expression text.
    pub data_offset: usize,
    /// Token ID offset for binary formats: 0 for shapes, 300 for world files.
    pub token_offset: i32,
}

/// Strip SIMISA wrapper and zlib-decompress (`SIMISA@F`) if present.
pub fn decode_simisa_container(bytes: &[u8]) -> Result<SimisaPayload, FormatError> {
    if bytes.len() < HEADER_LEN + 8 {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: "file too short for SIMISA header".into(),
        });
    }

    let header_prefix = std::str::from_utf8(&bytes[..8.min(bytes.len())])
        .unwrap_or("")
        .trim_end_matches('\0');

    let body: Vec<u8> = if header_prefix.starts_with("SIMISA@F") {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::UnexpectedToken {
                offset: 0,
                message: "truncated compressed SIMISA header".into(),
            });
        }
        let inflated = inflate_simisa_body(bytes)?;
        crate::encoding::msts_latin_bytes(&inflated)
    } else if header_prefix.starts_with("SIMISA@@") || header_prefix.starts_with("SIMISA") {
        bytes[HEADER_LEN..].to_vec()
    } else {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: format!("unrecognized SIMISA header: {header_prefix:?}"),
        });
    };

    if body.len() < 8 {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: "SIMISA body too short".into(),
        });
    }

    let sub = std::str::from_utf8(&body[..8.min(body.len())])
        .unwrap_or("")
        .trim_end_matches('\0');
    let is_text = sub.as_bytes().get(7) == Some(&b't');
    let is_binary = sub.as_bytes().get(7) == Some(&b'b');
    if !is_text && !is_binary {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: format!("unrecognized SIMISA sub-header: {sub:?}"),
        });
    }

    // Detect token offset for binary sub-types.
    // Shape binaries use the core token table directly; world binaries use
    // their own table range.
    let token_offset: i32 = if is_binary && sub.as_bytes().get(5) == Some(&b'w') {
        300
    } else {
        0
    };

    // After the 8-byte JINX sub-header, MSTS files have padding before the
    // first data block.  Binary shapes use `JINX0s1b` + `_____\r\n\r` (8
    // bytes of underscore+CR/LF padding).  Text files use `JINX0s1t` +
    // `______\r\n` or similar.  We scan past the padding to find where data
    // begins.  For binary, data starts at the first block header (token ID
    // in 0..131 or 300..430).  For text, data is the S-expression (first `(`).
    let data_offset = if is_text {
        // Find first '(' or letter — skip padding after JINX header
        find_text_start(&body)
    } else {
        // Find first binary block header (scan for a valid token ID)
        find_binary_start(&body, token_offset)
    };

    Ok(SimisaPayload {
        bytes: body,
        is_text,
        data_offset,
        token_offset,
    })
}

fn is_zlib_header(cmf: u8, flg: u8) -> bool {
    cmf == 0x78 && ((u16::from(cmf) << 8) | u16::from(flg)) % 31 == 0
}

fn inflate_simisa_body(bytes: &[u8]) -> Result<Vec<u8>, FormatError> {
    let mut starts = Vec::new();
    for fixed in [HEADER_LEN, HEADER_LEN + 2] {
        if fixed < bytes.len() {
            starts.push(fixed);
        }
    }
    let scan_end = bytes.len().min(128).saturating_sub(1);
    for i in 8..scan_end {
        if is_zlib_header(bytes[i], bytes[i + 1]) && !starts.contains(&i) {
            starts.push(i);
        }
    }

    let mut last_error = None;
    for start in starts {
        let mut decoder = flate2::read::ZlibDecoder::new(&bytes[start..]);
        let mut out = Vec::new();
        match decoder.read_to_end(&mut out) {
            Ok(_) if !out.is_empty() => return Ok(out),
            Ok(_) => {
                last_error = Some(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "empty zlib payload",
                ))
            }
            Err(e) => last_error = Some(e),
        }
    }

    Err(FormatError::UnexpectedToken {
        offset: 0,
        message: match last_error {
            Some(e) => format!("zlib inflate failed: {e}"),
            None => "zlib inflate failed: compressed payload is empty".into(),
        },
    })
}

/// Scan past the JINX sub-header and any padding to find the first S-expression
/// character (usually `(`).  Returns offset into `body`.
fn find_text_start(body: &[u8]) -> usize {
    let search_start = 8.min(body.len());
    for (i, &b) in body
        .iter()
        .enumerate()
        .take(body.len().min(search_start + 32))
        .skip(search_start)
    {
        if b == b'(' || b.is_ascii_alphabetic() {
            return i;
        }
    }
    search_start
}

/// Scan past the JINX sub-header and any padding to find the first binary block
/// header.  A valid block header has a u16 token whose ID (with offset) falls
/// in the known range, followed by plausible flags and remaining-bytes values.
fn find_binary_start(body: &[u8], token_offset: i32) -> usize {
    let search_start = 8.min(body.len());
    for i in search_start..body.len().min(search_start + 32) {
        if i + 8 > body.len() {
            break;
        }
        let token = u16::from_le_bytes([body[i], body[i + 1]]) as i32 + token_offset;
        if is_known_shape_or_world_token(token) {
            let remaining =
                u32::from_le_bytes([body[i + 4], body[i + 5], body[i + 6], body[i + 7]]) as usize;
            // Remaining must be reasonable (< body length) and leave room for
            // at least a label byte.
            let block_total = 8 + remaining;
            if block_total <= body.len() - i && remaining > 0 {
                return i;
            }
        }
    }
    search_start
}

/// Check whether a token ID is a known shape or world token.
fn is_known_shape_or_world_token(id: i32) -> bool {
    matches!(id, 1..=129 | 300..=430)
}

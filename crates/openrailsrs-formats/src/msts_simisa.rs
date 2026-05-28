//! SIMISA container handling for MSTS files (compressed and uncompressed).

use std::io::Read;

use crate::error::FormatError;

const HEADER_LEN: usize = 16;

/// Payload after the SIMISA + sub-header (ready for ASCII lexer or binary tokens).
pub struct SimisaPayload {
    pub bytes: Vec<u8>,
    /// `true` when sub-header ends with `t` (unicode text); `false` for `b` (binary).
    pub is_text: bool,
}

/// Strip SIMISA wrapper and zlib-compress (`SIMISA@F`) if present.
pub fn decode_simisa_container(bytes: &[u8]) -> Result<SimisaPayload, FormatError> {
    if bytes.len() < HEADER_LEN + 8 {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: "file too short for SIMISA header".into(),
        });
    }

    let header = std::str::from_utf8(&bytes[..HEADER_LEN.min(bytes.len())])
        .unwrap_or("")
        .trim_end_matches('\0');

    let body: Vec<u8> = if header.starts_with("SIMISA@F") {
        if bytes.len() < HEADER_LEN + 2 {
            return Err(FormatError::UnexpectedToken {
                offset: 0,
                message: "truncated compressed SIMISA header".into(),
            });
        }
        let deflate_in = &bytes[HEADER_LEN + 2..];
        let mut decoder = flate2::read::ZlibDecoder::new(deflate_in);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| FormatError::UnexpectedToken {
                offset: 0,
                message: format!("zlib inflate failed: {e}"),
            })?;
        out
    } else if header.starts_with("SIMISA@@") || header.starts_with("SIMISA") {
        bytes[HEADER_LEN..].to_vec()
    } else {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: format!("unrecognized SIMISA header: {header:?}"),
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

    Ok(SimisaPayload {
        bytes: body,
        is_text,
    })
}

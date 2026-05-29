//! Encoding detection and transcoding for MSTS-style text files.
//!
//! MSTS and older Open Rails content uses several encodings:
//!
//! | BOM bytes       | Encoding         | Common in                         |
//! |-----------------|------------------|-----------------------------------|
//! | `FF FE`         | UTF-16-LE        | Most MSTS route files             |
//! | `FE FF`         | UTF-16-BE        | Rare; some older tools            |
//! | `EF BB BF`      | UTF-8 with BOM   | Routes edited with modern editors |
//! | none            | Windows-1252     | Old European routes / Latin chars |
//! | none, ASCII     | UTF-8 / ASCII    | openrailsrs native files          |
//!
//! [`read_msts_file_to_string`] handles all five cases transparently and
//! always returns a valid UTF-8 `String` ready for the lexer.

use std::path::Path;

use crate::error::FormatError;

// ── BOM constants ─────────────────────────────────────────────────────────────

const BOM_UTF16_LE: [u8; 2] = [0xFF, 0xFE];
const BOM_UTF16_BE: [u8; 2] = [0xFE, 0xFF];
const BOM_UTF8: [u8; 3] = [0xEF, 0xBB, 0xBF];

// ── Public API ────────────────────────────────────────────────────────────────

/// Read a MSTS/Open Rails file from disk and return its contents as a UTF-8
/// `String`, applying the appropriate decoding.
///
/// The detection order is:
/// 1. UTF-16-LE (BOM `FF FE`)
/// 2. UTF-16-BE (BOM `FE FF`)
/// 3. UTF-8 with BOM (`EF BB BF`) — BOM is stripped
/// 4. Windows-1252 if no BOM but high bytes (`> 0x7F`) are present
/// 5. Plain UTF-8 / ASCII otherwise
pub fn read_msts_file_to_string(path: &Path) -> Result<String, FormatError> {
    let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedToken {
        offset: 0,
        message: format!("failed to read {}: {e}", path.display()),
    })?;
    Ok(decode_msts_bytes(&bytes))
}

/// Resolve `path` using a case-insensitive directory scan for each component.
///
/// MSTS content was authored on Windows (case-insensitive).  On Linux, file
/// extensions and folder names often differ in case from the paths stored
/// inside activity / service files (e.g. `.SRV` vs `.srv`, `.PAT` vs `.pat`).
/// This function walks every path component and does a case-insensitive
/// directory listing when the exact name is not found on disk.
///
/// Returns the resolved path if every component could be matched, or `None`
/// if any component does not exist even after the case-insensitive scan.
pub fn resolve_path_case_insensitive(path: &Path) -> Option<std::path::PathBuf> {
    use std::path::Component;
    let mut resolved = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {
                resolved.push(comp);
            }
            Component::ParentDir => {
                resolved.push("..");
            }
            Component::Normal(name) => {
                let name_str = name.to_string_lossy();
                let exact = resolved.join(&*name_str);
                if exact.exists() {
                    resolved = exact;
                    continue;
                }
                let lower = name_str.to_ascii_lowercase();
                let found = std::fs::read_dir(&resolved).ok()?.find_map(|e| {
                    let e = e.ok()?;
                    if e.file_name().to_string_lossy().to_ascii_lowercase() == lower {
                        Some(e.path())
                    } else {
                        None
                    }
                });
                resolved = found?;
            }
        }
    }
    Some(resolved)
}

/// Like [`read_msts_file_to_string`] but retries with a case-insensitive path
/// scan when the initial read fails.  This is useful for MSTS content where
/// file extensions or folder names may differ in case from the stored paths.
pub fn read_msts_file_case_insensitive(path: &Path) -> Result<String, FormatError> {
    if let Ok(text) = read_msts_file_to_string(path) {
        return Ok(text);
    }
    let resolved =
        resolve_path_case_insensitive(path).ok_or_else(|| FormatError::UnexpectedToken {
            offset: 0,
            message: format!(
                "failed to read {}: No such file or directory",
                path.display()
            ),
        })?;
    read_msts_file_to_string(&resolved)
}

/// Decode a byte slice using MSTS encoding heuristics.
///
/// This function is exposed for testing and for callers that already have the
/// raw bytes (e.g. when reading from a zip or in-memory buffer).
pub fn decode_msts_bytes(bytes: &[u8]) -> String {
    // UTF-16-LE BOM
    if bytes.starts_with(&BOM_UTF16_LE) {
        return decode_utf16_le(&bytes[2..]);
    }

    // UTF-16-BE BOM
    if bytes.starts_with(&BOM_UTF16_BE) {
        return decode_utf16_be(&bytes[2..]);
    }

    // UTF-8 BOM — strip it and return as UTF-8
    if bytes.starts_with(&BOM_UTF8) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }

    // No BOM: check for high bytes → Windows-1252 fallback
    if bytes.iter().any(|&b| b > 0x7F) {
        let (cow, _encoding, _had_errors) = encoding_rs::WINDOWS_1252.decode(bytes);
        return cow.into_owned();
    }

    // Pure ASCII / valid UTF-8
    String::from_utf8_lossy(bytes).into_owned()
}

/// If `bytes` is UTF-16-LE with BOM, collapse each code unit to its low byte.
///
/// MSTS stores SIMISA containers as UTF-16 where every ASCII character is
/// followed by a zero byte.  Decoding as UTF-16 produces a `String` that breaks
/// zlib/binary payloads; collapsing recovers the original byte stream.
pub fn utf16le_msts_to_latin_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(&BOM_UTF16_LE) {
        return None;
    }
    let payload = &bytes[2..];
    if payload.len() % 2 != 0 {
        return None;
    }
    Some(payload.chunks_exact(2).map(|pair| pair[0]).collect())
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn decode_utf16_le(bytes: &[u8]) -> String {
    let (cow, _encoding, _had_errors) = encoding_rs::UTF_16LE.decode(bytes);
    cow.into_owned()
}

fn decode_utf16_be(bytes: &[u8]) -> String {
    let (cow, _encoding, _had_errors) = encoding_rs::UTF_16BE.decode(bytes);
    cow.into_owned()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pure_ascii_roundtrips() {
        let src = b"( Train ( Name \"foo\" ) )";
        assert_eq!(decode_msts_bytes(src), "( Train ( Name \"foo\" ) )");
    }

    #[test]
    fn utf8_bom_stripped() {
        let mut src = BOM_UTF8.to_vec();
        src.extend_from_slice(b"hello");
        assert_eq!(decode_msts_bytes(&src), "hello");
    }

    #[test]
    fn utf16_le_decoded() {
        // Encode "AB" as UTF-16-LE with BOM
        let mut bytes: Vec<u8> = BOM_UTF16_LE.to_vec();
        for ch in "AB".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        assert_eq!(decode_msts_bytes(&bytes), "AB");
    }

    #[test]
    fn utf16_be_decoded() {
        let mut bytes: Vec<u8> = BOM_UTF16_BE.to_vec();
        for ch in "AB".encode_utf16() {
            bytes.extend_from_slice(&ch.to_be_bytes());
        }
        assert_eq!(decode_msts_bytes(&bytes), "AB");
    }

    #[test]
    fn windows1252_decoded() {
        // 0xE9 = 'é' in Windows-1252
        let bytes = b"caf\xe9";
        assert_eq!(decode_msts_bytes(bytes), "café");
    }
}

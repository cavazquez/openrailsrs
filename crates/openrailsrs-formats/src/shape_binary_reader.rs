//! Structured binary block reader for MSTS tokenized files (`.s`, `.w`, …).
//!
//! Mirrors the `SBR` class in Open Rails (`Source/Orts.Parsers.Msts/SBR.cs`).
//!
//! Each block in the binary stream has the layout:
//! ```text
//! u16  token_id       (subject to token_offset)
//! u16  flags          (unused by most consumers)
//! u32  remaining      (bytes after this header, including the label)
//! u8   label_len      (number of UTF-16 code units in the label)
//! u16* label          (label_len code units, little-endian UTF-16)
//! ...  body           (remaining - 1 - label_len*2 bytes)
//! ```
//!
//! The reader is **not** recursive by itself: callers drive parsing by
//! opening sub-blocks with [`BinaryBlockReader::open_sub_block`] and closing
//! them (or skipping) with [`BinaryBlockReader::skip_to_end`].

use crate::error::FormatError;

/// Structured binary block reader — one instance per open block scope.
///
/// The reader validates that reads stay within the declared `remaining` bytes.
/// Errors carry the absolute file offset, the token ID and any useful context.
pub struct BinaryBlockReader<'a> {
    /// Full binary payload (after the SIMISA header).
    data: &'a [u8],
    /// Current read position within `data`.
    pos: usize,
    /// End of the current block (exclusive).
    block_end: usize,
    /// Token offset applied to every raw token ID read from the stream.
    token_offset: i32,
    /// Absolute byte offset of `data[0]` within the original file (for diagnostics).
    base_offset: usize,
    /// Token ID of this block (for error messages).
    pub token_id: i32,
}

impl<'a> BinaryBlockReader<'a> {
    /// Create a root reader covering the entire payload slice.
    ///
    /// `base_offset` is the byte index within the original file where `data`
    /// starts (used only in error messages).
    pub fn new(data: &'a [u8], token_offset: i32, base_offset: usize) -> Self {
        Self {
            data,
            pos: 0,
            block_end: data.len(),
            token_offset,
            base_offset,
            token_id: -1,
        }
    }

    // ── position helpers ─────────────────────────────────────────────────────

    /// Absolute byte offset of the current position within the original file.
    #[inline]
    pub fn absolute_pos(&self) -> usize {
        self.base_offset + self.pos
    }

    /// Bytes remaining in the current block.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.block_end.saturating_sub(self.pos)
    }

    /// Whether the current block has been fully consumed.
    #[inline]
    pub fn end_of_block(&self) -> bool {
        self.pos >= self.block_end
    }

    // ── primitive reads ───────────────────────────────────────────────────────

    pub fn read_u8(&mut self) -> Result<u8, FormatError> {
        self.require(1)?;
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    pub fn read_u16(&mut self) -> Result<u16, FormatError> {
        self.require(2)?;
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    pub fn read_i32(&mut self) -> Result<i32, FormatError> {
        self.require(4)?;
        let v = i32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_u32(&mut self) -> Result<u32, FormatError> {
        self.require(4)?;
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        Ok(v)
    }

    pub fn read_f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    /// Read a length-prefixed UTF-16 LE string (u16 count, then count × 2 bytes).
    pub fn read_string_utf16(&mut self) -> Result<String, FormatError> {
        let count = self.read_u16()? as usize;
        if count == 0 {
            return Ok(String::new());
        }
        let byte_len = count * 2;
        self.require(byte_len)?;
        let mut utf16 = Vec::with_capacity(count);
        for i in 0..count {
            let lo = self.data[self.pos + i * 2];
            let hi = self.data[self.pos + i * 2 + 1];
            utf16.push(u16::from_le_bytes([lo, hi]));
        }
        self.pos += byte_len;
        String::from_utf16(&utf16).map_err(|e| self.err(format!("invalid UTF-16 string: {e}")))
    }

    /// Skip exactly `n` bytes within the current block.
    pub fn skip_bytes(&mut self, n: usize) -> Result<(), FormatError> {
        self.require(n)?;
        self.pos += n;
        Ok(())
    }

    /// Consume all remaining bytes in this block (equivalent to OR's `Skip()`).
    pub fn skip_to_end(&mut self) {
        self.pos = self.block_end;
    }

    // ── block navigation ──────────────────────────────────────────────────────

    /// Peek at the next token ID without consuming bytes.
    ///
    /// Returns `None` if there are fewer than 2 bytes remaining or the ID is
    /// out of range.
    pub fn peek_token_id(&self) -> Option<i32> {
        if self.pos + 2 > self.block_end || self.pos + 2 > self.data.len() {
            return None;
        }
        let raw = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]) as u32;
        (i32::try_from(raw).ok()).and_then(|t| t.checked_add(self.token_offset))
    }

    /// Open the next sub-block and return a child reader scoped to it.
    ///
    /// Reads the block header (token + flags + remaining + label) and returns a
    /// `BinaryBlockReader` whose `block_end` covers exactly the declared body.
    /// The parent's position is advanced past the header; the child's position
    /// starts at the first body byte.
    pub fn open_sub_block(&mut self) -> Result<BinaryBlockReader<'a>, FormatError> {
        // Read header: u16 token, u16 flags, u32 remaining
        let raw_token = self.read_u16()? as u32;
        let token_id = i32::try_from(raw_token)
            .ok()
            .and_then(|t| t.checked_add(self.token_offset))
            .ok_or_else(|| self.err(format!("token ID {raw_token} out of range")))?;
        let _flags = self.read_u16()?;
        let remaining = self.read_u32()? as usize;

        let header_end = self.pos; // position right after the 8-byte header
        let block_end = header_end
            .checked_add(remaining)
            .filter(|&e| e <= self.block_end && e <= self.data.len())
            .ok_or_else(|| {
                self.err(format!(
                    "block token={token_id} remaining={remaining} overruns parent (parent_end={}, data_len={})",
                    self.block_end,
                    self.data.len(),
                ))
            })?;

        // Read label: u8 label_len (in UTF-16 code units)
        let label_len = self.read_u8()? as usize;
        let label_bytes = label_len * 2;
        if self.pos + label_bytes > block_end {
            return Err(self.err(format!(
                "token={token_id} label ({label_len} code units) overruns block"
            )));
        }
        // Skip label bytes — consumers use `token_id` to dispatch, not the label.
        self.pos += label_bytes;

        let child_pos = self.pos;
        // Advance parent past this entire block.
        self.pos = block_end;

        Ok(BinaryBlockReader {
            data: self.data,
            pos: child_pos,
            block_end,
            token_offset: self.token_offset,
            base_offset: self.base_offset,
            token_id,
        })
    }

    /// Open the next sub-block **and** return its label as a separate `String`.
    ///
    /// Unlike [`open_sub_block`] (which skips the label), this decodes the
    /// UTF-16 label and returns it together with the child reader.
    pub fn open_sub_block_with_label(
        &mut self,
    ) -> Result<(BinaryBlockReader<'a>, String), FormatError> {
        let raw_token = self.read_u16()? as u32;
        let token_id = i32::try_from(raw_token)
            .ok()
            .and_then(|t| t.checked_add(self.token_offset))
            .ok_or_else(|| self.err(format!("token ID {raw_token} out of range")))?;
        let _flags = self.read_u16()?;
        let remaining = self.read_u32()? as usize;

        let header_end = self.pos;
        let block_end = header_end
            .checked_add(remaining)
            .filter(|&e| e <= self.block_end && e <= self.data.len())
            .ok_or_else(|| {
                self.err(format!(
                    "block token={token_id} remaining={remaining} overruns parent"
                ))
            })?;

        let label_len = self.read_u8()? as usize;
        let label_bytes = label_len * 2;
        if self.pos + label_bytes > block_end {
            return Err(self.err(format!("token={token_id} label overruns block")));
        }
        let label = if label_len > 0 {
            let mut utf16 = Vec::with_capacity(label_len);
            for i in 0..label_len {
                let lo = self.data[self.pos + i * 2];
                let hi = self.data[self.pos + i * 2 + 1];
                utf16.push(u16::from_le_bytes([lo, hi]));
            }
            self.pos += label_bytes;
            String::from_utf16(&utf16).unwrap_or_else(|_| format!("<label:{label_len}u>"))
        } else {
            String::new()
        };

        let child_pos = self.pos;
        self.pos = block_end;

        Ok((
            BinaryBlockReader {
                data: self.data,
                pos: child_pos,
                block_end,
                token_offset: self.token_offset,
                base_offset: self.base_offset,
                token_id,
            },
            label,
        ))
    }

    /// Whether there is at least one more sub-block header worth of bytes left.
    ///
    /// A valid block header is 8 bytes (token u16 + flags u16 + remaining u32) +
    /// 1 byte for label_len = 9 bytes minimum.
    pub fn has_more_blocks(&self) -> bool {
        self.pos + 9 <= self.block_end
    }

    // ── internal helpers ──────────────────────────────────────────────────────

    fn require(&self, n: usize) -> Result<(), FormatError> {
        if self.pos + n > self.block_end || self.pos + n > self.data.len() {
            return Err(FormatError::UnexpectedToken {
                offset: self.absolute_pos(),
                message: format!(
                    "token={} need {n} bytes but only {} remaining in block (data_len={})",
                    self.token_id,
                    self.remaining(),
                    self.data.len(),
                ),
            });
        }
        Ok(())
    }

    fn err(&self, message: impl Into<String>) -> FormatError {
        FormatError::UnexpectedToken {
            offset: self.absolute_pos(),
            message: message.into(),
        }
    }
}

// ── iterator-style helpers ────────────────────────────────────────────────────

impl<'a> BinaryBlockReader<'a> {
    /// Iterator-style: open sub-blocks one by one while there are bytes left.
    ///
    /// Returns `None` when `end_of_block()` is true or a header read fails.
    /// On failure, the parent reader position is left at the failing byte.
    pub fn next_sub_block(&mut self) -> Option<Result<BinaryBlockReader<'a>, FormatError>> {
        if self.end_of_block() {
            return None;
        }
        if !self.has_more_blocks() {
            return None;
        }
        Some(self.open_sub_block())
    }
}

// ── token ID helpers (shared with shape_binary.rs) ───────────────────────────

/// Apply a `token_offset` to a raw little-endian u16 token read from the stream.
pub fn apply_token_offset(raw: u16, token_offset: i32) -> Option<i32> {
    i32::try_from(raw as u32)
        .ok()
        .and_then(|t| t.checked_add(token_offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msts_simisa::decode_simisa_container;

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Build a minimal binary block: token + flags + remaining + label_len + body.
    fn make_block(token: u16, flags: u16, body: &[u8]) -> Vec<u8> {
        let remaining = 1 + body.len(); // 1 for label_len byte
        let mut out = Vec::new();
        out.extend_from_slice(&token.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&(remaining as u32).to_le_bytes());
        out.push(0u8); // label_len = 0
        out.extend_from_slice(body);
        out
    }

    /// Build a block with a UTF-16 label.
    fn make_block_with_label(token: u16, flags: u16, label: &str, body: &[u8]) -> Vec<u8> {
        let label_utf16: Vec<u16> = label.encode_utf16().collect();
        let label_bytes: Vec<u8> = label_utf16.iter().flat_map(|c| c.to_le_bytes()).collect();
        let remaining = 1 + label_bytes.len() + body.len();
        let mut out = Vec::new();
        out.extend_from_slice(&token.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&(remaining as u32).to_le_bytes());
        out.push(label_utf16.len() as u8);
        out.extend_from_slice(&label_bytes);
        out.extend_from_slice(body);
        out
    }

    // ── primitive tests ───────────────────────────────────────────────────────

    #[test]
    fn read_primitives_within_block() {
        // Build a simple block with body: i32(-42) + f32(PI) + u8(7)
        let mut body = Vec::new();
        body.extend_from_slice(&(-42i32).to_le_bytes());
        body.extend_from_slice(&std::f32::consts::PI.to_bits().to_le_bytes());
        body.push(7u8);

        let raw = make_block(71, 0, &body); // token 71 = shape
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let mut child = root.open_sub_block().expect("open root block");

        assert_eq!(child.token_id, 71);
        assert_eq!(child.read_i32().unwrap(), -42);
        assert!((child.read_f32().unwrap() - std::f32::consts::PI).abs() < 1e-5);
        assert_eq!(child.read_u8().unwrap(), 7);
        assert!(child.end_of_block());
    }

    #[test]
    fn empty_block_body() {
        let raw = make_block(2, 0, &[]); // token 2 = point, empty body
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let child = root.open_sub_block().expect("open empty block");
        assert_eq!(child.token_id, 2);
        assert!(child.end_of_block());
        assert_eq!(child.remaining(), 0);
    }

    #[test]
    fn utf16_label_decoded() {
        // Label "MAIN" in UTF-16 LE
        let label = "MAIN";
        let body = &[0u8, 0, 0, 0]; // 1 u32 = 0
        let raw = make_block_with_label(65, 0, label, body); // token 65 = matrix
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let (child, decoded_label) = root.open_sub_block_with_label().expect("open with label");
        assert_eq!(decoded_label, "MAIN");
        assert_eq!(child.token_id, 65);
    }

    #[test]
    fn token_offset_applied() {
        // raw token = 0, offset = 71 → effective token_id = 71 (shape)
        let raw = make_block(0, 0, &[]);
        let mut root = BinaryBlockReader::new(&raw, 71, 0);
        let child = root.open_sub_block().expect("open with offset");
        assert_eq!(child.token_id, 71);
    }

    #[test]
    fn nested_blocks() {
        // Outer block contains one inner block
        let inner_body = [1u8, 0, 0, 0]; // u32 = 1
        let inner = make_block(7, 0, &inner_body); // token 7 = points
        let outer = make_block(71, 0, &inner); // token 71 = shape

        let mut root = BinaryBlockReader::new(&outer, 0, 0);
        let mut shape_block = root.open_sub_block().expect("shape block");
        assert_eq!(shape_block.token_id, 71);

        let mut points_block = shape_block.open_sub_block().expect("points block");
        assert_eq!(points_block.token_id, 7);
        assert_eq!(points_block.read_u32().unwrap(), 1);
        assert!(points_block.end_of_block());
        assert!(shape_block.end_of_block());
    }

    #[test]
    fn skip_to_end_works() {
        let body = [0u8; 100];
        let raw = make_block(71, 0, &body);
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let mut child = root.open_sub_block().expect("block");
        assert_eq!(child.remaining(), 100);
        child.skip_to_end();
        assert!(child.end_of_block());
    }

    #[test]
    fn overrun_returns_error() {
        let body = [0u8; 3]; // only 3 bytes
        let raw = make_block(7, 0, &body);
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let mut child = root.open_sub_block().expect("block");
        let result = child.read_u32(); // needs 4 bytes
        assert!(result.is_err(), "should fail: only 3 bytes available");
    }

    #[test]
    fn read_string_utf16_empty() {
        // 2-byte count = 0
        let body = [0u8, 0];
        let raw = make_block(73, 0, &body); // token 73 = shader_name
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let mut child = root.open_sub_block().expect("block");
        let s = child.read_string_utf16().unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn read_string_utf16_ascii() {
        // "hi" → [0x68, 0x00, 0x69, 0x00] prefixed by count u16 = 2
        let mut body = Vec::new();
        body.extend_from_slice(&2u16.to_le_bytes()); // count = 2
        body.extend_from_slice(&(b'h' as u16).to_le_bytes());
        body.extend_from_slice(&(b'i' as u16).to_le_bytes());
        let raw = make_block(73, 0, &body);
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let mut child = root.open_sub_block().expect("block");
        let s = child.read_string_utf16().unwrap();
        assert_eq!(s, "hi");
    }

    #[test]
    fn has_more_blocks_false_on_short_remainder() {
        // Only 4 bytes body — not enough for a 9-byte sub-block header
        let body = [0u8; 4];
        let raw = make_block(71, 0, &body);
        let mut root = BinaryBlockReader::new(&raw, 0, 0);
        let child = root.open_sub_block().expect("block");
        assert!(!child.has_more_blocks());
    }

    #[test]
    fn chiltern_binary_shape_opens_via_bbr() {
        // Open one of the Chiltern binary shapes and read the root block token.
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s");
        if !path.is_file() {
            return;
        }
        let bytes = std::fs::read(&path).unwrap();
        let payload = decode_simisa_container(&bytes).unwrap();
        assert!(!payload.is_text, "DMBSA.s must be binary");
        let data = &payload.bytes[payload.data_offset..];
        let mut root = BinaryBlockReader::new(data, payload.token_offset, payload.data_offset);
        let shape_block = root.open_sub_block().expect("root shape block");
        // token 71 = shape (token_offset=0 for JINX0s1b)
        assert_eq!(
            shape_block.token_id, 71,
            "root block should be 'shape' (71), got {}",
            shape_block.token_id
        );
    }
}

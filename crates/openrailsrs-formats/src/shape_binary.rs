//! Binary tokenized MSTS shape (`.s` with `JINX0s1b`) → ASCII S-expression for [`super::shape::ShapeFile`].

use crate::error::FormatError;
use crate::msts_simisa::SimisaPayload;

/// Convert a binary shape payload (after SIMISA sub-header) to ASCII `( shape ... )` text.
pub fn binary_shape_to_ascii(payload: &SimisaPayload) -> Result<String, FormatError> {
    if payload.is_text {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: "binary_shape_to_ascii called on text payload".into(),
        });
    }
    let mut reader = BinaryReader::new(&payload.bytes, 0);
    let root = reader.dump_block()?;
    Ok(root)
}

struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
    token_offset: i32,
}

impl<'a> BinaryReader<'a> {
    fn new(data: &'a [u8], token_offset: i32) -> Self {
        Self {
            data,
            pos: 0,
            token_offset,
        }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Result<u8, FormatError> {
        if self.pos >= self.data.len() {
            return Err(FormatError::UnexpectedToken {
                offset: self.pos,
                message: "unexpected EOF reading u8".into(),
            });
        }
        let b = self.data[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_u16(&mut self) -> Result<u16, FormatError> {
        if self.remaining() < 2 {
            return Err(FormatError::UnexpectedToken {
                offset: self.pos,
                message: "unexpected EOF reading u16".into(),
            });
        }
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, FormatError> {
        if self.remaining() < 4 {
            return Err(FormatError::UnexpectedToken {
                offset: self.pos,
                message: "unexpected EOF reading u32".into(),
            });
        }
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn read_f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    fn read_string(&mut self) -> Result<String, FormatError> {
        let count = self.read_u16()? as usize;
        if count == 0 {
            return Ok(String::new());
        }
        let byte_len = count * 2;
        if self.remaining() < byte_len {
            return Err(FormatError::UnexpectedToken {
                offset: self.pos,
                message: "unexpected EOF reading string".into(),
            });
        }
        let mut utf16 = Vec::with_capacity(count);
        for i in 0..count {
            let lo = self.data[self.pos + i * 2];
            let hi = self.data[self.pos + i * 2 + 1];
            utf16.push(u16::from_le_bytes([lo, hi]));
        }
        self.pos += byte_len;
        String::from_utf16(&utf16).map_err(|e| FormatError::UnexpectedToken {
            offset: self.pos,
            message: format!("invalid UTF-16 string: {e}"),
        })
    }

    fn peek_subblock_header(&self, block_end: usize) -> bool {
        if self.pos + 8 > block_end || self.pos + 8 > self.data.len() {
            return false;
        }
        let token = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        let remaining = u32::from_le_bytes([
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ]) as usize;
        let token_id = token as i32 + self.token_offset;
        if !(0..=500).contains(&token_id) {
            return false;
        }
        remaining + 8 <= block_end.saturating_sub(self.pos)
    }

    fn dump_block(&mut self) -> Result<String, FormatError> {
        let token = self.read_u16()?;
        let _flags = self.read_u16()?;
        let remaining = self.read_u32()? as usize;
        let block_end = self.pos.saturating_add(remaining);
        if block_end > self.data.len() {
            return Err(FormatError::UnexpectedToken {
                offset: self.pos,
                message: format!(
                    "block overruns file (need {block_end}, have {})",
                    self.data.len()
                ),
            });
        }

        let label_len = self.read_u8()? as usize;
        let label = if label_len > 0 {
            let byte_len = label_len * 2;
            if self.pos + byte_len > block_end {
                return Err(FormatError::UnexpectedToken {
                    offset: self.pos,
                    message: "label overruns block".into(),
                });
            }
            let mut utf16 = Vec::with_capacity(label_len);
            for i in 0..label_len {
                let lo = self.data[self.pos + i * 2];
                let hi = self.data[self.pos + i * 2 + 1];
                utf16.push(u16::from_le_bytes([lo, hi]));
            }
            self.pos += byte_len;
            Some(
                String::from_utf16(&utf16).map_err(|e| FormatError::UnexpectedToken {
                    offset: self.pos,
                    message: format!("invalid label UTF-16: {e}"),
                })?,
            )
        } else {
            None
        };

        let name = token_name(token as i32 + self.token_offset);
        let mut out = String::from("( ");
        out.push_str(name);
        if let Some(ref l) = label {
            out.push_str(" \"");
            out.push_str(l);
            out.push('"');
        }

        while self.pos < block_end {
            if self.peek_subblock_header(block_end) {
                out.push(' ');
                out.push_str(&self.dump_block()?);
            } else if self.try_read_string_in_block(block_end, &mut out)? {
                // string appended
            } else if self.remaining() >= 4 && self.pos + 4 <= block_end {
                let f = self.read_f32()?;
                out.push(' ');
                out.push_str(&format_float(f as f64));
            } else {
                break;
            }
        }

        self.pos = block_end;
        out.push_str(" )");
        Ok(out)
    }

    fn try_read_string_in_block(
        &mut self,
        block_end: usize,
        out: &mut String,
    ) -> Result<bool, FormatError> {
        if self.pos + 2 > block_end {
            return Ok(false);
        }
        let count = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]) as usize;
        let byte_len = 2 + count * 2;
        if count == 0 || self.pos + byte_len > block_end {
            return Ok(false);
        }
        // Avoid mistaking a subblock header for a zero-length string.
        if count == 0 {
            return Ok(false);
        }
        let saved = self.pos;
        if let Ok(s) = self.read_string() {
            if !s.is_empty() && s.chars().all(|c| !c.is_control() || c == ' ') {
                out.push(' ');
                out.push('"');
                out.push_str(&s.replace('\\', "\\\\").replace('"', "\\\""));
                out.push('"');
                return Ok(true);
            }
        }
        self.pos = saved;
        Ok(false)
    }
}

fn format_float(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{:.0}", v.round())
    } else {
        format!("{v:.6}")
    }
}

fn token_name(id: i32) -> &'static str {
    match id {
        0 => "error",
        1 => "comment",
        2 => "point",
        3 => "vector",
        4 => "quat",
        5 => "normals",
        6 => "normal_idxs",
        7 => "points",
        8 => "uv_point",
        9 => "uv_points",
        10 => "colour",
        11 => "colours",
        12 => "packed_colour",
        13 => "image",
        14 => "images",
        15 => "texture",
        16 => "textures",
        17 => "light_material",
        18 => "light_materials",
        35 => "lod_controls",
        36 => "lod_control",
        37 => "distance_levels_header",
        38 => "distance_level_header",
        39 => "dlevel_selection",
        40 => "distance_levels",
        41 => "distance_level",
        42 => "sub_objects",
        43 => "sub_object",
        44 => "sub_object_header",
        49 => "vertices",
        52 => "primitives",
        53 => "prim_state",
        54 => "prim_states",
        55 => "prim_state_idx",
        58 => "indexed_trilist",
        59 => "tex_idxs",
        61 => "vertex_idxs",
        63 => "matrix",
        64 => "matrices",
        66 => "volumes",
        67 => "vol_sphere",
        68 => "shape_header",
        69 => "shape",
        70 => "shader_names",
        71 => "shader_name",
        72 => "texture_filenames",
        _ => "_unknown",
    }
}

#[cfg(test)]
mod tests {
    use crate::msts_simisa::decode_simisa_container;

    #[test]
    fn minimal_ascii_shape_still_parses_via_container() {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.s"),
        )
        .unwrap();
        let payload = decode_simisa_container(&bytes).unwrap();
        assert!(payload.is_text);
    }
}

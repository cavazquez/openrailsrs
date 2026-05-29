//! Binary tokenized MSTS shape (`.s` with `JINX0s1b`) → ASCII S-expression for [`super::shape::ShapeFile`].

use crate::error::FormatError;
use crate::msts_simisa::SimisaPayload;

/// Convert a binary shape payload to ASCII `( shape ... )` text.
pub fn binary_shape_to_ascii(payload: &SimisaPayload) -> Result<String, FormatError> {
    if payload.is_text {
        return Err(FormatError::UnexpectedToken {
            offset: 0,
            message: "binary_shape_to_ascii called on text payload".into(),
        });
    }
    if payload.data_offset > payload.bytes.len() {
        return Err(FormatError::UnexpectedToken {
            offset: payload.data_offset,
            message: "binary payload offset is past end of body".into(),
        });
    }
    let mut reader = BinaryReader::new(
        &payload.bytes[payload.data_offset..],
        payload.token_offset,
        payload.data_offset,
    );
    let root = reader.dump_block()?;
    Ok(root)
}

struct BinaryReader<'a> {
    data: &'a [u8],
    pos: usize,
    token_offset: i32,
    base_offset: usize,
}

impl<'a> BinaryReader<'a> {
    fn new(data: &'a [u8], token_offset: i32, base_offset: usize) -> Self {
        Self {
            data,
            pos: 0,
            token_offset,
            base_offset,
        }
    }

    fn absolute_pos(&self) -> usize {
        self.base_offset + self.pos
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn read_u8(&mut self) -> Result<u8, FormatError> {
        if self.pos >= self.data.len() {
            return Err(FormatError::UnexpectedToken {
                offset: self.absolute_pos(),
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
                offset: self.absolute_pos(),
                message: "unexpected EOF reading u16".into(),
            });
        }
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_token_id(&mut self) -> Result<i32, FormatError> {
        let raw = self.read_u16()? as u32;
        self.map_token_id(raw)
    }

    fn map_token_id(&self, raw: u32) -> Result<i32, FormatError> {
        let token = i32::try_from(raw).map_err(|_| FormatError::UnexpectedToken {
            offset: self.absolute_pos(),
            message: "binary token ID is out of range".into(),
        })?;
        token
            .checked_add(self.token_offset)
            .ok_or_else(|| FormatError::UnexpectedToken {
                offset: self.absolute_pos(),
                message: "binary token ID overflow".into(),
            })
    }

    fn read_u32(&mut self) -> Result<u32, FormatError> {
        if self.remaining() < 4 {
            return Err(FormatError::UnexpectedToken {
                offset: self.absolute_pos(),
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
                offset: self.absolute_pos(),
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
            offset: self.absolute_pos(),
            message: format!("invalid UTF-16 string: {e}"),
        })
    }

    fn peek_subblock_header(&self, block_end: usize) -> bool {
        self.peek_subblock_header_at(self.pos, block_end)
    }

    fn peek_subblock_header_at(&self, pos: usize, block_end: usize) -> bool {
        if pos + 8 > block_end || pos + 8 > self.data.len() {
            return false;
        }
        let token = u16::from_le_bytes([self.data[pos], self.data[pos + 1]]) as u32;
        let remaining = u32::from_le_bytes([
            self.data[pos + 4],
            self.data[pos + 5],
            self.data[pos + 6],
            self.data[pos + 7],
        ]) as usize;
        let Ok(token_id) = self.map_token_id(token) else {
            return false;
        };
        if !is_known_shape_token(token_id) || remaining == 0 {
            return false;
        }
        remaining + 8 <= block_end.saturating_sub(pos)
            && self.block_label_is_plausible(pos, remaining)
    }

    fn peek_token_id_at(&self, pos: usize) -> Option<i32> {
        if pos + 2 > self.data.len() {
            return None;
        }
        let token = u16::from_le_bytes([self.data[pos], self.data[pos + 1]]) as u32;
        self.map_token_id(token).ok()
    }

    fn block_label_is_plausible(&self, pos: usize, remaining: usize) -> bool {
        let Some(label_pos) = pos.checked_add(8) else {
            return false;
        };
        let Some(block_end) = label_pos.checked_add(remaining) else {
            return false;
        };
        let Some(&label_len) = self.data.get(label_pos) else {
            return false;
        };
        label_pos + 1 + label_len as usize * 2 <= block_end
    }

    fn try_read_count_before_subblocks(
        &mut self,
        parent_token_id: i32,
        block_end: usize,
        out: &mut String,
    ) -> Result<bool, FormatError> {
        if self.pos + 12 > block_end {
            return Ok(false);
        }
        let count = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        let count_child = self.peek_token_id_at(self.pos + 4);
        if count_child.is_some_and(|child| is_expected_collection_child(parent_token_id, child))
            && count <= 100_000
            && self.peek_subblock_header_at(self.pos + 4, block_end)
        {
            self.pos += 4;
            out.push(' ');
            out.push_str(&count.to_string());
            return Ok(true);
        }
        if self.peek_subblock_header(block_end) {
            return Ok(false);
        }
        if count <= 100_000 && self.peek_subblock_header_at(self.pos + 4, block_end) {
            self.pos += 4;
            out.push(' ');
            out.push_str(&count.to_string());
            return Ok(true);
        }
        Ok(false)
    }

    fn dump_block(&mut self) -> Result<String, FormatError> {
        let token_id = self.read_token_id()?;
        let _flags = self.read_u16()?;
        let remaining = self.read_u32()? as usize;
        let block_end = self.pos.saturating_add(remaining);
        if block_end > self.data.len() {
            return Err(FormatError::UnexpectedToken {
                offset: self.absolute_pos(),
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
                    offset: self.absolute_pos(),
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
            let label = String::from_utf16(&utf16).map_err(|e| FormatError::UnexpectedToken {
                offset: self.absolute_pos(),
                message: format!("invalid label UTF-16: {e}"),
            })?;
            if is_safe_ascii_text(&label) {
                Some(label)
            } else {
                None
            }
        } else {
            None
        };

        let name = token_name(token_id);
        let mut out = String::from("( ");
        out.push_str(name);
        if let Some(ref l) = label {
            out.push_str(" \"");
            out.push_str(l);
            out.push('"');
        }

        while self.pos < block_end {
            if self.try_read_count_before_subblocks(token_id, block_end, &mut out)? {
                // count appended
            } else if self.peek_subblock_header(block_end) {
                let saved = self.pos;
                match self.dump_block() {
                    Ok(block) => {
                        out.push(' ');
                        out.push_str(&block);
                    }
                    Err(_) => {
                        self.pos = saved;
                        self.append_scalar(token_id, block_end, &mut out)?;
                    }
                }
            } else if self.try_read_string_in_block(block_end, &mut out)? {
                // string appended
            } else if self.pos + 4 <= block_end {
                self.append_scalar(token_id, block_end, &mut out)?;
            } else {
                break;
            }
        }

        self.pos = block_end;
        out.push_str(" )");
        Ok(out)
    }

    fn append_scalar(
        &mut self,
        token_id: i32,
        block_end: usize,
        out: &mut String,
    ) -> Result<(), FormatError> {
        if self.pos + 4 > block_end {
            self.pos = block_end;
            return Ok(());
        }
        out.push(' ');
        if token_scalars_are_i32(token_id) {
            let n = self.read_u32()? as i32;
            out.push_str(&n.to_string());
        } else {
            let f = self.read_f32()?;
            out.push_str(&format_float(f as f64));
        }
        Ok(())
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
        if count == 0 || count > 512 || self.pos + byte_len > block_end {
            return Ok(false);
        }
        // Avoid mistaking a subblock header for a zero-length string.
        if count == 0 {
            return Ok(false);
        }
        let saved = self.pos;
        if let Ok(s) = self.read_string() {
            if !s.is_empty() && is_safe_ascii_text(&s) {
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

fn is_safe_ascii_text(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii() && (!c.is_control() || c == ' '))
}

fn format_float(v: f64) -> String {
    if !v.is_finite() || v.abs() > 1.0e12 {
        return "0".into();
    }
    if (v - v.round()).abs() < 1e-6 {
        format!("{:.0}", v.round())
    } else {
        format!("{v:.6}")
    }
}

fn token_scalars_are_i32(id: i32) -> bool {
    matches!(
        id,
        48  // vertex: flags, point index, normal index, colors
            | 49 // vertex_uvs
            | 54 // prim_state: flags, shader index, vertex state, etc.
            | 56 // prim_state_idx
            | 61 // tex_idxs
            | 63 // vertex_idxs
            | 64 // flags
            | 70 // shape_header
    )
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
        31 => "lod_controls",
        32 => "lod_control",
        33 => "distance_levels_header",
        34 => "distance_level_header",
        35 => "dlevel_selection",
        36 => "distance_levels",
        37 => "distance_level",
        38 => "sub_objects",
        39 => "sub_object",
        40 => "sub_object_header",
        41 => "geometry_info",
        42 => "geometry_nodes",
        43 => "geometry_node",
        44 => "geometry_node_map",
        45 => "cullable_prims",
        46 => "vtx_state",
        47 => "vtx_states",
        48 => "vertex",
        49 => "vertex_uvs",
        50 => "vertices",
        51 => "vertex_set",
        52 => "vertex_sets",
        53 => "primitives",
        54 => "prim_state",
        55 => "prim_states",
        56 => "prim_state_idx",
        60 => "indexed_trilist",
        61 => "tex_idxs",
        63 => "vertex_idxs",
        64 => "flags",
        65 => "matrix",
        66 => "matrices",
        67 => "hierarchy",
        68 => "volumes",
        69 => "vol_sphere",
        70 => "shape_header",
        71 => "shape",
        72 => "shader_names",
        73 => "shader_name",
        74 => "texture_filter_names",
        75 => "texture_filter_name",
        76 => "sort_vectors",
        79 => "light_model_cfgs",
        80 => "light_model_cfg",
        81 => "uv_ops",
        125 => "named_filter_mode",
        129 => "named_shader",
        _ => "_unknown",
    }
}

fn is_known_shape_token(id: i32) -> bool {
    matches!(
        id,
        1..=18
            | 31..=56
            | 60
            | 61
            | 63..=76
            | 79..=81
            | 125
            | 129
    )
}

fn is_expected_collection_child(parent: i32, child: i32) -> bool {
    matches!(
        (parent, child),
        (5, 3)    // normals -> vector
            | (7, 2)  // points -> point
            | (9, 8)  // uv_points -> uv_point
            | (11, 10) // colours -> colour
            | (14, 13) // images -> image
            | (16, 15) // textures -> texture
            | (18, 17) // light_materials -> light_material
            | (31, 32) // lod_controls -> lod_control
            | (36, 37) // distance_levels -> distance_level
            | (38, 39) // sub_objects -> sub_object
            | (42, 43) // geometry_nodes -> geometry_node
            | (47, 46) // vtx_states -> vtx_state
            | (50, 48) // vertices -> vertex
            | (52, 51) // vertex_sets -> vertex_set
            | (53, 56) // primitives -> prim_state_idx
            | (53, 60) // primitives -> indexed_trilist
            | (55, 54) // prim_states -> prim_state
            | (66, 65) // matrices -> matrix
            | (68, 69) // volumes -> vol_sphere
            | (72, 129) // shader_names -> named_shader
            | (74, 125) // texture_filter_names -> named_filter_mode
    )
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

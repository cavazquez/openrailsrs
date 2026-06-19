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

    fn peek_subblock_header(&self, parent_token_id: i32, block_end: usize) -> bool {
        self.peek_subblock_header_for_parent(parent_token_id, self.pos, block_end)
    }

    fn peek_subblock_header_for_parent(
        &self,
        parent_token_id: i32,
        pos: usize,
        block_end: usize,
    ) -> bool {
        if is_scalar_only_leaf_block(parent_token_id) {
            return false;
        }
        if !self.peek_subblock_header_at(pos, block_end) {
            return false;
        }
        let Some(child) = self.peek_token_id_at(pos) else {
            return false;
        };
        if parent_token_id == 60 {
            // indexed_trilist → vertex_idxs | normal_idxs | flags
            return matches!(child, 63 | 6 | 64);
        }
        if is_schema_collection_parent(parent_token_id) {
            return is_expected_collection_child(parent_token_id, child);
        }
        true
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
        if !is_known_binary_token(token_id, self.token_offset) || remaining == 0 {
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
            && self.peek_subblock_header_for_parent(parent_token_id, self.pos + 4, block_end)
        {
            self.pos += 4;
            out.push(' ');
            out.push_str(&count.to_string());
            return Ok(true);
        }
        if self.peek_subblock_header(parent_token_id, block_end) {
            return Ok(false);
        }
        if count_child.is_some_and(|child| is_expected_collection_child(parent_token_id, child))
            && count <= 100_000
            && self.peek_subblock_header_for_parent(parent_token_id, self.pos + 4, block_end)
        {
            self.pos += 4;
            out.push(' ');
            out.push_str(&count.to_string());
            return Ok(true);
        }
        Ok(false)
    }

    /// Blocks whose body is only numeric scalars (Open Rails never nests sub-blocks here).
    fn dump_vertex_idxs_content(
        &mut self,
        block_end: usize,
        out: &mut String,
    ) -> Result<(), FormatError> {
        if self.pos + 4 > block_end {
            return Ok(());
        }
        let total = self.read_u32()? as i32;
        out.push(' ');
        out.push_str(&total.to_string());
        for _ in 0..total.max(0) {
            if self.pos + 4 > block_end {
                break;
            }
            out.push(' ');
            out.push_str(&(self.read_u32()? as i32).to_string());
        }
        Ok(())
    }

    fn dump_normal_idxs_content(
        &mut self,
        block_end: usize,
        out: &mut String,
    ) -> Result<(), FormatError> {
        if self.pos + 4 > block_end {
            return Ok(());
        }
        let count = self.read_u32()? as i32;
        out.push(' ');
        out.push_str(&count.to_string());
        for _ in 0..count.max(0) {
            if self.pos + 4 > block_end {
                break;
            }
            out.push(' ');
            out.push_str(&(self.read_u32()? as i32).to_string());
            // Open Rails skips the constant `3` after each normal index.
            if self.pos + 4 <= block_end {
                self.read_u32()?;
            }
        }
        Ok(())
    }

    fn dump_flags_content(
        &mut self,
        block_end: usize,
        out: &mut String,
    ) -> Result<(), FormatError> {
        if self.pos + 4 > block_end {
            return Ok(());
        }
        let count = self.read_u32()? as i32;
        out.push(' ');
        out.push_str(&count.to_string());
        for _ in 0..count.max(0) {
            if self.pos + 4 > block_end {
                break;
            }
            out.push(' ');
            out.push_str(&self.read_u32()?.to_string());
        }
        Ok(())
    }

    /// Open Rails `texture`: `:uint,ImageIdx :uint,FilterMode :float,MipMapLODBias [:dword,BorderColor]`.
    fn dump_texture_content(
        &mut self,
        block_end: usize,
        out: &mut String,
    ) -> Result<(), FormatError> {
        if self.pos + 4 > block_end {
            return Ok(());
        }
        out.push(' ');
        out.push_str(&(self.read_u32()? as i32).to_string());
        if self.pos + 4 > block_end {
            return Ok(());
        }
        out.push(' ');
        out.push_str(&(self.read_u32()? as i32).to_string());
        if self.pos + 4 > block_end {
            return Ok(());
        }
        out.push(' ');
        out.push_str(&format_float(self.read_f32()? as f64));
        if self.pos + 4 <= block_end {
            out.push(' ');
            out.push_str(&self.read_u32()?.to_string());
        }
        Ok(())
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

        // OR reads these blocks as flat scalar runs, never nested sub-blocks.
        match token_id {
            63 => {
                self.dump_vertex_idxs_content(block_end, &mut out)?;
                self.pos = block_end;
                out.push_str(" )");
                return Ok(out);
            }
            6 => {
                self.dump_normal_idxs_content(block_end, &mut out)?;
                self.pos = block_end;
                out.push_str(" )");
                return Ok(out);
            }
            64 => {
                self.dump_flags_content(block_end, &mut out)?;
                self.pos = block_end;
                out.push_str(" )");
                return Ok(out);
            }
            15 => {
                self.dump_texture_content(block_end, &mut out)?;
                self.pos = block_end;
                out.push_str(" )");
                return Ok(out);
            }
            60 => {
                while self.pos < block_end
                    && self.peek_subblock_header_for_parent(token_id, self.pos, block_end)
                {
                    out.push(' ');
                    out.push_str(&self.dump_block()?);
                }
                self.pos = block_end;
                out.push_str(" )");
                return Ok(out);
            }
            // Open Rails reads `shape` children in fixed order; `animations` is optional last.
            71 => {
                for _ in 0..17 {
                    if self.pos >= block_end || !self.peek_subblock_header_at(self.pos, block_end) {
                        break;
                    }
                    out.push(' ');
                    out.push_str(&self.dump_block()?);
                }
                if self.pos < block_end
                    && self.peek_subblock_header_at(self.pos, block_end)
                    && self.peek_token_id_at(self.pos) == Some(90)
                {
                    out.push(' ');
                    out.push_str(&self.dump_block()?);
                }
                self.pos = block_end;
                out.push_str(" )");
                return Ok(out);
            }
            _ => {}
        }

        while self.pos < block_end {
            if self.try_read_count_before_subblocks(token_id, block_end, &mut out)? {
                // count appended
            } else if self.peek_subblock_header(token_id, block_end) {
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
            | 67 // hierarchy
            | 70 // shape_header
            // World tokens whose payload is u32 (not f32):
            | 401 // StaticDetailLevel
            | 404 // StaticFlags
            | 405 // CollideFlags
            | 408 // UiD
            | 419 // SectionIdx
            | 500 // Population
            | 583 // VDbId
            | 584 // VDbIdCount
            | 922 // TrItemId
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
        90 => "animations",
        91 => "animation",
        92 => "anim_nodes",
        93 => "anim_node",
        94 => "controllers",
        95 => "tcb_rot",
        96 => "linear_pos",
        97 => "slerp_rot",
        99 => "tcb_key",
        101 => "linear_key",
        103 => "slerp_key",
        125 => "named_filter_mode",
        129 => "named_shader",
        // World tokens: raw binary value + 300, matching Open Rails `TokenID.cs`.
        303 => "Static",
        305 => "TrackObj",
        308 => "Forest",
        311 => "CollideObject",
        317 => "Signal",
        360 => "Platform",
        362 => "LevelCr",
        364 => "Speedpost",
        365 => "Hazard",
        375 => "Tr_Worldfile",
        376 => "Tr_Watermark",
        395 => "FileName",
        396 => "FileNames",
        397 => "Position",
        398 => "Direction",
        399 => "MaxVisDistance",
        400 => "Quality",
        401 => "StaticDetailLevel",
        404 => "StaticFlags",
        405 => "CollideFlags",
        408 => "UiD",
        409 => "TrackSections",
        410 => "TrackSection",
        419 => "SectionIdx",
        420 => "SectionCurve",
        424 => "JNodePosn",
        458 => "SignalSubObj",
        486 => "SignalUnits",
        487 => "SignalUnit",
        493 => "Elevation",
        500 => "Population",
        501 => "Area",
        503 => "ScaleRange",
        579 => "ViewDbSphere",
        580 => "Radius",
        583 => "VDbId",
        584 => "VDbIdCount",
        598 => "Matrix3x3",
        922 => "TrItemId",
        945 => "QDirection",
        1107 => "PlatformData",
        1111 => "SpeedRange",
        1112 => "PickupType",
        1113 => "PickupAnimData",
        1114 => "PickupCapacity",
        1116 => "CarFrequency",
        1117 => "CarAvSpeed",
        1120 => "SidingData",
        1122 => "LevelCrParameters",
        1123 => "LevelCrData",
        1124 => "LevelCrTiming",
        1131 => "Speed_Sign_Shape",
        1134 => "Speed_Digit_Tex",
        1139 => "Speed_Text_Size",
        1152 => "Width",
        1153 => "Height",
        1154 => "TreeTexture",
        1155 => "TreeSize",
        1531 => "CrashProbability",
        1540 => "CarSpawner",
        1541 => "Siding",
        1542 => "Dyntrack",
        1543 => "Transfer",
        1544 => "Gantry",
        1545 => "Pickup",
        1561 => "Length",
        1562 => "Flipped",
        1563 => "Ruler",
        _ if (300..=430).contains(&id) => "_world",
        _ => "_unknown",
    }
}

fn is_known_binary_token(id: i32, token_offset: i32) -> bool {
    if token_offset >= 300 {
        // World tokens above 430 (orientation, forest, signal, dyntrack, …)
        // per Open Rails `TokenID.cs`; without them `QDirection`/`Matrix3x3`
        // blocks in binary `.w` tiles are dropped and objects lose rotation.
        return matches!(
            id,
            300..=430
                | 458
                | 486
                | 487
                | 493
                | 500
                | 501
                | 503
                | 579
                | 580
                | 583
                | 584
                | 598
                | 922
                | 945
                | 1107
                | 1111..=1117
                | 1120..=1124
                | 1131
                | 1134
                | 1139
                | 1152..=1155
                | 1531
                | 1540..=1545
                | 1561..=1563
        );
    }
    matches!(
        id,
        1..=18
            | 31..=56
            | 60
            | 61
            | 63..=76
            | 79..=81
            | 90..=97
            | 99
            | 101
            | 103
            | 125
            | 129
    )
}

fn is_scalar_only_leaf_block(token_id: i32) -> bool {
    matches!(
        token_id,
        2 | 3 | 6 | 8 | 56 | 63 | 64 | 67 | 99 | 101 | 103 // point, vector, normal_idxs, uv_point, prim_state_idx, vertex_idxs, flags, hierarchy, anim_keys
    )
}

fn is_schema_collection_parent(parent: i32) -> bool {
    matches!(
        parent,
        5 | 7
            | 9
            | 11
            | 14
            | 16
            | 18
            | 31
            | 36
            | 38
            | 42
            | 47
            | 50
            | 52
            | 53
            | 55
            | 60
            | 66
            | 68
            | 72
            | 74
            | 90 // animations
            | 92 // anim_nodes
            | 94 // controllers
            | 95 // tcb_rot
            | 96 // linear_pos
            | 97 // slerp_rot
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
            | (60, 63) // indexed_trilist -> vertex_idxs
            | (60, 6) // indexed_trilist -> normal_idxs
            | (60, 64) // indexed_trilist -> flags
            | (66, 65) // matrices -> matrix
            | (68, 69) // volumes -> vol_sphere
            | (72, 129) // shader_names -> named_shader
            | (74, 125) // texture_filter_names -> named_filter_mode
            | (90, 91) // animations -> animation
            | (92, 93) // anim_nodes -> anim_node
            | (94, 95) // controllers -> tcb_rot
            | (94, 96) // controllers -> linear_pos
            | (94, 97) // controllers -> slerp_rot
            | (95, 99) // tcb_rot -> tcb_key
            | (96, 101) // linear_pos -> linear_key
            | (97, 103) // slerp_rot -> slerp_key
    )
}

#[cfg(test)]
mod tests {
    use crate::ShapeFile;
    use crate::msts_simisa::decode_simisa_container;
    use crate::shape_binary::binary_shape_to_ascii;

    #[test]
    fn minimal_ascii_shape_still_parses_via_container() {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal.s"),
        )
        .unwrap();
        let payload = decode_simisa_container(&bytes).unwrap();
        assert!(payload.is_text);
    }

    #[test]
    fn dmb_sa_exterior_vertex_uv_indices_diversity() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s");
        if !path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&path).expect("parse");
        let level = &shape.lod_controls[0].distance_levels[0];
        let mut uv_idx_set = std::collections::HashSet::new();
        for sub in &level.sub_objects {
            for v in &sub.vertices {
                if let Some(&ui) = v.uv_indices.first() {
                    if ui >= 0 {
                        uv_idx_set.insert(ui);
                    }
                }
            }
        }
        assert!(
            uv_idx_set.len() > 100,
            "exterior shape should have many UV indices, got {}",
            uv_idx_set.len()
        );
    }

    #[test]
    fn pullman_cab_vertex_uv_indices_are_populated() {
        let path = std::path::PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&path).expect("parse cab");
        let level = shape.lod_controls[0]
            .distance_levels
            .iter()
            .min_by(|a, b| {
                a.selection_m
                    .partial_cmp(&b.selection_m)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .expect("lod");
        let mut uv_idx_set = std::collections::HashSet::new();
        for sub in &level.sub_objects {
            for v in &sub.vertices {
                if let Some(&ui) = v.uv_indices.first() {
                    if ui >= 0 {
                        uv_idx_set.insert(ui);
                    }
                }
            }
        }
        assert!(
            uv_idx_set.len() > 1000,
            "cab shape should have many distinct UV indices, got {}",
            uv_idx_set.len()
        );
    }

    #[test]
    fn chiltern_tree_shape_vertex_indices_are_sane() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/SHAPES/POPLAR15.S");
        if !path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&path).expect("parse POPLAR15");
        let prim = &shape.lod_controls[0].distance_levels[0].sub_objects[0].primitives[0];
        assert!(
            prim.vertex_indices.iter().all(|&idx| idx < 16),
            "tree shape vertex indices must be small table indices, got {:?}",
            prim.vertex_indices
        );
    }

    #[test]
    fn parse_shape_with_animations() {
        let ascii = r#"
        SIMISA@@@@@@@@@@JINX0s1t______

        ( shape
            ( animations 1
                ( animation 30 30
                    ( anim_nodes 1
                        ( anim_node "BOGIE1"
                            ( controllers 1
                                ( linear_pos 2
                                    ( linear_key 0.0 1.0 2.0 3.0 )
                                    ( linear_key 1.0 4.0 5.0 6.0 )
                                )
                            )
                        )
                    )
                )
            )
        )
        "#;
        let ast = crate::parser::parse_first_from_first_paren(ascii).expect("parse AST");
        let shape = ShapeFile::from_ast(&ast).expect("parse ShapeFile");
        assert_eq!(shape.animations.len(), 1);
        let anim = &shape.animations[0];
        assert_eq!(anim.frame_count, 30);
        assert_eq!(anim.nodes.len(), 1);
        let node = &anim.nodes[0];
        assert_eq!(node.name, "BOGIE1");
        assert_eq!(node.controllers.len(), 1);
        match &node.controllers[0] {
            crate::AnimController::LinearPos { keys } => {
                assert_eq!(keys.len(), 2);
                assert_eq!(keys[0], (0.0, [1.0, 2.0, 3.0]));
                assert_eq!(keys[1], (1.0, [4.0, 5.0, 6.0]));
            }
            _ => panic!("Expected LinearPos controller"),
        }
    }

    #[test]
    fn pullman_cab_geometry_and_anim_diagnostics() {
        let path = std::path::PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(&path).expect("parse cab");
        eprintln!(
            "Pullman: matrices={} animations={} anim_nodes={}",
            shape.matrices.len(),
            shape.animations.len(),
            shape.animations.first().map(|a| a.nodes.len()).unwrap_or(0)
        );
        if let Some(anim) = shape.animations.first() {
            for (i, node) in anim.nodes.iter().enumerate() {
                if !node.controllers.is_empty() {
                    eprintln!(
                        "  anim_node {i} {} ctrl={}",
                        node.name,
                        node.controllers.len()
                    );
                }
            }
        }
        let payload = crate::msts_simisa::decode_simisa_container(&std::fs::read(&path).unwrap())
            .expect("decode");
        let ascii = binary_shape_to_ascii(&payload).expect("ascii");
        eprintln!(
            "ascii has geometry_node_map={}",
            ascii.contains("geometry_node_map")
        );
        eprintln!("ascii has animations={}", ascii.contains("animations"));
        // Dump geometry_node_map per sub_object from raw ASCII.
        for (sub_idx, chunk) in ascii.split("( sub_object").skip(1).enumerate() {
            if let Some(map_start) = chunk.find("( geometry_node_map") {
                let rest = &chunk[map_start..];
                if let Some(end) = rest.find(')') {
                    let inner = &rest["( geometry_node_map".len()..end];
                    let nums: Vec<i32> = inner
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    if nums.len() > 1 {
                        let count = nums[0] as usize;
                        let values = &nums[1..];
                        let active: Vec<(usize, i32)> = values
                            .iter()
                            .take(count)
                            .enumerate()
                            .filter(|(_, v)| **v >= 0)
                            .map(|(i, &v)| (i, v))
                            .collect();
                        if !active.is_empty() {
                            eprintln!("sub_object {sub_idx} geometry_node_map active: {active:?}");
                        }
                    }
                }
            }
        }
        for (n, _chunk) in ascii.match_indices("geometry_node_map") {
            let snippet = &ascii[n..n + 80.min(ascii.len() - n)];
            eprintln!("gnm@{n}: {snippet}");
            if n > 50000 {
                break;
            }
        }
        if let Some(level) = shape
            .lod_controls
            .first()
            .and_then(|c| c.distance_levels.first())
        {
            eprintln!("hierarchy: {:?}", level.hierarchy);
        }
    }

    #[test]
    fn chiltern_water_column_has_animations_if_present() {
        let path = std::path::PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/SHAPES/RF_GW_WaterColumn.s",
        );
        if !path.is_file() {
            eprintln!("skip: RF_GW_WaterColumn.s no disponible");
            return;
        }
        let shape = ShapeFile::from_path(&path).expect("parse water column");
        eprintln!(
            "water column: matrices={} animations={}",
            shape.matrices.len(),
            shape.animations.len()
        );
    }

    #[test]
    fn chiltern_water_column_shape_child_tokens() {
        let path = std::path::PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/SHAPES/RF_GW_WaterColumn.s",
        );
        if !path.is_file() {
            return;
        }
        let bytes = std::fs::read(&path).unwrap();
        let payload = crate::msts_simisa::decode_simisa_container(&bytes).unwrap();
        let data = &payload.bytes[payload.data_offset..];
        // root shape block header
        if data.len() < 8 {
            return;
        }
        let root_tok = u16::from_le_bytes([data[0], data[1]]) as i32 + payload.token_offset;
        let root_rem = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        eprintln!("root token={root_tok} rem={root_rem}");
        let mut pos = 8 + 1 + (data[8] as usize) * 2; // skip label
        let block_end = 8 + root_rem;
        let mut children = Vec::new();
        while pos + 8 <= block_end && pos + 8 <= data.len() {
            let tok = u16::from_le_bytes([data[pos], data[pos + 1]]) as i32 + payload.token_offset;
            let rem =
                u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize;
            if rem == 0 || rem > block_end.saturating_sub(pos) {
                break;
            }
            children.push(tok);
            let label_len = data.get(pos + 8).copied().unwrap_or(0) as usize;
            pos += 8 + 1 + label_len * 2 + rem;
        }
        eprintln!("shape children: {children:?}");
    }
}

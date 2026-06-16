//! Descriptor `.sd` junto a cada shape (flags de textura MSTS/Open Rails).

use std::path::Path;

use openrailsrs_formats::msts_file_text::decode_msts_file_bytes;

/// Flags leídos de `ESD_Alternative_Texture` en el `.sd` del shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapeDescriptor {
    /// Valor crudo `ESD_Alternative_Texture` (bitfield OR `TextureFlags`).
    pub alternative_texture: u32,
    /// `ESD_SubObj` presente: sub-objeto 1 es geometría nocturna.
    pub has_night_subobj: bool,
}

impl ShapeDescriptor {
    pub fn load_for_shape(shape_path: &Path) -> Self {
        let sd_path = shape_path.with_extension("sd");
        let Ok(bytes) = std::fs::read(&sd_path) else {
            return Self::default();
        };
        let Ok(text) = decode_msts_file_bytes(&bytes) else {
            return Self::default();
        };
        let lower = text.to_ascii_lowercase();
        Self {
            alternative_texture: parse_esd_int(&text, "esd_alternative_texture"),
            has_night_subobj: lower.contains("esd_subobj"),
        }
    }
}

fn parse_esd_int(text: &str, field: &str) -> u32 {
    let lower = text.to_ascii_lowercase();
    let Some(pos) = lower.find(field) else {
        return 0;
    };
    let tail = &text[pos..];
    let Some(open) = tail.find('(') else {
        return 0;
    };
    let rest = &tail[open + 1..];
    for token in rest.split(|c: char| !c.is_ascii_digit() && c != '-') {
        if let Ok(v) = token.parse::<i64>() {
            if v >= 0 {
                return v as u32;
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_chiltern_pullman_sd() {
        let shape = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s");
        if !shape.is_file() {
            return;
        }
        let desc = ShapeDescriptor::load_for_shape(&shape);
        assert_eq!(desc.alternative_texture, 0);
        assert!(!desc.has_night_subobj);
    }
}

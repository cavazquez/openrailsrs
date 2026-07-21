//! Minimal visual `sigcfg.dat` loader for signal heads and lamp quads (#37).
//!
//! Scan-based (like `carspawn.dat`) because MSTS uses `Name ( … )` blocks rather
//! than canonical S-expressions. Scripts / NumClearAhead are out of scope.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::FormatError;
use crate::msts_file_text::read_msts_file_decoded;

/// UV rectangle for a light texture atlas entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LightTextureDef {
    pub name: String,
    pub texture_file: String,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// Named colour from `LightsTab` (`colour ( a r g b )`, 0–255).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightColour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl LightColour {
    pub fn to_linear_rgb(self) -> [f32; 3] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
        ]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalLightDef {
    pub index: u32,
    pub colour_name: String,
    /// MSTS local position (metres) before OR X-flip / Bevy Z-flip.
    pub position: [f32; 3],
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalDrawStateDef {
    pub name: String,
    pub draw_lights: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalTypeDef {
    pub name: String,
    pub light_texture: Option<String>,
    pub lights: Vec<SignalLightDef>,
    pub draw_states: Vec<SignalDrawStateDef>,
    /// `(aspect_name, draw_state_name)` e.g. `STOP` → `Red`.
    pub aspects: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalShapeSubObjDef {
    pub index: u32,
    pub matrix_name: String,
    pub signal_sub_type: Option<String>,
    pub signal_type_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalShapeDef {
    pub shape_file: String,
    pub description: String,
    pub sub_objs: Vec<SignalShapeSubObjDef>,
}

/// Visual subset of a route `sigcfg.dat`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SigCfgFile {
    pub light_textures: HashMap<String, LightTextureDef>,
    pub lights_tab: HashMap<String, LightColour>,
    pub signal_types: HashMap<String, SignalTypeDef>,
    /// Keyed by uppercase shape filename (`BRCL-H2A.S`).
    pub signal_shapes: HashMap<String, SignalShapeDef>,
}

impl SigCfgFile {
    pub fn load_for_route(route_dir: &Path) -> Result<Self, FormatError> {
        for rel in [
            "sigcfg.dat",
            "SIGCFG.DAT",
            "OpenRails/sigcfg.dat",
            "openrails/sigcfg.dat",
        ] {
            let path = route_dir.join(rel);
            if path.is_file() {
                return Self::from_path(&path);
            }
        }
        Ok(Self::default())
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, FormatError> {
        let text = read_msts_file_decoded(path.as_ref())?;
        Ok(Self::from_text(&text))
    }

    pub fn from_text(text: &str) -> Self {
        let mut out = Self::default();
        scan_light_textures(text, &mut out);
        scan_lights_tab(text, &mut out);
        scan_signal_types(text, &mut out);
        scan_signal_shapes(text, &mut out);
        out
    }

    pub fn signal_shape(&self, shape_file: &str) -> Option<&SignalShapeDef> {
        let key = PathBuf::from(shape_file)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(shape_file)
            .to_ascii_uppercase();
        self.signal_shapes.get(&key)
    }

    pub fn signal_type(&self, name: &str) -> Option<&SignalTypeDef> {
        self.signal_types.get(name).or_else(|| {
            self.signal_types
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        })
    }

    pub fn light_colour(&self, name: &str) -> Option<LightColour> {
        self.lights_tab.get(name).copied().or_else(|| {
            self.lights_tab
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| *v)
        })
    }
}

fn matching_paren_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(|b| *b == b'(') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_string = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_keyword_blocks<'a>(text: &'a str, keyword: &str) -> Vec<&'a str> {
    let lower = text.to_ascii_lowercase();
    let key = keyword.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(&key) {
        let idx = search_from + rel;
        // Require keyword boundary (start or non-alnum before).
        if idx > 0 {
            let prev = text.as_bytes()[idx - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search_from = idx + key.len();
                continue;
            }
        }
        let after = &text[idx + key.len()..];
        let Some(open_rel) = after.find('(') else {
            search_from = idx + key.len();
            continue;
        };
        // Only whitespace between keyword and '('.
        if !after[..open_rel].chars().all(|c| c.is_whitespace()) {
            search_from = idx + key.len();
            continue;
        }
        let block_start = idx + key.len() + open_rel;
        let Some(end_rel) = matching_paren_end(&text[block_start..]) else {
            search_from = idx + key.len();
            continue;
        };
        out.push(&text[block_start..block_start + end_rel + 1]);
        search_from = block_start + end_rel + 1;
    }
    out
}

fn quoted_strings(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = block.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            if i <= bytes.len() {
                out.push(String::from_utf8_lossy(&bytes[start..i]).into_owned());
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    out
}

fn first_numbers(block: &str, max: usize) -> Vec<f32> {
    let mut out = Vec::new();
    for tok in block.split(|c: char| c == '(' || c == ')' || c.is_whitespace()) {
        let t = tok.trim();
        if t.is_empty() || t.starts_with('"') {
            continue;
        }
        if let Ok(n) = t.parse::<f32>() {
            out.push(n);
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

fn scan_light_textures(text: &str, out: &mut SigCfgFile) {
    for block in find_keyword_blocks(text, "LightTex") {
        let strings = quoted_strings(block);
        if strings.len() < 2 {
            continue;
        }
        let nums = first_numbers(block, 8);
        // LightTex ( "name" "file" u0 v0 u1 v1 ) — numbers after strings.
        let floats: Vec<f32> = nums;
        // Prefer trailing 4 floats after the two strings; scan all numbers and take last 4 if >4.
        let (u0, v0, u1, v1) = if floats.len() >= 4 {
            let n = floats.len();
            (floats[n - 4], floats[n - 3], floats[n - 2], floats[n - 1])
        } else {
            (0.0, 0.0, 1.0, 1.0)
        };
        let name = strings[0].clone();
        out.light_textures.insert(
            name.clone(),
            LightTextureDef {
                name,
                texture_file: strings[1].clone(),
                u0,
                v0,
                u1,
                v1,
            },
        );
    }
}

fn scan_lights_tab(text: &str, out: &mut SigCfgFile) {
    for block in find_keyword_blocks(text, "LightsTabEntry") {
        let strings = quoted_strings(block);
        let Some(name) = strings.first().cloned() else {
            continue;
        };
        // colour ( a r g b )
        if let Some(rel) = block.to_ascii_lowercase().find("colour") {
            let after = &block[rel..];
            if let Some(open) = after.find('(') {
                let nums = first_numbers(&after[open..], 4);
                if nums.len() >= 4 {
                    out.lights_tab.insert(
                        name,
                        LightColour {
                            a: nums[0].clamp(0.0, 255.0) as u8,
                            r: nums[1].clamp(0.0, 255.0) as u8,
                            g: nums[2].clamp(0.0, 255.0) as u8,
                            b: nums[3].clamp(0.0, 255.0) as u8,
                        },
                    );
                }
            }
        }
    }
}

fn scan_signal_types(text: &str, out: &mut SigCfgFile) {
    for block in find_keyword_blocks(text, "SignalType") {
        let strings = quoted_strings(block);
        let Some(name) = strings.first().cloned() else {
            continue;
        };
        // Skip nested tokens that aren't the type name (rare).
        if name.eq_ignore_ascii_case("NORMAL")
            || name.eq_ignore_ascii_case("DISTANCE")
            || name.eq_ignore_ascii_case("INFO")
        {
            continue;
        }
        let light_texture = find_keyword_blocks(block, "SignalLightTex")
            .first()
            .and_then(|b| quoted_strings(b).into_iter().next());
        let lights = scan_signal_lights(block);
        let draw_states = scan_draw_states(block);
        let aspects = scan_aspects(block);
        out.signal_types.insert(
            name.clone(),
            SignalTypeDef {
                name,
                light_texture,
                lights,
                draw_states,
                aspects,
            },
        );
    }
}

fn scan_signal_lights(block: &str) -> Vec<SignalLightDef> {
    let mut out = Vec::new();
    for light in find_keyword_blocks(block, "SignalLight") {
        let nums = first_numbers(light, 1);
        let strings = quoted_strings(light);
        let index = nums.first().copied().unwrap_or(0.0) as u32;
        let colour_name = strings
            .first()
            .cloned()
            .unwrap_or_else(|| "White Light".into());
        let mut position = [0.0, 0.0, 0.0];
        let mut radius = 0.15;
        if let Some(pos_block) = find_keyword_blocks(light, "Position").first() {
            let p = first_numbers(pos_block, 3);
            if p.len() >= 3 {
                position = [p[0], p[1], p[2]];
            }
        }
        if let Some(r_block) = find_keyword_blocks(light, "Radius").first() {
            if let Some(r) = first_numbers(r_block, 1).first() {
                radius = *r;
            }
        }
        out.push(SignalLightDef {
            index,
            colour_name,
            position,
            radius,
        });
    }
    out
}

fn scan_draw_states(block: &str) -> Vec<SignalDrawStateDef> {
    let mut out = Vec::new();
    for ds in find_keyword_blocks(block, "SignalDrawState") {
        let strings = quoted_strings(ds);
        let Some(name) = strings.first().cloned() else {
            continue;
        };
        let mut draw_lights = Vec::new();
        for dl in find_keyword_blocks(ds, "DrawLight") {
            if let Some(n) = first_numbers(dl, 1).first() {
                draw_lights.push(*n as u32);
            }
        }
        out.push(SignalDrawStateDef { name, draw_lights });
    }
    out
}

fn scan_aspects(block: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for asp in find_keyword_blocks(block, "SignalAspect") {
        // SignalAspect ( STOP "Red" ) — first token may be unquoted symbol.
        let strings = quoted_strings(asp);
        let body = asp.trim().trim_start_matches('(').trim();
        let mut tokens = Vec::new();
        for tok in body.split_whitespace() {
            let t = tok.trim_matches(|c| c == '(' || c == ')');
            if t.is_empty() || t.eq_ignore_ascii_case("SignalAspect") {
                continue;
            }
            if t.starts_with('"') {
                continue; // captured via quoted_strings
            }
            if t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                tokens.push(t.to_string());
            }
        }
        let aspect = tokens.first().cloned();
        let draw = strings.first().cloned().or_else(|| tokens.get(1).cloned());
        if let (Some(a), Some(d)) = (aspect, draw) {
            out.push((a, d));
        }
    }
    out
}

fn scan_signal_shapes(text: &str, out: &mut SigCfgFile) {
    for block in find_keyword_blocks(text, "SignalShape") {
        let strings = quoted_strings(block);
        // Often unquoted: SignalShape ( "file.s" "desc" ) OR SignalShape (\n "file.s"
        let mut shape_file = strings.first().cloned();
        if shape_file.is_none() {
            // Unquoted filename token ending in .s
            for tok in block.split_whitespace() {
                let t = tok.trim_matches(|c| c == '(' || c == ')' || c == '"');
                if t.to_ascii_lowercase().ends_with(".s") {
                    shape_file = Some(t.to_string());
                    break;
                }
            }
        }
        let Some(shape_file) = shape_file else {
            continue;
        };
        let shape_file = PathBuf::from(&shape_file)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&shape_file)
            .to_string();
        let description = strings.get(1).cloned().unwrap_or_default();
        let sub_objs = scan_shape_sub_objs(block);
        out.signal_shapes.insert(
            shape_file.to_ascii_uppercase(),
            SignalShapeDef {
                shape_file,
                description,
                sub_objs,
            },
        );
    }
}

fn scan_shape_sub_objs(block: &str) -> Vec<SignalShapeSubObjDef> {
    let mut out = Vec::new();
    for sub in find_keyword_blocks(block, "SignalSubObj") {
        let nums = first_numbers(sub, 1);
        let strings = quoted_strings(sub);
        let index = nums.first().copied().unwrap_or(0.0) as u32;
        let matrix_name = strings.first().cloned().unwrap_or_default();
        if matrix_name.is_empty() {
            continue;
        }
        let signal_sub_type = find_keyword_blocks(sub, "SigSubType")
            .first()
            .and_then(|b| {
                b.split_whitespace()
                    .map(|t| t.trim_matches(|c| c == '(' || c == ')'))
                    .find(|t| {
                        !t.is_empty()
                            && !t.eq_ignore_ascii_case("SigSubType")
                            && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    })
                    .map(|s| s.to_string())
            });
        let signal_type_name = find_keyword_blocks(sub, "SigSubSType")
            .first()
            .and_then(|b| quoted_strings(b).into_iter().next());
        out.push(SignalShapeSubObjDef {
            index,
            matrix_name,
            signal_sub_type,
            signal_type_name,
        });
    }
    out
}

/// Map coarse aspect (0=Stop, 1=Caution, 2=Clear) onto lit draw-light indices.
pub fn lit_light_indices_for_aspect(
    signal_type: &SignalTypeDef,
    aspect_stop_caution_clear: u8,
) -> Vec<u32> {
    let aliases: &[&str] = match aspect_stop_caution_clear {
        0 => &["STOP", "STOP_AND_PROCEED", "RESTRICTING"],
        1 => &["APPROACH_1", "APPROACH_2", "APPROACH_3", "APPROACH"],
        _ => &["CLEAR_1", "CLEAR_2", "CLEAR"],
    };
    for (asp, ds_name) in &signal_type.aspects {
        if aliases.iter().any(|a| asp.eq_ignore_ascii_case(a)) {
            if let Some(ds) = signal_type
                .draw_states
                .iter()
                .find(|d| d.name.eq_ignore_ascii_case(ds_name))
            {
                return ds.draw_lights.clone();
            }
        }
    }
    let want = match aspect_stop_caution_clear {
        0 => "red",
        1 => "amber",
        _ => "green",
    };
    signal_type
        .lights
        .iter()
        .filter(|l| l.colour_name.to_ascii_lowercase().contains(want))
        .map(|l| l.index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_sigcfg_visual_blocks() {
        let text = r#"
LightTextures ( 1
	LightTex ( "ltex" "SigLight.ace" 0 0 1 1 )
)
LightsTab ( 1
	LightsTabEntry (
		"Red Light"
		colour ( 255 255 40 40 )
	)
)
SignalTypes ( 1
	SignalType ( "BRCLHome"
		SignalLightTex ( "ltex" )
		SignalLights ( 2
			SignalLight ( 0 "Red Light"
				Position ( 0 4.27 0.2 )
				Radius ( 0.26 )
			)
			SignalLight ( 1 "Green Light"
				Position ( 0 4.65 0.2 )
				Radius ( 0.26 )
			)
		)
		SignalDrawStates ( 2
			SignalDrawState ( 0
				"Red"
				DrawLights ( 1
					DrawLight ( 0 )
				)
			)
			SignalDrawState ( 1
				"Green"
				DrawLights ( 1
					DrawLight ( 1 )
				)
			)
		)
		SignalAspects ( 2
			SignalAspect ( STOP "Red" )
			SignalAspect ( CLEAR_2 "Green" )
		)
	)
)
SignalShapes ( 1
	SignalShape (
		"BRCL-H2A.s"
		"BR Colour Light 2 Aspect Home"
		SignalSubObjs ( 1
			SignalSubObj ( 0
				"HEAD1"
				"Signal Head 1"
				SigSubType ( SIGNAL_HEAD )
				SigSubSType ( "BRCLHome" )
			)
		)
	)
)
"#;
        let cfg = SigCfgFile::from_text(text);
        assert!(cfg.light_textures.contains_key("ltex"));
        assert_eq!(cfg.lights_tab["Red Light"].r, 255);
        let st = cfg.signal_type("BRCLHome").expect("type");
        assert_eq!(st.lights.len(), 2);
        assert!((st.lights[0].position[1] - 4.27).abs() < 1e-3);
        assert_eq!(lit_light_indices_for_aspect(st, 0), vec![0]);
        assert_eq!(lit_light_indices_for_aspect(st, 2), vec![1]);
        let shape = cfg.signal_shape("BRCL-H2A.s").expect("shape");
        assert_eq!(
            shape.sub_objs[0].signal_type_name.as_deref(),
            Some("BRCLHome")
        );
    }

    #[test]
    fn chiltern_sigcfg_loads_shapes_and_types() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Some(home) = home else { return };
        let path = home.join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/sigcfg.dat");
        if !path.is_file() {
            return;
        }
        let cfg = SigCfgFile::from_path(&path).expect("sigcfg");
        assert!(
            cfg.signal_shapes.len() > 100,
            "expected many SignalShapes, got {}",
            cfg.signal_shapes.len()
        );
        assert!(cfg.signal_type("BRCLHome").is_some());
        assert!(cfg.signal_shape("BRCL-H2A.s").is_some());
    }
}

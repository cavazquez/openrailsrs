//! Opt-in PBR metadata beside an MSTS shape (`*.s.pbr.json`) — issue #44.
//!
//! Classic MSTS content has no sidecar and pays zero cost (no tangents, no normal maps).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Sidecar describing optional normal maps for a shape's albedo textures.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ShapePbrSidecar {
    /// Map albedo texture filename → normal-map ACE/DDS filename (same search dirs).
    #[serde(default)]
    pub normal_maps: HashMap<String, String>,
    /// DirectX-style Y flip for the normal map (`StandardMaterial::flip_normal_map_y`).
    /// Default `false` = OpenGL convention (Bevy default).
    #[serde(default)]
    pub flip_normal_map_y: bool,
}

impl ShapePbrSidecar {
    /// Resolve the normal-map filename for an albedo texture (case-insensitive).
    pub fn normal_map_for_albedo(&self, albedo: &str) -> Option<&str> {
        let key = albedo.trim();
        if key.is_empty() {
            return None;
        }
        if let Some(v) = self.normal_maps.get(key) {
            return Some(v.as_str());
        }
        let upper = key.to_ascii_uppercase();
        self.normal_maps
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&upper) || k.to_ascii_uppercase() == upper)
            .map(|(_, v)| v.as_str())
    }

    /// All normal-map filenames referenced by this sidecar (for ACE prefetch).
    pub fn normal_map_filenames(&self) -> impl Iterator<Item = &str> {
        self.normal_maps.values().map(|s| s.as_str())
    }

    pub fn has_any_normal_map(&self) -> bool {
        !self.normal_maps.is_empty()
    }
}

/// Path of the PBR sidecar for a shape file (`foo.s` → `foo.s.pbr.json`).
pub fn shape_pbr_sidecar_path(shape_path: &Path) -> PathBuf {
    let mut p = shape_path.as_os_str().to_os_string();
    p.push(".pbr.json");
    PathBuf::from(p)
}

/// Load [`ShapePbrSidecar`] if the sibling JSON exists and parses.
/// Missing file or invalid JSON → `None` (debug log only; never hard-fail).
pub fn load_shape_pbr_sidecar(shape_path: &Path) -> Option<ShapePbrSidecar> {
    let path = shape_pbr_sidecar_path(shape_path);
    if !path.is_file() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<ShapePbrSidecar>(&text) {
            Ok(sc) => {
                if sc.has_any_normal_map() {
                    bevy::log::debug!(
                        "PBR sidecar {}: {} normal map(s)",
                        path.display(),
                        sc.normal_maps.len()
                    );
                }
                Some(sc)
            }
            Err(e) => {
                bevy::log::debug!(
                    "PBR sidecar {}: invalid JSON ({e}); ignoring",
                    path.display()
                );
                None
            }
        },
        Err(e) => {
            bevy::log::debug!(
                "PBR sidecar {}: read failed ({e}); ignoring",
                path.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_sidecar_and_case_insensitive_lookup() {
        let json = r#"{
            "normal_maps": { "Body.ACE": "body_n.ace" },
            "flip_normal_map_y": false
        }"#;
        let sc: ShapePbrSidecar = serde_json::from_str(json).expect("parse");
        assert_eq!(sc.normal_map_for_albedo("body.ace"), Some("body_n.ace"));
        assert_eq!(sc.normal_map_for_albedo("BODY.ACE"), Some("body_n.ace"));
        assert!(sc.normal_map_for_albedo("other.ace").is_none());
        assert!(!sc.flip_normal_map_y);
    }

    #[test]
    fn load_from_sibling_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shape = dir.path().join("demo.s");
        std::fs::write(&shape, b"( shape )").expect("shape");
        let sidecar = shape_pbr_sidecar_path(&shape);
        assert_eq!(sidecar.file_name().unwrap(), "demo.s.pbr.json");
        {
            let mut f = std::fs::File::create(&sidecar).expect("create");
            write!(
                f,
                r#"{{"normal_maps":{{"a.ace":"a_n.ace"}},"flip_normal_map_y":true}}"#
            )
            .expect("write");
        }
        let sc = load_shape_pbr_sidecar(&shape).expect("load");
        assert_eq!(sc.normal_map_for_albedo("a.ace"), Some("a_n.ace"));
        assert!(sc.flip_normal_map_y);
    }

    #[test]
    fn missing_sidecar_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shape = dir.path().join("alone.s");
        std::fs::write(&shape, b"( shape )").expect("shape");
        assert!(load_shape_pbr_sidecar(&shape).is_none());
    }
}

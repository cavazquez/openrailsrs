//! MSTS / OpenRails `.haz` hazard definition files (#33).
//!
//! WORLD `Hazard` objects store a `.haz` path in `FileName`. OpenRails loads
//! `Tr_Worldfile` / `Tr_HazardFile` and takes the nested `FileName` (a `.s`
//! under `Global/Shapes`).

use std::path::Path;

use crate::error::FormatError;
use crate::msts_file_text::read_msts_file_decoded;

/// Parsed hazard config (visual fields only for v1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HazardFile {
    /// Shape filename from `Tr_Worldfile` / `FileName` (e.g. `crowhaz1.s`).
    pub file_name: String,
}

impl HazardFile {
    pub fn from_path(path: &Path) -> Result<Self, FormatError> {
        let text = read_msts_file_decoded(path)?;
        Self::from_text(&text).ok_or_else(|| FormatError::MissingField {
            key: "FileName".into(),
            context: format!("hazard {}", path.display()),
        })
    }

    /// Scan decoded text for the shape `FileName` inside `Tr_Worldfile`.
    pub fn from_text(text: &str) -> Option<Self> {
        let lower = text.to_ascii_lowercase();
        // Prefer FileName inside a Tr_Worldfile / Tr_HazardFile block when present.
        let search = if let Some(rel) = lower.find("tr_worldfile") {
            &text[rel..]
        } else if let Some(rel) = lower.find("tr_hazardfile") {
            &text[rel..]
        } else {
            text
        };
        let file_name = scan_filename(search)?;
        Some(Self { file_name })
    }
}

/// Resolve WORLD `crow.haz` → shape basename `crowhaz1.s` under `route_dir`.
pub fn resolve_hazard_shape_name(route_dir: &Path, haz_file: &str) -> Option<String> {
    let trimmed = haz_file.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_lowercase().ends_with(".s") {
        return Some(trimmed.to_string());
    }
    let path = crate::resolve_route_relative_file(route_dir, trimmed).or_else(|| {
        let direct = route_dir.join(trimmed);
        direct.is_file().then_some(direct)
    })?;
    HazardFile::from_path(&path).ok().map(|h| h.file_name)
}

fn scan_filename(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find("filename") {
        let idx = search_from + rel;
        let after = &text[idx + "filename".len()..];
        if let Some(name) = parse_filename_args(after) {
            return Some(name);
        }
        search_from = idx + "filename".len();
    }
    None
}

/// Accepts ` ( crowhaz1.s )` or ` ( "crowhaz1.s" )` after the `FileName` token.
fn parse_filename_args(after: &str) -> Option<String> {
    let open = after.find('(')?;
    let rest = &after[open + 1..];
    let close = rest.find(')')?;
    let inner = rest[..close].trim();
    if inner.is_empty() {
        return None;
    }
    let name = if let Some(stripped) = inner.strip_prefix('"') {
        let end = stripped.find('"')?;
        stripped[..end].to_string()
    } else {
        inner.split_whitespace().next().unwrap_or(inner).to_string()
    };
    if name.is_empty() { None } else { Some(name) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_crow_haz_text() {
        let text = r#"
SIMISA@@@@@@@@@@JINX0h0t______
Tr_Worldfile
(
FileName ( crowhaz1.s )
Distance ( 10 )
)
"#;
        let haz = HazardFile::from_text(text).expect("parse");
        assert_eq!(haz.file_name, "crowhaz1.s");
    }

    fn chiltern_route() -> Option<PathBuf> {
        std::env::var_os("CHILTERN_ROUTE")
            .map(PathBuf::from)
            .or_else(|| {
                let home = std::env::var_os("HOME")?;
                let p = PathBuf::from(home)
                    .join("Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
                p.join("crow.haz").is_file().then_some(p)
            })
    }

    #[test]
    fn chiltern_crow_haz_resolves_shape() {
        let Some(route) = chiltern_route() else {
            return;
        };
        let name = resolve_hazard_shape_name(&route, "crow.haz").expect("resolve");
        assert_eq!(name.to_ascii_lowercase(), "crowhaz1.s");
    }
}

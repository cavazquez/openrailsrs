//! Load and blit OR `Content/ETCS` NA_*/PL_* symbols (#160).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use image::RgbaImage;

use super::colors;

/// Cached RGBA symbols keyed by filename (`NA_13.bmp`, `PL_22.png`, …).
pub struct EtcsSymbols {
    images: HashMap<String, RgbaImage>,
    root: Option<PathBuf>,
}

impl EtcsSymbols {
    pub fn load() -> Self {
        let root = resolve_etcs_content_dir();
        let mut images = HashMap::new();
        if let Some(ref dir) = root {
            for name in REQUIRED_SYMBOLS {
                if let Some(img) = load_symbol(dir, name) {
                    images.insert((*name).to_string(), img);
                }
            }
        }
        Self { images, root }
    }

    pub fn global() -> &'static EtcsSymbols {
        static CACHE: OnceLock<EtcsSymbols> = OnceLock::new();
        CACHE.get_or_init(EtcsSymbols::load)
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn get(&self, name: &str) -> Option<&RgbaImage> {
        self.images.get(name)
    }

    pub fn blit(
        &self,
        rgba: &mut [u8],
        stride_w: u32,
        stride_h: u32,
        dest_x: i32,
        dest_y: i32,
        name: &str,
    ) -> bool {
        let Some(img) = self.get(name) else {
            return false;
        };
        blit_rgba(rgba, stride_w, stride_h, dest_x, dest_y, img);
        true
    }

    /// Centre `name` inside a button rect.
    pub fn blit_centered(
        &self,
        rgba: &mut [u8],
        stride_w: u32,
        stride_h: u32,
        bx: i32,
        by: i32,
        bw: i32,
        bh: i32,
        name: &str,
    ) -> bool {
        let Some(img) = self.get(name) else {
            return false;
        };
        let dx = bx + (bw - img.width() as i32) / 2;
        let dy = by + (bh - img.height() as i32) / 2;
        blit_rgba(rgba, stride_w, stride_h, dx, dy, img);
        true
    }
}

const REQUIRED_SYMBOLS: &[&str] = &[
    "NA_03.bmp",
    "NA_04.bmp",
    "NA_05.bmp",
    "NA_06.bmp",
    "NA_13.bmp",
    "NA_14.bmp",
    "NA_15.bmp",
    "NA_16.bmp",
    "PL_21.png",
    "PL_22.png",
    "PL_23.png",
];

/// Resolve `Content/ETCS` (OR RunActivity content, not MSTS route pack).
pub fn resolve_etcs_content_dir() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("OPENRAILSRS_ETCS_CONTENT") {
        let p = PathBuf::from(env);
        if p.is_dir() {
            return Some(p);
        }
    }
    // Fixtures shipped with the repo (tests / fallback).
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/fixtures/etcs");
    if fixture.is_dir() && fixture.join("NA_13.bmp").is_file() {
        // Prefer a full OR install when available; keep fixture as last resort below.
    }

    let mut candidates = Vec::new();
    if let Ok(env) = std::env::var("OPENRAILSRS_MSTS_CONTENT") {
        candidates.push(PathBuf::from(env).join("ETCS"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        candidates.push(home.join("Documentos/Open Rails/Content/ETCS"));
        candidates.push(home.join("Documents/Open Rails/Content/ETCS"));
        candidates.push(
            home.join("wine64-OpenRails/drive_c/Program Files/Open Rails/Content/ETCS"),
        );
    }
    // Sibling Open Rails source tree (dev checkout).
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../openrails/Source/RunActivity/Content/ETCS"),
    );
    candidates.push(fixture);

    for c in candidates {
        if c.is_dir() && (c.join("NA_13.bmp").is_file() || c.join("PL_22.png").is_file()) {
            return Some(c);
        }
    }
    None
}

fn load_symbol(dir: &Path, name: &str) -> Option<RgbaImage> {
    let path = dir.join(name);
    let img = image::open(&path).ok()?.to_rgba8();
    Some(key_bg_to_alpha(img))
}

/// BMP symbols use DMI background RGB as transparent; PNGs already have alpha.
fn key_bg_to_alpha(mut img: RgbaImage) -> RgbaImage {
    let key = colors::BG;
    for px in img.pixels_mut() {
        if px.0[0] == key[0] && px.0[1] == key[1] && px.0[2] == key[2] {
            px.0[3] = 0;
        }
    }
    img
}

fn blit_rgba(
    rgba: &mut [u8],
    stride_w: u32,
    stride_h: u32,
    dest_x: i32,
    dest_y: i32,
    img: &RgbaImage,
) {
    for (py, row) in img.rows().enumerate() {
        for (px, p) in row.enumerate() {
            let a = p.0[3];
            if a == 0 {
                continue;
            }
            let x = dest_x + px as i32;
            let y = dest_y + py as i32;
            if x < 0 || y < 0 {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            if x >= stride_w || y >= stride_h {
                continue;
            }
            let i = ((y * stride_w + x) * 4) as usize;
            if i + 3 >= rgba.len() {
                continue;
            }
            if a == 255 {
                rgba[i..i + 4].copy_from_slice(&[p.0[0], p.0[1], p.0[2], 255]);
            } else {
                let inv = 255u32 - u32::from(a);
                for c in 0..3 {
                    let dst = u32::from(rgba[i + c]);
                    let src = u32::from(p.0[c]);
                    rgba[i + c] = ((src * u32::from(a) + dst * inv) / 255) as u8;
                }
                rgba[i + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_or_content_resolve() {
        let dir = resolve_etcs_content_dir();
        assert!(dir.is_some(), "expected fixtures or OR Content/ETCS");
        let sym = EtcsSymbols::load();
        assert!(sym.get("NA_13.bmp").is_some());
        assert!(sym.get("PL_22.png").is_some());
    }
}

//! Planning ranges, not residency measurements. Inventory reads metadata only.
use crate::player_settings::PlayerSettings;
use serde::Serialize;
use std::{collections::HashSet, path::Path};
#[derive(Clone, Debug, Default, Serialize)]
pub struct Inventory {
    pub disk_bytes: u64,
    pub model_bytes: u64,
    pub texture_bytes: u64,
    pub world_bytes: u64,
    pub tiles: u64,
    pub files: u64,
    pub partial: bool,
}
impl Inventory {
    pub fn scan(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let mut result = Self::default();
        let mut stack = vec![root.clone()];
        let mut seen = HashSet::new();
        while let Some(dir) = stack.pop() {
            openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
            for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                let meta = entry.metadata().map_err(|e| e.to_string())?;
                if entry.file_type().map_err(|e| e.to_string())?.is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    if stack.len() < 10_000 {
                        stack.push(path);
                    } else {
                        result.partial = true;
                    }
                    continue;
                }
                if !meta.is_file() || !seen.insert(path.clone()) {
                    continue;
                }
                result.files += 1;
                result.disk_bytes = result.disk_bytes.saturating_add(meta.len());
                match path
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "s" => result.model_bytes = result.model_bytes.saturating_add(meta.len()),
                    "ace" | "dds" | "png" | "bmp" | "jpg" => {
                        result.texture_bytes = result.texture_bytes.saturating_add(meta.len())
                    }
                    "w" => result.world_bytes = result.world_bytes.saturating_add(meta.len()),
                    "t" => result.tiles += 1,
                    _ => (),
                }
                if result.files >= 250_000 {
                    result.partial = true;
                    return Ok(result);
                }
            }
        }
        Ok(result)
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct TrainBytes {
    pub disk: u64,
    pub geometry: u64,
    pub textures: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Estimate {
    pub disk_mib: f64,
    pub ram_mib: [f64; 2],
    pub vram_mib: [f64; 2],
    pub partial: bool,
    pub resident_tiles: u64,
}
impl Estimate {
    pub fn from_inventory(route: &Inventory, train: TrainBytes, s: &PlayerSettings) -> Self {
        let mib = 1024. * 1024.;
        let keep_ring = ((f64::from(s.view_distance_m) + 2048.) / 2048.).ceil() as u64;
        let resident_tiles = (2 * keep_ring + 1).pow(2).min(route.tiles.max(1));
        let fraction = resident_tiles as f64 / route.tiles.max(1) as f64;
        // World placements bound local use. Global assets may be shared or loaded
        // repeatedly; the upper estimate allows four times uniform distribution.
        let low = fraction.min(1.);
        let high = (fraction * 4.).min(1.);
        let models = route.model_bytes as f64 / mib;
        let textures = route.texture_bytes as f64 / mib;
        let quality = match s.scenery_quality {
            crate::enhanced_scenery::SceneryQuality::Low => 0.8,
            crate::enhanced_scenery::SceneryQuality::Medium => 1.,
            crate::enhanced_scenery::SceneryQuality::High => 1.3,
        };
        let train_ram = (train.geometry + train.textures) as f64 / mib;
        let train_gpu = train_ram * 1.34;
        let shadow = if s.shadows { 48. } else { 0. };
        Self {
            disk_mib: (route.disk_bytes + train.disk) as f64 / mib,
            ram_mib: [
                384. + train_ram + (models * 2. + textures) * low * quality,
                1536. + train_ram * 2. + (models * 16. + textures * 24.) * high * quality,
            ],
            vram_mib: [
                96. + shadow + train_gpu + (models + textures * 0.5) * low * quality,
                384. + shadow + train_gpu + (models * 8. + textures * 16.) * high * quality,
            ],
            partial: route.partial,
            resident_tiles,
        }
    }
    pub fn text(&self, s: &PlayerSettings) -> String {
        format!(
            "Estimación previa · inventario de archivos{}\nDisco instalado: {:.0} MiB · RAM: {:.0}–{:.0} MiB\nVRAM de recursos: {:.0}–{:.0} MiB · sectores retenidos: hasta {}\nDistancia {:.0} m · detalle {} · render {}\nRangos orientativos: compresión, reutilización y cachés varían. En CPU por software, los recursos gráficos usan memoria del sistema; no sumes RAM y VRAM. F8 → Diagnóstico muestra mediciones de la partida.",
            if self.partial { " parcial" } else { "" },
            self.disk_mib,
            self.ram_mib[0],
            self.ram_mib[1],
            self.vram_mib[0],
            self.vram_mib[1],
            self.resident_tiles,
            s.view_distance_m,
            s.scenery_quality.label(),
            s.renderer.label()
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distance_quality_and_separate_ranges_follow_inventory() {
        let route = Inventory {
            disk_bytes: 1024 * 1024 * 1024,
            texture_bytes: 512 * 1024 * 1024,
            model_bytes: 64 * 1024 * 1024,
            tiles: 100,
            ..Default::default()
        };
        let mut s = PlayerSettings {
            view_distance_m: 1000.,
            ..Default::default()
        };
        let a = Estimate::from_inventory(&route, TrainBytes::default(), &s);
        s.view_distance_m = 4000.;
        s.scenery_quality = crate::enhanced_scenery::SceneryQuality::High;
        let b = Estimate::from_inventory(&route, TrainBytes::default(), &s);
        assert!(b.ram_mib[0] > a.ram_mib[0]);
        assert!(b.vram_mib[1] > a.vram_mib[1]);
        assert_eq!(a.disk_mib, b.disk_mib);
        assert!(a.ram_mib[1] > a.ram_mib[0]);
        assert!(a.text(&s).contains("no sumes"));
    }
}

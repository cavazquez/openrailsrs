//! Resolución de texturas MSTS/Open Rails (paridad con `openrailsrs-viewer3d`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor,
    ImageType,
};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_ace::{AceFile, read_ace};
use openrailsrs_formats::{MstsTexAddrMode, msts_tex_addr_mode, resolve_path_case_insensitive};

/// Estación activa (paridad Open Rails `SeasonType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Season {
    Spring,
    #[default]
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spring => "spring",
            Self::Summer => "summer",
            Self::Autumn => "autumn",
            Self::Winter => "winter",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "spring" => Self::Spring,
            "autumn" | "fall" => Self::Autumn,
            "winter" => Self::Winter,
            _ => Self::Summer,
        }
    }
}

/// Bitfield `ESD_Alternative_Texture` / `Helpers.TextureFlags` (Open Rails).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureFlags(u32);

impl TextureFlags {
    pub const NONE: u32 = 0;
    pub const SNOW: u32 = 0x1;
    pub const SNOW_TRACK: u32 = 0x2;
    pub const SPRING: u32 = 0x4;
    pub const AUTUMN: u32 = 0x8;
    pub const WINTER: u32 = 0x10;
    pub const SPRING_SNOW: u32 = 0x20;
    pub const AUTUMN_SNOW: u32 = 0x40;
    pub const WINTER_SNOW: u32 = 0x80;
    pub const NIGHT: u32 = 0x100;

    /// Flags que Open Rails usa para bosques (`GetForestTextureFile`).
    pub const FOREST: u32 = Self::SPRING
        | Self::AUTUMN
        | Self::WINTER
        | Self::SPRING_SNOW
        | Self::AUTUMN_SNOW
        | Self::WINTER_SNOW;

    pub const fn from_raw(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, flag: u32) -> bool {
        (self.0 & flag) != 0
    }

    pub const fn intersects(self, mask: u32) -> bool {
        (self.0 & mask) != 0
    }
}

/// Entorno de resolución de texturas (estación, clima, día/noche).
#[derive(Clone, Copy, Debug, Resource, PartialEq, Eq)]
pub struct TextureEnvironment {
    pub season: Season,
    pub snow_weather: bool,
    pub night: bool,
}

impl TextureEnvironment {
    /// Verano diurno sin nieve — resolución “legacy” (todas las subcarpetas `TEXTURES/`).
    pub fn summer_day() -> Self {
        Self {
            season: Season::Summer,
            snow_weather: false,
            night: false,
        }
    }

    pub fn from_cli(season: &str, weather: &str, night: bool) -> Self {
        Self {
            season: Season::parse(season),
            snow_weather: weather.eq_ignore_ascii_case("snow"),
            night,
        }
    }

    /// Paridad OR `Helpers.IsSnow`.
    pub fn is_snow(self) -> bool {
        matches!(self.season, Season::Winter)
            || (!matches!(self.season, Season::Summer) && self.snow_weather)
    }

    pub fn is_day(self) -> bool {
        !self.night
    }

    pub fn cache_key(self) -> u32 {
        (self.season as u32) | (u32::from(self.snow_weather) << 4) | (u32::from(self.night) << 5)
    }
}

/// Flags de textura para un shape concreto (`.sd` + heurísticas de ruta).
pub fn shape_texture_flags(shape_path: &Path, alternative_texture: u32) -> TextureFlags {
    let mut flags = TextureFlags::from_raw(alternative_texture);
    let path = shape_path.to_string_lossy().to_ascii_lowercase();
    if path.contains("/global/") || path.contains("\\global\\") {
        flags = TextureFlags::from_raw(flags.bits() | TextureFlags::SNOW_TRACK);
    }
    flags
}

/// Basename de un path MSTS (`TEXTURES\foo.ace` → `foo.ace`).
pub fn texture_file_basename(file_name: &str) -> &str {
    file_name
        .rsplit(['\\', '/'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(file_name)
}

/// Árboles `GLOBAL/` bajo el content pack de la ruta.
pub fn global_assets_dirs(route_dir: &Path, msts_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        let has_shapes = p.join("SHAPES").is_dir() || p.join("shapes").is_dir();
        if has_shapes && !out.iter().any(|existing| existing == &p) {
            out.push(p);
        }
    };
    push(msts_root.join("GLOBAL"));
    let Some(stem) = route_dir.file_name().and_then(|s| s.to_str()) else {
        return out;
    };
    push(msts_root.join(stem).join("GLOBAL"));
    if let Ok(entries) = std::fs::read_dir(msts_root) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(stem)
            {
                push(entry.path().join("GLOBAL"));
            }
        }
    }
    out
}

/// Directorios donde buscar texturas dado el path del `.s` resuelto.
///
/// Conserva el orden de inserción (ruta / shape root antes que GLOBAL).
pub fn texture_search_dirs_for_shape(
    shape_path: &Path,
    route_dir: &Path,
    msts_root: &Path,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |p: PathBuf| {
        if !dirs.iter().any(|existing| existing == &p) {
            dirs.push(p);
        }
    };
    push(route_dir.to_path_buf());
    if let Some(parent) = shape_path.parent() {
        let in_asset_subdir = parent.file_name().is_some_and(|n| {
            n.eq_ignore_ascii_case("shapes")
                || n.eq_ignore_ascii_case("cabview3d")
                || n.eq_ignore_ascii_case("cabview")
        });
        if in_asset_subdir {
            push(parent.to_path_buf());
            if let Some(asset_root) = parent.parent() {
                if asset_root != route_dir {
                    push(asset_root.to_path_buf());
                }
            }
        }
    }
    for global in global_assets_dirs(route_dir, msts_root) {
        push(global);
    }
    if let Some(trainset_root) = vehicle_texture_root_for_shape_path(shape_path) {
        push(trainset_root.to_path_buf());
    }
    dirs
}

/// Raíz del vehículo/trainset dado el path del `.s` resuelto.
pub fn vehicle_texture_root_for_shape_path(shape_path: &Path) -> Option<&Path> {
    let parent = shape_path.parent()?;
    if parent
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("SHAPES") || n.eq_ignore_ascii_case("shapes"))
    {
        parent.parent()
    } else {
        Some(parent)
    }
}

/// Indexa recursivamente `TEXTURES/` (estaciones, subcarpetas).
///
/// Usa `insert` (última escritura gana). El catálogo de ruta indexa de baja→alta
/// prioridad (trainsets → GLOBAL → pack → ruta).
pub fn index_textures_tree(map: &mut HashMap<String, PathBuf>, root: &Path) {
    for sub in ["TEXTURES", "textures"] {
        index_textures_dir(map, &root.join(sub));
    }
    index_textures_dir(map, root);
}

fn index_textures_dir(map: &mut HashMap<String, PathBuf>, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext.eq_ignore_ascii_case("ace") || ext.eq_ignore_ascii_case("dds") {
                    map.insert(name.to_ascii_lowercase(), path);
                }
            }
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            index_textures_dir(map, &path);
        }
    }
}

pub fn index_trainset_textures(map: &mut HashMap<String, PathBuf>, msts_root: &Path) {
    for trains_sub in [
        "TRAINS/TRAINSET",
        "trains/trainset",
        "trains/TRAINSET",
        "TRAINSET",
        "trainset",
    ] {
        let trainsets = msts_root.join(trains_sub);
        let Ok(rd) = std::fs::read_dir(&trainsets) else {
            continue;
        };
        for entry in rd.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                index_textures_tree(map, &entry.path());
            }
        }
    }
}

pub fn resolve_texture_with_index(
    index: &HashMap<String, PathBuf>,
    dirs: &[&Path],
    file_name: &str,
    env: &TextureEnvironment,
    flags: TextureFlags,
) -> Option<PathBuf> {
    for candidate in texture_name_candidates(file_name) {
        for dir in dirs {
            for path in texture_path_candidates(dir, &candidate, env, flags) {
                if path.is_file() {
                    return Some(path);
                }
                if let Some(resolved) = resolve_path_case_insensitive(&path) {
                    return Some(resolved);
                }
            }
        }
        let base = texture_file_basename(&candidate);
        let key = Path::new(base)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())?;
        if let Some(path) = index.get(&key) {
            // El índice es plano; preferir candidatos estacionales explícitos arriba.
            if seasonal_subdir(flags, env).is_none() && !env.night {
                return Some(path.clone());
            }
        }
    }
    None
}

/// Subcarpeta estacional bajo `TEXTURES/` según flags y entorno (OR `GetTextureFile`).
pub fn seasonal_subdir(flags: TextureFlags, env: &TextureEnvironment) -> Option<&'static str> {
    if flags.intersects(TextureFlags::SNOW | TextureFlags::SNOW_TRACK) && env.is_snow() {
        return Some("Snow");
    }
    if env.snow_weather {
        match env.season {
            Season::Spring if flags.contains(TextureFlags::SPRING_SNOW) => {
                return Some("SpringSnow");
            }
            Season::Autumn if flags.contains(TextureFlags::AUTUMN_SNOW) => {
                return Some("AutumnSnow");
            }
            Season::Winter if flags.contains(TextureFlags::WINTER_SNOW) => {
                return Some("WinterSnow");
            }
            _ => {}
        }
    } else {
        match env.season {
            Season::Spring if flags.contains(TextureFlags::SPRING) => return Some("Spring"),
            Season::Autumn if flags.contains(TextureFlags::AUTUMN) => return Some("Autumn"),
            Season::Winter if flags.contains(TextureFlags::WINTER) => return Some("Winter"),
            _ => {}
        }
    }
    None
}

fn texture_roots(asset_root: &Path) -> [PathBuf; 2] {
    [asset_root.join("TEXTURES"), asset_root.join("textures")]
}

fn push_file_variant(out: &mut Vec<PathBuf>, dir: &Path, file_name: &str) {
    out.push(dir.join(file_name));
    let path_obj = Path::new(file_name);
    if path_obj
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ace"))
    {
        let dds = path_obj.with_extension("dds");
        out.push(dir.join(dds));
    }
}

/// Candidatos ordenados para una textura bajo una raíz de assets.
pub fn texture_path_candidates(
    asset_root: &Path,
    file_name: &str,
    env: &TextureEnvironment,
    flags: TextureFlags,
) -> Vec<PathBuf> {
    let base = texture_file_basename(file_name);
    let mut out = Vec::new();

    if env.night && flags.contains(TextureFlags::NIGHT) {
        for root in texture_roots(asset_root) {
            out.extend(night_texture_candidates(&root, base));
        }
    }

    if let Some(season) = seasonal_subdir(flags, env) {
        for root in texture_roots(asset_root) {
            push_file_variant(&mut out, &root.join(season), base);
        }
    }

    for root in texture_roots(asset_root) {
        push_file_variant(&mut out, &root, base);
    }
    push_file_variant(&mut out, asset_root, base);

    // Sin flags estacionales: aceptar cualquier subcarpeta (rutas legacy).
    if flags.bits() == TextureFlags::NONE {
        for root in texture_roots(asset_root) {
            if let Ok(rd) = std::fs::read_dir(&root) {
                for entry in rd.flatten() {
                    if entry.file_type().is_ok_and(|t| t.is_dir()) {
                        push_file_variant(&mut out, &entry.path(), base);
                    }
                }
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Paridad OR `GetNightTextureFile`.
fn night_texture_candidates(textures_root: &Path, file_name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let local_night = textures_root.join("Night");
    let parent_night = textures_root
        .parent()
        .map(|p| p.join("Night"))
        .unwrap_or_else(|| textures_root.join("Night"));

    for night_dir in [local_night, parent_night] {
        push_file_variant(&mut out, &night_dir, file_name);
    }
    out
}

/// Variantes de nombre para texturas MSTS mal referenciadas o con prefijos de pack distintos.
fn texture_name_candidates(file_name: &str) -> Vec<String> {
    let base = texture_file_basename(file_name);
    let mut out = vec![base.to_string(), file_name.to_string()];
    let stem = Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(base);
    let ext = Path::new(base)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("ace");
    let push = |out: &mut Vec<String>, name: String| {
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&name))
        {
            out.push(name);
        }
    };
    for prefix in ["MAS_", "SR_", "BR_", "UK_", "US_"] {
        if let Some(rest) = stem.strip_prefix(prefix) {
            push(&mut out, format!("{rest}.{ext}"));
            for alt in ["MAS", "SR", "BR", "UK", "US"] {
                push(&mut out, format!("{alt}_{rest}.{ext}"));
            }
        }
    }
    if stem.contains('_') {
        if let Some((_p, rest)) = stem.split_once('_') {
            push(&mut out, format!("{rest}.{ext}"));
        }
    }
    out
}

/// Directorios para resolver shapes (ruta + pack MSTS + GLOBAL).
///
/// Orden de búsqueda: **ruta → pack → GLOBAL** (sin `sort`, para no romper precedencia).
pub fn shape_search_dirs(route_dir: &Path, msts_root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push = |p: PathBuf| {
        if !dirs.iter().any(|existing| existing == &p) {
            dirs.push(p);
        }
    };
    push(route_dir.to_path_buf());
    if let Some(stem) = route_dir.file_name() {
        let pack = msts_root.join(stem);
        if pack.is_dir() {
            push(pack);
        } else if let Ok(rd) = std::fs::read_dir(msts_root) {
            for entry in rd.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && path
                        .file_name()
                        .is_some_and(|n| n.eq_ignore_ascii_case(stem))
                {
                    push(path);
                    break;
                }
            }
        }
    }
    for global in global_assets_dirs(route_dir, msts_root) {
        push(global);
    }
    dirs
}

pub fn shape_file_basename(file_name: &str) -> &str {
    texture_file_basename(file_name)
}

/// Resuelve `SHAPES/foo.s` bajo una raíz de assets.
pub fn resolve_shape_path(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    let base = shape_file_basename(file_name);
    for subdir in ["SHAPES", "shapes"] {
        let shapes_root = route_dir.join(subdir);
        let path = shapes_root.join(base);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
        if let Ok(entries) = std::fs::read_dir(&shapes_root) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let nested = entry.path().join(base);
                if nested.is_file() {
                    return Some(nested);
                }
                if let Some(resolved) = resolve_path_case_insensitive(&nested) {
                    return Some(resolved);
                }
            }
        }
    }
    let direct = route_dir.join(base);
    if direct.is_file() {
        return Some(direct);
    }
    resolve_path_case_insensitive(&direct)
}

pub fn resolve_shape_path_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(path) = resolve_shape_path(dir, file_name) {
            return Some(path);
        }
    }
    None
}

/// Lookup por índice (basename lowercase) y fallback a [`resolve_shape_path_in_dirs`].
pub fn resolve_shape_path_with_index(
    index: &HashMap<String, PathBuf>,
    dirs: &[&Path],
    file_name: &str,
) -> Option<PathBuf> {
    let base = shape_file_basename(file_name);
    if let Some(path) = index.get(&base.to_ascii_lowercase()) {
        if path.is_file() {
            return Some(path.clone());
        }
    }
    resolve_shape_path_in_dirs(dirs, file_name)
}

/// Resuelve `TEXTURES/foo.ace` bajo una raíz (variantes estacionales / nocturnas).
pub fn resolve_texture_path(
    route_dir: &Path,
    file_name: &str,
    env: &TextureEnvironment,
    flags: TextureFlags,
) -> Option<PathBuf> {
    for path in texture_path_candidates(route_dir, file_name, env, flags) {
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    None
}

pub fn resolve_texture_path_in_dirs(
    dirs: &[&Path],
    file_name: &str,
    env: &TextureEnvironment,
    flags: TextureFlags,
) -> Option<PathBuf> {
    for dir in dirs {
        if let Some(p) = resolve_texture_path(dir, file_name, env, flags) {
            return Some(p);
        }
    }
    None
}

/// Resolución sin estación/noche (`TextureFlags::NONE` → también subcarpetas legacy).
pub fn resolve_texture_path_legacy(route_dir: &Path, file_name: &str) -> Option<PathBuf> {
    resolve_texture_path(
        route_dir,
        file_name,
        &TextureEnvironment::summer_day(),
        TextureFlags::from_raw(TextureFlags::NONE),
    )
}

/// Como [`resolve_texture_path_legacy`] sobre varias raíces de assets.
pub fn resolve_texture_path_legacy_in_dirs(dirs: &[&Path], file_name: &str) -> Option<PathBuf> {
    resolve_texture_path_in_dirs(
        dirs,
        file_name,
        &TextureEnvironment::summer_day(),
        TextureFlags::from_raw(TextureFlags::NONE),
    )
}

/// Bevy address mode for OR `TexAddrMode` (default Wrap → Repeat).
pub fn image_address_mode_from_msts(raw: Option<i32>) -> ImageAddressMode {
    match raw.and_then(msts_tex_addr_mode).unwrap_or(MstsTexAddrMode::Wrap) {
        MstsTexAddrMode::Wrap => ImageAddressMode::Repeat,
        MstsTexAddrMode::Mirror => ImageAddressMode::MirrorRepeat,
        MstsTexAddrMode::Clamp => ImageAddressMode::ClampToEdge,
        MstsTexAddrMode::Border => ImageAddressMode::ClampToBorder,
    }
}

/// Apply OR texture address mode to a Bevy [`Image`] sampler (U+V).
pub fn apply_tex_addr_mode(image: &mut Image, tex_addr_mode: Option<i32>) {
    let addr = image_address_mode_from_msts(tex_addr_mode);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: addr,
        address_mode_v: addr,
        ..Default::default()
    });
}

pub fn decode_dds_to_image(bytes: &[u8]) -> Result<Image, String> {
    decode_dds_to_image_with_addr(bytes, None)
}

pub fn decode_dds_to_image_with_addr(
    bytes: &[u8],
    tex_addr_mode: Option<i32>,
) -> Result<Image, String> {
    let mut image = Image::from_buffer(
        bytes,
        ImageType::Extension("dds"),
        CompressedImageFormats::all(),
        false,
        ImageSampler::Default,
        RenderAssetUsages::default(),
    )
    .map_err(|e| e.to_string())?;
    apply_tex_addr_mode(&mut image, tex_addr_mode);
    Ok(image)
}

/// Decodifica `.ace` o `.dds` a `Image` Bevy (descomprimido para poder leer sus píxeles).
/// Decodifica `.ace` o `.dds` a `Image` Bevy.
pub fn load_texture_image(path: &Path) -> Option<Image> {
    load_texture_image_with_addr(path, None)
}

pub fn load_texture_image_with_addr(path: &Path, tex_addr_mode: Option<i32>) -> Option<Image> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "dds" {
        let bytes = std::fs::read(path).ok()?;
        return decode_dds_to_image_with_addr(&bytes, tex_addr_mode).ok();
    }
    let ace = read_ace(path).ok()?;
    Some(ace_to_image_with_addr(&ace, tex_addr_mode))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DdsAlpha {
    NoneOr1Bit,
    Full,
}

pub fn dds_alpha_type(path: &Path) -> Option<DdsAlpha> {
    use std::fs::File;
    use std::io::Read;
    let mut f = File::open(path).ok()?;
    let mut header = [0u8; 128];
    f.read_exact(&mut header).ok()?;

    if &header[0..4] != b"DDS " {
        return None;
    }

    let pf_flags = u32::from_le_bytes(header[80..84].try_into().unwrap());
    if (pf_flags & 0x4) != 0 {
        let fourcc = &header[84..88];
        match fourcc {
            b"DXT1" => Some(DdsAlpha::NoneOr1Bit),
            b"DXT3" | b"DXT5" => Some(DdsAlpha::Full),
            _ => Some(DdsAlpha::Full),
        }
    } else {
        if (pf_flags & 0x1) != 0 {
            Some(DdsAlpha::Full)
        } else {
            Some(DdsAlpha::NoneOr1Bit)
        }
    }
}

pub fn load_ace_file(path: &Path) -> Option<AceFile> {
    read_ace(path).ok()
}

pub fn ace_to_image(ace: &AceFile) -> Image {
    ace_to_image_with_addr(ace, None)
}

pub fn ace_to_image_with_addr(ace: &AceFile, tex_addr_mode: Option<i32>) -> Image {
    let mips = if ace.mips.is_empty() {
        vec![openrailsrs_ace::AceMipLevel {
            width: ace.width,
            height: ace.height,
            rgba: ace.mip0.clone(),
        }]
    } else {
        ace.mips.clone()
    };

    let mut data = Vec::new();
    for mip in &mips {
        data.extend_from_slice(&mip.rgba);
    }

    let mut image = Image::new(
        Extent3d {
            width: ace.width,
            height: ace.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    if mips.len() > 1 {
        image.texture_descriptor.mip_level_count = mips.len() as u32;
    }
    apply_tex_addr_mode(&mut image, tex_addr_mode);
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tex_addr_modes_map_to_bevy_samplers() {
        assert!(matches!(
            image_address_mode_from_msts(Some(1)),
            ImageAddressMode::Repeat
        ));
        assert!(matches!(
            image_address_mode_from_msts(Some(2)),
            ImageAddressMode::MirrorRepeat
        ));
        assert!(matches!(
            image_address_mode_from_msts(Some(3)),
            ImageAddressMode::ClampToEdge
        ));
        assert!(matches!(
            image_address_mode_from_msts(Some(4)),
            ImageAddressMode::ClampToBorder
        ));
        assert!(matches!(
            image_address_mode_from_msts(None),
            ImageAddressMode::Repeat
        ));
    }

    fn chiltern_route() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
    }

    fn summer_env() -> TextureEnvironment {
        TextureEnvironment {
            season: Season::Summer,
            snow_weather: false,
            night: false,
        }
    }

    fn default_flags() -> TextureFlags {
        TextureFlags::from_raw(TextureFlags::NONE)
    }

    #[test]
    fn resolve_texture_strips_msts_prefix() {
        let route = chiltern_route();
        if !route.is_dir() {
            return;
        }
        let env = summer_env();
        assert!(
            resolve_texture_path(&route, r"TEXTURES\poplar15_1.ace", &env, default_flags())
                .is_some()
        );
    }

    #[test]
    fn resolve_texture_finds_seasonal_subdir() {
        let route = chiltern_route();
        if !route.is_dir() {
            return;
        }
        let env = summer_env();
        assert!(resolve_texture_path(&route, "poplar15_1.ace", &env, default_flags()).is_some());
    }

    #[test]
    fn new_forest_spring_subdir_when_flagged() {
        let route = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/Routes/Watersnake"));
        let Some(route) = route else { return };
        if !route.is_dir() {
            return;
        }
        let spring_env = TextureEnvironment {
            season: Season::Spring,
            snow_weather: false,
            night: false,
        };
        let flags = TextureFlags::from_raw(TextureFlags::SPRING);
        let spring_dir = route.join("TEXTURES/SPRING");
        let textures_root = route.join("TEXTURES");
        if !spring_dir.is_dir() {
            return;
        }
        let sample = std::fs::read_dir(&spring_dir).ok().and_then(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .find(|e| {
                    let name = e.file_name();
                    !textures_root.join(&name).is_file()
                })
                .map(|e| e.file_name().to_string_lossy().into_owned())
        });
        let Some(name) = sample else {
            return;
        };
        let resolved = resolve_texture_path(&route, &name, &spring_env, flags);
        assert!(
            resolved
                .as_ref()
                .is_some_and(|p| { p.to_string_lossy().to_ascii_lowercase().contains("spring") }),
            "expected SPRING variant for {name}, got {resolved:?}"
        );
    }

    #[test]
    fn new_forest_night_subdir_when_flagged() {
        let route = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join("routes/NewForestRouteV3/Routes/Watersnake"));
        let Some(route) = route else { return };
        if !route.is_dir() {
            return;
        }
        let night_dir = route.join("TEXTURES/NIGHT");
        if !night_dir.is_dir() {
            return;
        }
        let sample = std::fs::read_dir(&night_dir)
            .ok()
            .and_then(|mut rd| rd.find_map(|e| e.ok()))
            .map(|e| e.file_name().to_string_lossy().into_owned());
        let Some(name) = sample else { return };
        let night_env = TextureEnvironment {
            season: Season::Summer,
            snow_weather: false,
            night: true,
        };
        let flags = TextureFlags::from_raw(TextureFlags::NIGHT);
        let resolved = resolve_texture_path(&route, &name, &night_env, flags);
        assert!(
            resolved
                .as_ref()
                .is_some_and(|p| { p.to_string_lossy().to_ascii_lowercase().contains("night") }),
            "expected NIGHT variant for {name}, got {resolved:?}"
        );
    }

    #[test]
    fn resolve_shape_in_chiltern_shapes() {
        let route = chiltern_route();
        if !route.is_dir() {
            return;
        }
        assert!(resolve_shape_path(&route, "smoke1.s").is_some());
    }
}

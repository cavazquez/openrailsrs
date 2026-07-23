//! WORLD item classification shared by render3d / viewer adapters (#112).

use std::path::Path;

use bevy::math::{Quat, Vec3};
use openrailsrs_formats::{DyntrackSection, WorldItem};
pub use openrailsrs_or_shader::coordinates::{
    matrix3x3_to_affine, matrix3x3_to_rotation_scale, matrix3x3_to_xna_mat3, qdir_to_quat,
};

/// Classified WORLD item kind (independent of render marker colors).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MstsWorldItemKind {
    Static,
    Track,
    Dyntrack,
    Signal,
    Forest,
    HWater,
    Pickup,
    Transfer,
    Hazard,
    Other,
}

impl MstsWorldItemKind {
    pub fn from_item(item: &WorldItem) -> Self {
        match item.kind() {
            "Static" => Self::Static,
            "TrackObj" => Self::Track,
            "Dyntrack" => Self::Dyntrack,
            "Signal" => Self::Signal,
            "Forest" => Self::Forest,
            "HWater" => Self::HWater,
            "Transfer" => Self::Transfer,
            "Pickup" => Self::Pickup,
            "Hazard" => Self::Hazard,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::Track => "TrackObj",
            Self::Dyntrack => "Dyntrack",
            Self::Signal => "Signal",
            Self::Forest => "Forest",
            Self::HWater => "HWater",
            Self::Pickup => "Pickup",
            Self::Transfer => "Transfer",
            Self::Hazard => "Hazard",
            Self::Other => "Other",
        }
    }

    /// Marker color RGB used by render3d debug pillars.
    pub fn color_rgb(self) -> (f32, f32, f32) {
        match self {
            Self::Static => (0.95, 0.55, 0.15),
            Self::Track => (0.20, 0.80, 0.85),
            Self::Dyntrack => (0.85, 0.25, 0.85),
            Self::Signal => (0.90, 0.15, 0.15),
            Self::Forest => (0.20, 0.70, 0.25),
            Self::HWater => (0.20, 0.40, 0.95),
            Self::Pickup => (0.55, 0.45, 0.35),
            Self::Transfer => (0.45, 0.72, 0.38),
            Self::Hazard => (0.85, 0.35, 0.25),
            Self::Other => (0.65, 0.65, 0.65),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MstsForestPatch {
    pub uid: u32,
    pub population: u32,
    pub patch_half_x: f32,
    pub patch_half_z: f32,
    pub tree_width: f32,
    pub tree_height: f32,
    pub scale_min: f32,
    pub scale_max: f32,
    pub tree_texture: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MstsHWaterPatch {
    pub uid: u32,
    pub half_x: f32,
    pub half_z: f32,
    pub texture: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MstsTransferPatch {
    pub uid: u32,
    pub width: f32,
    pub height: f32,
    pub texture: Option<String>,
}

/// Classified WORLD item in tile-local `.w` coordinates (X east, Y MSL, Z forward).
///
/// Adapters apply App-specific frames (centered + Y-rebase, absolute MSTS origin, …).
#[derive(Clone, Debug)]
pub struct MstsClassifiedWorldItem {
    pub kind: MstsWorldItemKind,
    /// Original `WorldItem::kind()` string (preserves Speedpost / Other labels).
    pub kind_label: &'static str,
    pub uid: Option<u32>,
    pub position: [f64; 3],
    pub rotation: Quat,
    pub scale: Vec3,
    pub file_name: Option<String>,
    pub section_idx: Option<u32>,
    /// Authored Dyntrack subsections (#87); empty for other kinds.
    pub dyntrack_sections: Vec<DyntrackSection>,
    pub forest: Option<MstsForestPatch>,
    pub hwater: Option<MstsHWaterPatch>,
    pub transfer: Option<MstsTransferPatch>,
}

/// Classify all positioned items in a parsed [`openrailsrs_formats::WorldFile`].
pub fn classify_world_file(
    world: &openrailsrs_formats::WorldFile,
    route_dir: Option<&Path>,
) -> Vec<MstsClassifiedWorldItem> {
    world
        .items
        .iter()
        .filter_map(|item| classify_world_item(item, route_dir))
        .collect()
}

/// Classify one WORLD item. Returns `None` when the item has no position.
pub fn classify_world_item(
    item: &WorldItem,
    route_dir: Option<&Path>,
) -> Option<MstsClassifiedWorldItem> {
    let p = item.position()?;
    let (rotation, scale) = item_transform(item);
    let (forest, hwater, transfer) = scenery_from_item(item);
    let file_name = match item {
        WorldItem::Hazard {
            haz_file: Some(haz),
            ..
        } => route_dir
            .and_then(|dir| openrailsrs_formats::resolve_hazard_shape_name(dir, haz))
            .or_else(|| Some(haz.clone())),
        _ => item.file_name().map(str::to_string),
    };
    Some(MstsClassifiedWorldItem {
        kind: MstsWorldItemKind::from_item(item),
        kind_label: item.kind(),
        uid: item.uid(),
        position: [p.x, p.y, p.z],
        rotation,
        scale,
        file_name,
        section_idx: item.section_idx(),
        dyntrack_sections: item.dyntrack_sections().to_vec(),
        forest,
        hwater,
        transfer,
    })
}

fn scenery_from_item(
    item: &WorldItem,
) -> (
    Option<MstsForestPatch>,
    Option<MstsHWaterPatch>,
    Option<MstsTransferPatch>,
) {
    match item {
        WorldItem::Forest {
            uid,
            tree_texture,
            scale_range,
            patch_size,
            tree_size,
            population,
            ..
        } => {
            let (scale_min, scale_max) = scale_range
                .map(|r| (r[0].max(0.1) as f32, r[1].max(r[0] + 0.01) as f32))
                .unwrap_or((0.85, 1.15));
            let (patch_half_x, patch_half_z) = patch_size
                .map(|a| ((a[0] * 0.5) as f32, (a[1] * 0.5) as f32))
                .unwrap_or((128.0, 128.0));
            let (tree_width, tree_height) = tree_size
                .map(|s| (s[0].max(0.5) as f32, s[1].max(1.0) as f32))
                .unwrap_or((5.0, 12.0));
            (
                Some(MstsForestPatch {
                    uid: *uid,
                    population: *population,
                    patch_half_x,
                    patch_half_z,
                    tree_width,
                    tree_height,
                    scale_min,
                    scale_max,
                    tree_texture: tree_texture.clone(),
                }),
                None,
                None,
            )
        }
        WorldItem::HWater {
            uid,
            file_name,
            size,
            ..
        } => (
            None,
            Some(MstsHWaterPatch {
                uid: *uid,
                half_x: (size[0].max(0.5) * 0.5) as f32,
                half_z: (size[1].max(0.5) * 0.5) as f32,
                texture: file_name.clone(),
            }),
            None,
        ),
        WorldItem::Transfer {
            uid,
            file_name,
            width,
            height,
            ..
        } => (
            None,
            None,
            Some(MstsTransferPatch {
                uid: *uid,
                width: (*width).max(0.5) as f32,
                height: (*height).max(0.5) as f32,
                texture: file_name.clone(),
            }),
        ),
        _ => (None, None, None),
    }
}

/// Rotation + scale following Open Rails / XNA conventions.
pub fn item_transform(item: &WorldItem) -> (Quat, Vec3) {
    if let Some(m) = item.matrix3x3() {
        let (rot, scale) = matrix3x3_to_rotation_scale(&m);
        return (sanitize_quat(rot), scale);
    }
    let rot = item
        .qdirection()
        .map(|q| qdir_to_quat(&q))
        .unwrap_or(Quat::IDENTITY);
    (sanitize_quat(rot), Vec3::ONE)
}

fn sanitize_quat(q: Quat) -> Quat {
    if q.is_finite() && q.length_squared() > 1e-6 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

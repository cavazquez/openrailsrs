//! 3D cab interior from MSTS `CABVIEW3D/` (Open Rails `ThreeDimentionCabCamera`).
//!
//! OR attaches the cab shape (`.s` + `.ace` in `CABVIEW3D/`, driven by `.cvf`) to the
//! lead vehicle; the driver camera uses `ORTS3DCabHeadPos` from the `.eng`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;

use crate::cab_cvf::{self, CabCvfPart, static_lever_transform};
use crate::cab_diag;
use crate::camera::CameraFollowMode;
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::{
    RouteAssets, load_cab_interior_render_asset_from_path, msts_shape_to_train_rotation,
    resolve_shape_path_in_dirs, texture_search_dirs_for_shape,
};
use crate::viewer_log;
use openrailsrs_formats::ShapeFile;

/// Marker on cab-interior entities parented to the lead vehicle in driver view.
#[derive(Component)]
pub struct CabInteriorMarker;

/// Per-part metadata for occluder / SortIndex diagnostics (#167).
#[derive(Component, Debug, Clone)]
pub struct CabPartInfo {
    pub prim_state_idx: i32,
    pub sort_index: u32,
    pub texture_name: Option<String>,
    pub shader_name: Option<String>,
    pub cab_matrix_idx: Option<usize>,
}

/// Root node for the cab interior hierarchy (despawn this only).
#[derive(Component)]
pub struct CabInteriorRoot;

/// Parent train entity for the 3D cab mesh (spawned on the live player consist root).
#[derive(Component)]
pub struct CabTrainParent;

/// Lead vehicle entity — cab interior is parented here (same shape space as exterior).
#[derive(Component)]
pub struct CabLeadVehicle;

#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
enum CabInteriorLookup {
    #[default]
    Pending,
    Missing,
    LoadFailed,
    Ready,
}

/// Cached cab lookup so we do not scan disk or spam logs every frame.
#[derive(Resource, Default, Debug)]
pub struct CabInteriorState {
    lookup: CabInteriorLookup,
    cab_shape: Option<PathBuf>,
}

impl CabInteriorState {
    #[allow(dead_code)]
    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Resolve the cabview folder under a trainset (`CABVIEW3D`, `Cabview3d`, …).
pub fn find_cabview_dir(trainset_root: &Path) -> Option<PathBuf> {
    openrailsrs_formats::find_cabview_dir(trainset_root)
}

/// Pick the main cab `.s` (OR uses the file paired with `.cvf`, e.g. `PULLMAN_GR.s`).
pub fn pick_cab_shape_in_dir(cab_dir: &Path) -> Option<PathBuf> {
    openrailsrs_formats::pick_cab_shape_in_dir(cab_dir)
}

/// Resolve `CABVIEW3D/*.s` under a trainset folder (same search order as Open Rails).
pub fn resolve_cab_shape_path(trainset_root: &Path) -> Option<PathBuf> {
    openrailsrs_formats::resolve_cab_assets_scan(trainset_root).map(|a| a.shape_path)
}

fn resolve_cab_assets_for_trainset(
    trainset_root: &Path,
    cab: &openrailsrs_formats::EngineCabView,
) -> Option<openrailsrs_formats::ResolvedCabAssets> {
    openrailsrs_formats::resolve_cab_assets(trainset_root, cab)
}

/// Trainset root for the lead vehicle of the primary consist.
pub fn lead_trainset_root(consist: &TrainConsistScene, route_dir: &Path) -> Option<PathBuf> {
    let mut shape_dir_bufs = consist.shape_search_dirs(route_dir);
    for dir in crate::shapes::shape_search_dirs(route_dir) {
        if !shape_dir_bufs.iter().any(|d| d == &dir) {
            shape_dir_bufs.push(dir);
        }
    }
    let shape_dirs: Vec<&Path> = shape_dir_bufs.iter().map(|p| p.as_path()).collect();
    let vehicles = consist.vehicles_for("primary");
    let shape_name = vehicles.first()?.shape_file.as_deref()?;
    let shape_path = resolve_shape_path_in_dirs(&shape_dirs, shape_name)?;
    let trainset = if shape_path
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n.eq_ignore_ascii_case("shapes"))
    {
        shape_path.parent()?.parent()?
    } else {
        shape_path.parent()?
    };
    Some(trainset.to_path_buf())
}

fn trainset_folder_name(trainset_root: &Path) -> Option<String> {
    trainset_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

fn push_unique_root(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !candidates.iter().any(|p| p == &path) {
        candidates.push(path);
    }
}

/// Candidate trainset folders: scenario stub first, then MSTS/OR `Content/`.
pub fn cab_trainset_candidates(consist: &TrainConsistScene, route_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(root) = lead_trainset_root(consist, route_dir) {
        let name = trainset_folder_name(&root);
        push_unique_root(&mut candidates, root.clone());
        if let Some(name) = name.as_deref() {
            for content in crate::shapes::msts_content_roots() {
                for route_name in route_dir
                    .file_name()
                    .into_iter()
                    .map(|n| n.to_string_lossy().into_owned())
                    .chain(["Chiltern".into()])
                {
                    for trains_sub in [
                        "trains/trainset",
                        "TRAINS/TRAINSET",
                        "trains/TRAINSET",
                        "Trains/Trainset",
                    ] {
                        push_unique_root(
                            &mut candidates,
                            content.join(&route_name).join(trains_sub).join(name),
                        );
                    }
                }
                for trains_sub in ["trains/trainset", "TRAINS/TRAINSET", "trains/TRAINSET"] {
                    push_unique_root(&mut candidates, content.join(trains_sub).join(name));
                }
            }
        }
    }
    candidates
}

pub fn resolve_cab_shape_for_consist(
    consist: &TrainConsistScene,
    route_dir: &Path,
) -> Option<PathBuf> {
    let lead_shape = consist
        .vehicles_for("primary")
        .first()
        .and_then(|v| v.shape_file.as_deref());
    for root in cab_trainset_candidates(consist, route_dir) {
        if let Some(shape_name) = lead_shape {
            let stem = Path::new(shape_name).file_stem()?.to_str()?;
            let eng_path = root.join(format!("{stem}.eng"));
            let eng_path =
                openrailsrs_formats::resolve_path_case_insensitive(&eng_path).unwrap_or(eng_path);
            if let Ok(openrailsrs_formats::MstsFile::Engine(eng)) =
                openrailsrs_formats::parse_msts_file(&eng_path)
            {
                if let Some(assets) = resolve_cab_assets_for_trainset(&root, &eng.cab) {
                    return Some(assets.shape_path);
                }
            }
        }
        if let Some(path) = resolve_cab_shape_path(&root) {
            return Some(path);
        }
    }
    None
}

fn log_cab_missing_once(
    state: &mut CabInteriorState,
    consist: &TrainConsistScene,
    route_dir: &Path,
) {
    if state.lookup != CabInteriorLookup::Pending {
        return;
    }
    state.lookup = CabInteriorLookup::Missing;
    let tried = cab_trainset_candidates(consist, route_dir);
    let listing = tried
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    viewer_log!(
        "openrailsrs-viewer3d: no CABVIEW3D cab shape found (searched: {listing}). \
         Install Open Rails content or set OPENRAILSRS_MSTS_CONTENT to Content/ \
         (see examples/chiltern/README.md)."
    );
}

/// One Open Rails 3D cab / head-out eyepoint ready for the viewer camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orts3dCabViewpointConfig {
    pub head_pos_msts: Vec3,
    /// Look pitch (rad); `.eng` `StartDirection.X` positive = look down → negative pitch.
    pub look_pitch: f32,
    /// Look yaw (rad) from `StartDirection.Y` (rear cab ≈ ±π).
    pub look_yaw: f32,
    /// Optional pitch clamp half-range from `RotationLimit.X` (rad).
    pub pitch_limit: Option<f32>,
    /// Optional yaw clamp half-range from `RotationLimit.Y` (rad).
    pub yaw_limit: Option<f32>,
}

/// Open Rails `ORTS3DCabHeadPos` / `StartDirection` / `RotationLimit` / `HeadOut` from a `.eng`.
#[derive(Clone, Debug, PartialEq)]
pub struct Orts3dCabConfig {
    /// Active / primary eyepoint (first `ORTS3DCab`).
    pub head_pos_msts: Vec3,
    pub look_pitch: f32,
    pub look_yaw: f32,
    pub pitch_limit: Option<f32>,
    pub yaw_limit: Option<f32>,
    pub viewpoints: Vec<Orts3dCabViewpointConfig>,
    pub head_out_msts: Vec<Vec3>,
}

/// Cab shape local transform on the train (same MSTS→train rotation as rolling stock).
pub fn cab_shape_train_transform() -> Transform {
    Transform {
        rotation: msts_shape_to_train_rotation(),
        ..default()
    }
}

fn parse_float_triplet(text: &str, tag: &str) -> Option<Vec3> {
    let start = text.find(tag)? + tag.len();
    let rest = &text[start..];
    let open = rest.find('(')? + 1;
    let close = rest[open..].find(')')? + open;
    let nums: Vec<f32> = rest[open..close]
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() >= 3 {
        Some(Vec3::new(nums[0], nums[1], nums[2]))
    } else {
        None
    }
}

fn viewpoint_from_eng_fields(
    head: [f64; 3],
    start: [f64; 3],
    limit: Option<[f64; 3]>,
) -> Orts3dCabViewpointConfig {
    Orts3dCabViewpointConfig {
        head_pos_msts: Vec3::new(head[0] as f32, head[1] as f32, head[2] as f32),
        look_pitch: -(start[0] as f32).to_radians(),
        look_yaw: (start[1] as f32).to_radians(),
        pitch_limit: limit.map(|l| (l[0] as f32).to_radians().abs()),
        yaw_limit: limit.map(|l| (l[1] as f32).to_radians().abs()),
    }
}

fn orts_3d_cab_from_engine_cab(
    cab: &openrailsrs_formats::EngineCabView,
) -> Option<Orts3dCabConfig> {
    let viewpoints: Vec<Orts3dCabViewpointConfig> = if cab.viewpoints.is_empty() {
        let head = cab.orts_3d_cab_head_pos_m?;
        vec![viewpoint_from_eng_fields(
            head,
            cab.start_direction_deg.unwrap_or([0.0, 0.0, 0.0]),
            cab.rotation_limit_deg,
        )]
    } else {
        cab.viewpoints
            .iter()
            .map(|vp| {
                viewpoint_from_eng_fields(
                    vp.head_pos_m,
                    vp.start_direction_deg,
                    vp.rotation_limit_deg.or(cab.rotation_limit_deg),
                )
            })
            .collect()
    };
    let primary = *viewpoints.first()?;
    let head_out_msts = cab
        .head_out_m
        .iter()
        .map(|p| Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32))
        .collect();
    Some(Orts3dCabConfig {
        head_pos_msts: primary.head_pos_msts,
        look_pitch: primary.look_pitch,
        look_yaw: primary.look_yaw,
        pitch_limit: primary.pitch_limit,
        yaw_limit: primary.yaw_limit,
        viewpoints,
        head_out_msts,
    })
}

/// Parse `ORTS3DCabHeadPos` and `StartDirection` from an MSTS `.eng` file.
pub fn parse_orts_3d_cab_from_eng(eng_path: &Path) -> Option<Orts3dCabConfig> {
    if let Ok(openrailsrs_formats::MstsFile::Engine(eng)) =
        openrailsrs_formats::parse_msts_file(eng_path)
    {
        if let Some(config) = orts_3d_cab_from_engine_cab(&eng.cab) {
            return Some(config);
        }
    }
    parse_orts_3d_cab_from_eng_text(eng_path)
}

fn parse_orts_3d_cab_from_eng_text(eng_path: &Path) -> Option<Orts3dCabConfig> {
    let text = openrailsrs_formats::read_msts_file_case_insensitive(eng_path).ok()?;
    if !text.contains("ORTS3DCab") {
        return None;
    }
    let head_pos_msts = parse_float_triplet(&text, "ORTS3DCabHeadPos")?;
    let start_dir = parse_float_triplet(&text, "StartDirection").unwrap_or(Vec3::ZERO);
    let limit = parse_float_triplet(&text, "RotationLimit");
    let vp = Orts3dCabViewpointConfig {
        head_pos_msts,
        look_pitch: -start_dir.x.to_radians(),
        look_yaw: start_dir.y.to_radians(),
        pitch_limit: limit.map(|l| l.x.to_radians().abs()),
        yaw_limit: limit.map(|l| l.y.to_radians().abs()),
    };
    Some(Orts3dCabConfig {
        head_pos_msts: vp.head_pos_msts,
        look_pitch: vp.look_pitch,
        look_yaw: vp.look_yaw,
        pitch_limit: vp.pitch_limit,
        yaw_limit: vp.yaw_limit,
        viewpoints: vec![vp],
        head_out_msts: Vec::new(),
    })
}

/// Trainset root for a vehicle `.s` path (`…/SHAPES/foo.s` or `…/Trainset/foo.s`).
pub fn trainset_root_from_shape_path(shape_path: &Path) -> Option<PathBuf> {
    let parent = shape_path.parent()?;
    if parent
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("shapes"))
    {
        parent.parent().map(|p| p.to_path_buf())
    } else {
        Some(parent.to_path_buf())
    }
}

/// Resolve ORTS 3D cab eyepoint for a lead vehicle (searches all trainset dirs).
pub fn orts_3d_cab_for_vehicle(
    shape_dirs: &[&Path],
    shape_file: &str,
    route_dir: &Path,
) -> Option<Orts3dCabConfig> {
    let stem = Path::new(shape_file).file_stem()?.to_str()?;
    if let Some(name) = crate::shapes::trainset_name_from_shape_search(shape_dirs, shape_file) {
        for root in crate::shapes::or_content_trainset_roots(route_dir, &name) {
            let eng = root.join(format!("{stem}.eng"));
            if let Some(config) = parse_orts_3d_cab_from_eng(&eng) {
                return Some(config);
            }
            if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&eng) {
                if let Some(config) = parse_orts_3d_cab_from_eng(&resolved) {
                    return Some(config);
                }
            }
        }
    }
    for dir in shape_dirs {
        let eng = dir.join(format!("{stem}.eng"));
        if let Some(config) = parse_orts_3d_cab_from_eng(&eng) {
            return Some(config);
        }
        if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&eng) {
            if let Some(config) = parse_orts_3d_cab_from_eng(&resolved) {
                return Some(config);
            }
        }
    }
    let shape_path = crate::shapes::resolve_vehicle_shape_path(shape_dirs, shape_file, route_dir)?;
    orts_3d_cab_from_shape_path(&shape_path)
}

/// Resolve ORTS 3D cab eyepoint from the lead vehicle exterior shape path.
pub fn orts_3d_cab_from_shape_path(shape_path: &Path) -> Option<Orts3dCabConfig> {
    let trainset_root = trainset_root_from_shape_path(shape_path)?;
    let stem = shape_path.file_stem()?.to_str()?;
    let eng = trainset_root.join(format!("{stem}.eng"));
    parse_orts_3d_cab_from_eng(&eng).or_else(|| {
        openrailsrs_formats::resolve_path_case_insensitive(&eng)
            .as_deref()
            .and_then(parse_orts_3d_cab_from_eng)
    })
}

impl Orts3dCabConfig {
    /// Eyepoint in train-local metres (transformed with the lead vehicle placement matrix).
    pub fn head_pos_in_train(&self, vehicle_placement: Transform) -> Vec3 {
        vehicle_placement
            .transform_point(crate::shapes::msts_shape_vec3_to_bevy(self.head_pos_msts))
    }

    /// True when `StartDirection.Y` selects a rear-facing 3D cab (Open Rails CabViewType.Rear).
    pub fn primary_is_rear_cab(&self) -> bool {
        let yaw_deg = self.look_yaw.to_degrees().abs();
        (90.0..=270.0).contains(&yaw_deg)
    }
}

/// Local transform for a 3D cab mesh parented to the driver camera.
///
/// Meshes are in Bevy shape space ([`msts_shape_vec3_to_bevy`]); this offset places
/// `ORTS3DCabHeadPos` at the camera origin.  Mesh −Z points toward the windshield,
/// matching the Bevy camera look axis (−Z).
pub fn cab_transform_on_camera(head_msts: Vec3) -> Transform {
    let head = crate::shapes::msts_shape_vec3_to_bevy(head_msts);
    Transform {
        translation: -head,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    }
}

/// Re-export for cab tuning docs / tests.
pub use crate::shapes::cab_interior_albedo_boost;

#[allow(clippy::too_many_arguments)]
pub fn sync_cab_interior(
    follow: Res<CameraFollowMode>,
    consist: Res<TrainConsistScene>,
    assets: Res<RouteAssets>,
    mut state: ResMut<CabInteriorState>,
    mut cvf_state: ResMut<cab_cvf::CabCvfState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut or_materials: ResMut<Assets<crate::or_cab_material::OrCabMaterial>>,
    lead_car: Query<Entity, With<CabLeadVehicle>>,
    existing: Query<Entity, With<CabInteriorRoot>>,
    driver_cab: Option<Res<crate::camera::LiveDriverCab>>,
) {
    let driver = *follow == CameraFollowMode::DriverCam;
    let cab2d = follow.is_cab2d();
    if !driver && !cab2d {
        cab_cvf::reset_cab_cvf_state(&mut cvf_state);
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }

    // Resolve cab shape / CVF for both 3D and 2D cab views.
    let cab_shape = if let Some(path) = state.cab_shape.clone() {
        Some(path)
    } else if state.lookup == CabInteriorLookup::Missing
        || state.lookup == CabInteriorLookup::LoadFailed
    {
        None
    } else if let Some(path) = resolve_cab_shape_for_consist(&consist, &assets.route_dir) {
        state.cab_shape = Some(path.clone());
        state.lookup = CabInteriorLookup::Ready;
        Some(path)
    } else {
        log_cab_missing_once(&mut state, &consist, &assets.route_dir);
        None
    };

    let Some(cab_shape) = cab_shape else {
        return;
    };

    if let Some(trainset) = lead_trainset_root(&consist, &assets.route_dir) {
        let vehicles = consist.vehicles_for("primary");
        if let Some(shape_name) = vehicles.first().and_then(|v| v.shape_file.as_deref()) {
            let stem = Path::new(shape_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("engine");
            let eng_path = trainset.join(format!("{stem}.eng"));
            let eng_path =
                openrailsrs_formats::resolve_path_case_insensitive(&eng_path).unwrap_or(eng_path);
            if let Ok(openrailsrs_formats::MstsFile::Engine(eng)) =
                openrailsrs_formats::parse_msts_file(&eng_path)
            {
                cab_cvf::load_cab_cvf_runtime(&mut cvf_state, &trainset, &eng.cab, &cab_shape);
            }
        }
    }

    // 2D Cab: CVF runtime only — no CABVIEW3D mesh (#152).
    if cab2d {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }

    if !existing.is_empty() {
        return;
    }

    let _head_msts = driver_cab.as_ref().and_then(|c| c.head_msts);
    let cab_shape_file = ShapeFile::from_path(&cab_shape).ok();

    let lever_matrices: HashSet<usize> = cvf_state
        .runtime
        .as_ref()
        .map(cab_cvf::cab_lever_matrix_indices)
        .unwrap_or_default();

    let tex_dirs: Vec<PathBuf> = texture_search_dirs_for_shape(&cab_shape, &assets.route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let mut texture_cache = HashMap::new();
    let Some(asset) = load_cab_interior_render_asset_from_path(
        &cab_shape,
        &tex_refs,
        Some(2.0),
        &mut meshes,
        &mut images,
        &mut materials,
        &mut or_materials,
        &mut texture_cache,
        Color::srgb(0.35, 0.38, 0.42),
        &lever_matrices,
    ) else {
        if state.lookup != CabInteriorLookup::LoadFailed {
            state.lookup = CabInteriorLookup::LoadFailed;
            viewer_log!(
                "openrailsrs-viewer3d: failed to load cab shape {}",
                cab_shape.display()
            );
        }
        return;
    };

    let textured = asset.parts.iter().filter(|p| p.has_texture).count();
    let or_textured = asset
        .parts
        .iter()
        .filter(|p| p.or_cab_material.is_some())
        .count();
    let mut shader_kinds: std::collections::HashMap<i32, u32> = std::collections::HashMap::new();
    for part in &asset.parts {
        if let Some(h) = part.or_cab_material.as_ref() {
            if let Some(m) = or_materials.get(h) {
                *shader_kinds.entry(m.params.shader_kind as i32).or_insert(0) += 1;
            }
        }
    }
    if !shader_kinds.is_empty() {
        viewer_log!(
            "openrailsrs-viewer3d: cab OR shader kinds (gpu id → count): {:?}",
            shader_kinds
        );
    }
    cab_diag::log_cab_interior_part_diagnostics(
        &cab_shape,
        &tex_dirs,
        &asset,
        &meshes,
        &materials,
        &or_materials,
    );
    viewer_log!(
        "openrailsrs-viewer3d: cab interior from {} ({} part(s), {} textured, {} OR shader, lead-attached)",
        cab_shape.display(),
        asset.parts.len(),
        textured,
        or_textured,
    );

    if let Some(cab_res) = driver_cab.as_ref() {
        if let Some(head_msts) = cab_res.head_msts {
            let cab_mesh_refs: Vec<&Mesh> = asset
                .parts
                .iter()
                .filter_map(|p| meshes.get(&p.mesh))
                .collect();
            let aligned = crate::shapes::orts_head_inside_cab_aabb(head_msts, &cab_mesh_refs);
            viewer_log!(
                "openrailsrs-viewer3d: cab alignment ORTS head in cab AABB (MSTS): {aligned}"
            );
        }
    }

    let Ok(lead_entity) = lead_car.single() else {
        return;
    };

    commands.entity(lead_entity).with_children(|cab_parent| {
        cab_parent
            .spawn((
                CabInteriorRoot,
                CabInteriorMarker,
                NotShadowCaster,
                Transform::IDENTITY,
                Visibility::Visible,
                Name::new("cab:interior:root"),
            ))
            .with_children(|root| {
                let screen_ctrls: Vec<&openrailsrs_formats::CabControl> = cvf_state
                    .runtime
                    .as_ref()
                    .map(|rt| crate::cab_screen::screen_controls(&rt.cvf))
                    .unwrap_or_default();
                let mut screen_claimed = vec![false; screen_ctrls.len()];
                // Open Rails cab 3D uses SceneryShader (ambient/sun), not Bevy point lights.
                for (pi, part) in asset.parts.iter().enumerate() {
                    let matrix_idx = part.cab_matrix_idx.or_else(|| {
                        cab_shape_file.as_ref().and_then(|shape| {
                            crate::cab_cvf::matrix_idx_for_prim_state(shape, part.prim_state_idx)
                                .filter(|idx| {
                                    cvf_state
                                        .runtime
                                        .as_ref()
                                        .is_some_and(|rt| rt.matrix_drivers.contains_key(idx))
                                })
                        })
                    });
                    let pivot_at_mesh = if part.lever_pivot_at_mesh_center {
                        part.bounds_center
                    } else {
                        None
                    };
                    let initial_transform = matrix_idx.and_then(|idx| {
                        cvf_state.runtime.as_ref().and_then(|rt| {
                            rt.matrix_drivers
                                .get(&idx)
                                .map(|_| static_lever_transform(&rt.shape, idx, pivot_at_mesh))
                        })
                    });
                    let mut entity = root.spawn((
                        CabInteriorMarker,
                        CabPartInfo {
                            prim_state_idx: part.prim_state_idx,
                            sort_index: part.sort_index,
                            texture_name: part.texture_name.clone(),
                            shader_name: part.shader_name.clone(),
                            cab_matrix_idx: part.cab_matrix_idx,
                        },
                        NotShadowCaster,
                        NotShadowReceiver,
                        Mesh3d(part.mesh.clone()),
                        initial_transform.unwrap_or(Transform::default()),
                        Visibility::Visible,
                        Name::new(format!("cab:interior:part:{pi}")),
                    ));
                    if let Some(or_mat) = part.or_cab_material.clone() {
                        entity.insert(MeshMaterial3d(or_mat));
                    } else {
                        entity.insert(MeshMaterial3d(part.material.clone()));
                    }
                    if let Some(matrix_idx) = matrix_idx {
                        if cvf_state
                            .runtime
                            .as_ref()
                            .is_some_and(|rt| rt.matrix_drivers.contains_key(&matrix_idx))
                        {
                            entity.insert(CabCvfPart {
                                matrix_idx,
                                pivot_at_mesh,
                                local_spin_axis: part.lever_local_axis,
                            });
                        }
                    }
                    let _ = crate::cab_screen::try_attach_screen_to_part(
                        &mut entity,
                        part,
                        &screen_ctrls,
                        &mut screen_claimed,
                        &mut images,
                        &mut materials,
                        &mut or_materials,
                    );
                }
                if let Some(runtime) = cvf_state.runtime.as_ref() {
                    crate::cab_native_instruments::spawn_cab_native_instruments(
                        root,
                        runtime,
                        &cab_shape,
                        &assets.route_dir,
                        &mut meshes,
                        &mut images,
                        &mut materials,
                    );
                }
            });
    });

    if *follow == CameraFollowMode::DriverCam {
        viewer_log!(
            "openrailsrs-viewer3d: cab interior ready — {} part(s) in driver view",
            asset.parts.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rolling_stock::ConsistVehicleVisual;
    use std::path::PathBuf;

    fn cab_fixture_eng() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/fixtures/cab/orts3d_viewpoints.eng")
    }

    fn msts_content_root() -> Option<PathBuf> {
        std::env::var_os("OPENRAILSRS_MSTS_CONTENT").map(PathBuf::from)
    }

    fn or_pullman_trainset() -> Option<PathBuf> {
        let root = msts_content_root()?;
        let trainset = root.join("Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman");
        trainset.is_dir().then_some(trainset)
    }

    #[test]
    fn resolve_cab_shape_prefers_cab_s() {
        let dir =
            std::env::temp_dir().join(format!("openrailsrs_cabview_test_{}", std::process::id()));
        let cab_dir = dir.join("CABVIEW3D");
        std::fs::create_dir_all(&cab_dir).unwrap();
        std::fs::write(cab_dir.join("other.s"), b"").unwrap();
        std::fs::write(cab_dir.join("cab.s"), b"").unwrap();
        let resolved = resolve_cab_shape_path(&dir).unwrap();
        assert!(resolved.ends_with("cab.s"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_cab_shape_prefers_cvf_pair() {
        let dir =
            std::env::temp_dir().join(format!("openrailsrs_cabview_pair_{}", std::process::id()));
        let cab_dir = dir.join("Cabview3d");
        std::fs::create_dir_all(&cab_dir).unwrap();
        std::fs::write(cab_dir.join("cab.s"), b"").unwrap();
        std::fs::write(cab_dir.join("PULLMAN_GR.s"), b"").unwrap();
        std::fs::write(cab_dir.join("PULLMAN_GR.cvf"), b"").unwrap();
        let resolved = resolve_cab_shape_path(&dir).unwrap();
        assert!(resolved.ends_with("PULLMAN_GR.s"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_cab_shape_prefers_3d_over_2d_cabview() {
        let dir =
            std::env::temp_dir().join(format!("openrailsrs_cabview_3d_{}", std::process::id()));
        let cab2d = dir.join("CabView");
        let cab3d = dir.join("Cabview3d");
        std::fs::create_dir_all(&cab2d).unwrap();
        std::fs::create_dir_all(&cab3d).unwrap();
        std::fs::write(cab2d.join("RF_BP_DMBS.cvf"), b"").unwrap();
        std::fs::write(cab3d.join("PULLMAN_GR.s"), b"").unwrap();
        std::fs::write(cab3d.join("PULLMAN_GR.cvf"), b"").unwrap();
        let resolved = resolve_cab_shape_path(&dir).unwrap();
        assert!(resolved.ends_with("PULLMAN_GR.s"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman"]
    fn chiltern_or_pullman_cab_shape_loads_when_content_present() {
        let trainset = or_pullman_trainset().expect("OPENRAILSRS_MSTS_CONTENT Pullman trainset");
        let shape = resolve_cab_shape_path(&trainset).expect("PULLMAN cab shape");
        let loaded = crate::shapes::load_shape_from_path(&shape, Some(2.0));
        assert!(loaded.is_some(), "parse {}", shape.display());
        assert!(!loaded.unwrap().parts.is_empty());
    }

    #[test]
    fn lead_trainset_root_from_chiltern_fixture() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let mut consist = TrainConsistScene::default();
        consist.set_scenario_dir(route.clone());
        consist.by_label.insert(
            "primary".into(),
            vec![ConsistVehicleVisual {
                name: "DMBSA".into(),
                shape_file: Some("RF_WP_DMBSA.s".into()),
                length_m: 20.879,
                offset_m: 0.0,
                flipped: false,
            }],
        );
        let root = lead_trainset_root(&consist, &route).expect("trainset root");
        assert!(root.ends_with("RF_Blue_Pullman"));
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman"]
    fn resolve_cab_shape_for_consist_finds_or_content() {
        let _ = or_pullman_trainset().expect("OPENRAILSRS_MSTS_CONTENT Pullman trainset");
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let mut consist = TrainConsistScene::default();
        consist.set_scenario_dir(route.clone());
        consist.by_label.insert(
            "primary".into(),
            vec![ConsistVehicleVisual {
                name: "DMBSA".into(),
                shape_file: Some("RF_WP_DMBSA.s".into()),
                length_m: 20.879,
                offset_m: 0.0,
                flipped: false,
            }],
        );
        let shape = resolve_cab_shape_for_consist(&consist, &route).expect("OR cab shape");
        assert!(shape.ends_with("PULLMAN_GR.s"));
    }

    #[test]
    fn parse_fixture_orts_3d_cab_head_pos_and_limits() {
        let eng = cab_fixture_eng();
        let config = parse_orts_3d_cab_from_eng(&eng).expect("ORTS3DCab fixture");
        assert!((config.head_pos_msts.x + 0.8).abs() < 1e-3);
        assert!((config.head_pos_msts.y - 2.875).abs() < 1e-3);
        assert!((config.head_pos_msts.z - 8.60).abs() < 1e-2);
        assert!((config.look_pitch + 15f32.to_radians()).abs() < 1e-3);
        assert!(config.look_yaw.abs() < 1e-4);
        assert!((config.pitch_limit.unwrap() - 10f32.to_radians()).abs() < 1e-3);
        assert!((config.yaw_limit.unwrap() - 90f32.to_radians()).abs() < 1e-3);
        assert_eq!(config.viewpoints.len(), 2);
        assert!((config.viewpoints[1].look_yaw.abs() - std::f32::consts::PI).abs() < 1e-3);
        assert_eq!(config.head_out_msts.len(), 2);
        let vehicle_t = crate::shapes::vehicle_authored_frame_transform(0.0, false);
        let head = config.head_pos_in_train(vehicle_t);
        assert!((head.x - (-8.60)).abs() < 1e-2);
        assert!((head.y - 2.875).abs() < 1e-3);
    }

    #[test]
    fn authored_frame_keeps_orts_head_in_lead_local_space() {
        let config = parse_orts_3d_cab_from_eng(&cab_fixture_eng()).expect("fixture");
        let placement = crate::shapes::vehicle_authored_frame_transform(0.0, false);
        let head_bevy = crate::shapes::msts_shape_vec3_to_bevy(config.head_pos_msts);
        let head_train = placement.transform_point(head_bevy);
        // Same transform as exterior/cab lead frame: no AABB shift between spaces.
        assert!((head_train - config.head_pos_in_train(placement)).length() < 1e-4);
        assert!((head_train.x - (-8.60)).abs() < 1e-2);
        assert!(!config.primary_is_rear_cab());
        let rear = viewpoint_from_eng_fields([-0.8, 2.875, 8.60], [15.0, 180.0, 0.0], None);
        assert!((rear.look_yaw.to_degrees().abs() - 180.0).abs() < 1e-2);
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman shapes"]
    fn pullman_cab_geometry_diagnostic() {
        let trainset = or_pullman_trainset().expect("OPENRAILSRS_MSTS_CONTENT Pullman trainset");
        let cab = trainset.join("Cabview3d/PULLMAN_GR.s");
        let ext = trainset.join("RF_WP_DMBSA.s");
        assert!(cab.is_file() && ext.is_file(), "missing Pullman shapes");
        let cab_loaded = crate::shapes::load_shape_from_path(&cab, Some(2.0)).unwrap();
        let ext_loaded = crate::shapes::load_shape_from_path(&ext, Some(50.0)).unwrap();
        let cab_meshes: Vec<&Mesh> = cab_loaded.parts.iter().map(|p| &p.mesh).collect();
        let head_msts = Vec3::new(-0.8, 2.875, 8.60);
        assert!(
            crate::shapes::orts_head_inside_cab_aabb(head_msts, &cab_meshes),
            "ORTS head must lie inside cab shape AABB (MSTS metres)"
        );
        assert!(
            crate::shapes::orts_head_inside_cab_train_space(
                head_msts,
                &ext_loaded.mesh,
                &cab_meshes,
                0.0,
                20.879,
            ),
            "ORTS head must lie inside cab AABB after authored cab frame transform"
        );
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman cab textures"]
    fn pullman_cab_render_asset_textures_load_from_or_content() {
        let trainset = or_pullman_trainset().expect("OPENRAILSRS_MSTS_CONTENT Pullman trainset");
        let cab = trainset.join("Cabview3d/PULLMAN_GR.s");
        assert!(cab.is_file(), "missing PULLMAN_GR.s");
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let tex_dirs: Vec<PathBuf> = crate::shapes::texture_search_dirs_for_shape(&cab, &route);
        let tex_refs: Vec<&std::path::Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let mut meshes = bevy::prelude::Assets::<bevy::prelude::Mesh>::default();
        let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
        let mut materials = bevy::prelude::Assets::<bevy::prelude::StandardMaterial>::default();
        let mut or_materials =
            bevy::prelude::Assets::<crate::or_cab_material::OrCabMaterial>::default();
        let mut texture_cache = std::collections::HashMap::new();
        let asset = crate::shapes::load_cab_interior_render_asset_from_path(
            &cab,
            &tex_refs,
            Some(2.0),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut or_materials,
            &mut texture_cache,
            bevy::prelude::Color::srgb(0.35, 0.38, 0.42),
            &HashSet::new(),
        )
        .expect("cab render asset");
        let textured = asset.parts.iter().filter(|p| p.has_texture).count();
        let with_or_shader = asset
            .parts
            .iter()
            .filter(|p| p.or_cab_material.is_some())
            .count();
        let with_fullbright = asset
            .parts
            .iter()
            .filter(|p| {
                p.or_cab_material.as_ref().is_some_and(|h| {
                    or_materials
                        .get(h)
                        .is_some_and(|m| m.params.shader_kind >= 4.0)
                })
            })
            .count();
        let with_opaque = asset
            .parts
            .iter()
            .filter(|p| {
                p.or_cab_material.as_ref().is_some_and(|h| {
                    or_materials
                        .get(h)
                        .is_some_and(|m| matches!(m.alpha_mode, AlphaMode::Opaque))
                })
            })
            .count();
        let untextured: Vec<_> = asset
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.has_texture)
            .map(|(i, p)| (i, p.prim_state_idx))
            .collect();
        eprintln!("untextured parts: {untextured:?}");
        assert!(
            textured >= 30,
            "expected cab ACE textures, got {textured}/{} untextured={untextured:?}",
            asset.parts.len()
        );
        assert!(
            with_or_shader >= 30,
            "cab interior should use OR shader materials, got {with_or_shader}"
        );
        assert!(
            with_fullbright + with_opaque >= 30,
            "cab OR materials should be mostly opaque or FullBright, fullbright={with_fullbright} opaque={with_opaque}"
        );
    }

    #[test]
    fn cab_interior_albedo_boost_default() {
        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_ALBEDO");
        }
        assert!((cab_interior_albedo_boost() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn cab_transform_on_camera_places_head_at_origin() {
        let head_msts = Vec3::new(-0.8, 2.875, 8.60);
        let head_bevy = crate::shapes::msts_shape_vec3_to_bevy(head_msts);
        let local = cab_transform_on_camera(head_msts);
        let eye = local.transform_point(head_bevy);
        assert!(eye.length() < 1e-3, "eye={eye:?}");
        // Windshield / mesh forward (−Z) → camera look axis (−Z).
        let forward = local.rotation * Vec3::NEG_Z;
        assert!(
            (forward.z + 1.0).abs() < 1e-2 && forward.x.abs() < 1e-2,
            "forward={forward:?}"
        );
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman cab shape"]
    fn cab_transform_on_camera_pullman_head_in_mesh_space() {
        let trainset = or_pullman_trainset().expect("OPENRAILSRS_MSTS_CONTENT Pullman trainset");
        let cab = trainset.join("Cabview3d/PULLMAN_GR.s");
        assert!(cab.is_file(), "missing PULLMAN_GR.s");
        let head_msts = Vec3::new(-0.8, 2.875, 8.60);
        let head_bevy = crate::shapes::msts_shape_vec3_to_bevy(head_msts);
        let mount = cab_transform_on_camera(head_msts);
        let loaded = crate::shapes::load_shape_from_path(&cab, Some(2.0)).unwrap();
        let cab_meshes: Vec<&Mesh> = loaded.parts.iter().map(|p| &p.mesh).collect();
        assert!(crate::shapes::orts_head_inside_cab_aabb(
            head_msts,
            &cab_meshes
        ));
        let eye_in_mount = mount.transform_point(head_bevy);
        assert!(
            eye_in_mount.length() < 1e-3,
            "ORTS eyepoint must sit at camera origin, got {eye_in_mount:?}"
        );
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT with Chiltern RF_Blue_Pullman"]
    fn orts_3d_cab_from_or_trainset_root_shape_path() {
        let trainset = or_pullman_trainset().expect("OPENRAILSRS_MSTS_CONTENT Pullman trainset");
        let shape = trainset.join("RF_WP_DMBSA.s");
        assert!(shape.is_file(), "missing RF_WP_DMBSA.s");
        let config = orts_3d_cab_from_shape_path(&shape).expect("ORTS from OR root .s");
        assert!((config.head_pos_msts.y - 2.875).abs() < 1e-3);
    }

    #[test]
    fn orts_3d_cab_from_chiltern_stub_eng() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let shape_dirs: Vec<PathBuf> = vec![route.join("trains/RF_Blue_Pullman")];
        let refs: Vec<&Path> = shape_dirs.iter().map(|p| p.as_path()).collect();
        let config = orts_3d_cab_for_vehicle(refs.as_slice(), "RF_WP_DMBSA.s", &route)
            .expect("ORTS from stub eng");
        assert!((config.head_pos_msts.z - 8.60).abs() < 1e-2);
    }

    #[test]
    fn cab_missing_logs_once() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let mut consist = TrainConsistScene::default();
        consist.set_scenario_dir(route.clone());
        consist.by_label.insert(
            "primary".into(),
            vec![ConsistVehicleVisual {
                name: "DMBSA".into(),
                shape_file: Some("RF_WP_DMBSA.s".into()),
                length_m: 20.879,
                offset_m: 0.0,
                flipped: false,
            }],
        );
        let mut state = CabInteriorState::default();
        log_cab_missing_once(&mut state, &consist, &route);
        assert_eq!(state.lookup, CabInteriorLookup::Missing);
        log_cab_missing_once(&mut state, &consist, &route);
        assert_eq!(state.lookup, CabInteriorLookup::Missing);
    }
}

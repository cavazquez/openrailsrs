//! 3D cab interior from MSTS `CABVIEW3D/` (Open Rails `ThreeDimentionCabCamera`).
//!
//! OR attaches the cab shape (`.s` + `.ace` in `CABVIEW3D/`, driven by `.cvf`) to the
//! lead vehicle; the driver camera uses `ORTS3DCabHeadPos` from the `.eng`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::camera::CameraFollowMode;
use crate::rolling_stock::TrainConsistScene;
use crate::shapes::{
    RouteAssets, load_shape_render_asset_from_path, msts_content_root,
    msts_shape_to_train_rotation, resolve_shape_path_in_dirs, texture_search_dirs_for_shape,
};
use crate::viewer_log;

/// Marker on cab-interior entities parented to the camera in driver view.
#[derive(Component)]
pub struct CabInteriorMarker;

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
    let Ok(entries) = std::fs::read_dir(trainset_root) else {
        return None;
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.eq_ignore_ascii_case("cabview3d")
            || name.eq_ignore_ascii_case("cabview")
            || name.eq_ignore_ascii_case("cabview2d")
        {
            candidates.push(path);
        }
    }
    candidates.sort_by(|a, b| {
        let a3d = a
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains('3'));
        let b3d = b
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().contains('3'));
        b3d.cmp(&a3d)
    });
    for dir in &candidates {
        if pick_cab_shape_in_dir(dir).is_some() {
            return Some(dir.clone());
        }
    }
    None
}

/// Pick the main cab `.s` (OR uses the file paired with `.cvf`, e.g. `PULLMAN_GR.s`).
pub fn pick_cab_shape_in_dir(cab_dir: &Path) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(cab_dir) else {
        return None;
    };
    let mut shapes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("s"))
        {
            shapes.push(path);
        }
    }
    for path in &shapes {
        let cvf = path.with_extension("cvf");
        if cvf.is_file() {
            return Some(path.clone());
        }
        if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&cvf) {
            if resolved.is_file() {
                return Some(path.clone());
            }
        }
    }
    for preferred in ["cab.s", "Cab.s", "CAB.s"] {
        let path = cab_dir.join(preferred);
        if path.is_file() {
            return Some(path);
        }
        if let Some(resolved) = openrailsrs_formats::resolve_path_case_insensitive(&path) {
            return Some(resolved);
        }
    }
    shapes.into_iter().next()
}

/// Resolve `CABVIEW3D/*.s` under a trainset folder (same search order as Open Rails).
pub fn resolve_cab_shape_path(trainset_root: &Path) -> Option<PathBuf> {
    let cab_dir = find_cabview_dir(trainset_root)?;
    pick_cab_shape_in_dir(&cab_dir)
}

/// Trainset root for the lead vehicle of the primary consist.
pub fn lead_trainset_root(consist: &TrainConsistScene, route_dir: &Path) -> Option<PathBuf> {
    let shape_dir_bufs = consist.shape_search_dirs(route_dir);
    let shape_dirs: Vec<&Path> = shape_dir_bufs.iter().map(|p| p.as_path()).collect();
    let vehicles = consist.vehicles_for("primary");
    let shape_name = vehicles.first()?.shape_file.as_deref()?;
    let shape_path = resolve_shape_path_in_dirs(&shape_dirs, shape_name)?;
    shape_path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
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
        if let (Some(content), Some(name)) = (msts_content_root(), name.as_deref()) {
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
    candidates
}

pub fn resolve_cab_shape_for_consist(
    consist: &TrainConsistScene,
    route_dir: &Path,
) -> Option<PathBuf> {
    cab_trainset_candidates(consist, route_dir)
        .iter()
        .find_map(|root| resolve_cab_shape_path(root))
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

/// Open Rails `ORTS3DCabHeadPos` / `StartDirection` from a `.eng` file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Orts3dCabConfig {
    pub head_pos_msts: Vec3,
    pub look_pitch: f32,
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

/// Parse `ORTS3DCabHeadPos` and `StartDirection` from an MSTS `.eng` file.
pub fn parse_orts_3d_cab_from_eng(eng_path: &Path) -> Option<Orts3dCabConfig> {
    let text = openrailsrs_formats::read_msts_file_case_insensitive(eng_path).ok()?;
    if !text.contains("ORTS3DCab") {
        return None;
    }
    let head_pos_msts = parse_float_triplet(&text, "ORTS3DCabHeadPos")?;
    let start_dir = parse_float_triplet(&text, "StartDirection").unwrap_or(Vec3::ZERO);
    // MSTS StartDirection X is pitch (degrees, positive = look down).
    let look_pitch = -start_dir.x.to_radians();
    Some(Orts3dCabConfig {
        head_pos_msts,
        look_pitch,
    })
}

/// Resolve ORTS 3D cab eyepoint for a lead vehicle (searches all trainset dirs).
pub fn orts_3d_cab_for_vehicle(shape_dirs: &[&Path], shape_file: &str) -> Option<Orts3dCabConfig> {
    let stem = Path::new(shape_file).file_stem()?.to_str()?;
    for dir in shape_dirs {
        let eng = dir.join(format!("{stem}.eng"));
        if let Some(config) = parse_orts_3d_cab_from_eng(&eng) {
            return Some(config);
        }
    }
    let shape_path = resolve_shape_path_in_dirs(shape_dirs, shape_file)?;
    orts_3d_cab_from_shape_path(&shape_path)
}

/// Resolve ORTS 3D cab eyepoint from the lead vehicle exterior shape path.
pub fn orts_3d_cab_from_shape_path(shape_path: &Path) -> Option<Orts3dCabConfig> {
    let trainset_root = shape_path.parent()?.parent()?;
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
    pub fn head_pos_in_train(self, vehicle_placement: Transform) -> Vec3 {
        vehicle_placement.transform_point(self.head_pos_msts)
    }
}

/// Local transform for a 3D cab mesh parented to the driver camera.
///
/// Shape vertices are in MSTS space (+Z forward); the Bevy camera looks down -Z.
/// The eyepoint `head_msts` is moved to the camera origin.
pub fn cab_transform_on_camera(head_msts: Vec3, vehicle: Transform) -> Transform {
    // MSTS +Z (windshield / forward) → Bevy camera look axis -Z
    let align = Quat::from_rotation_y(std::f32::consts::PI);
    let scaled_head = head_msts * vehicle.scale;
    Transform {
        rotation: align,
        scale: vehicle.scale,
        translation: -align.mul_vec3(scaled_head),
    }
}

fn cab_interior_material(
    part: &crate::shapes::ShapePartAsset,
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    let Some(base) = materials.get(&part.material) else {
        return materials.add(StandardMaterial {
            unlit: true,
            double_sided: true,
            alpha_mode: AlphaMode::Opaque,
            base_color: Color::srgb(0.55, 0.58, 0.62),
            ..default()
        });
    };
    materials.add(StandardMaterial {
        unlit: true,
        double_sided: true,
        alpha_mode: AlphaMode::Opaque,
        base_color: Color::WHITE,
        base_color_texture: base.base_color_texture.clone(),
        emissive: LinearRgba::new(0.35, 0.35, 0.38, 1.0),
        ..default()
    })
}

#[allow(clippy::too_many_arguments)]
pub fn sync_cab_interior(
    follow: Res<CameraFollowMode>,
    consist: Res<TrainConsistScene>,
    assets: Res<RouteAssets>,
    mut state: ResMut<CabInteriorState>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    lead_car: Query<Entity, With<CabLeadVehicle>>,
    existing: Query<Entity, With<CabInteriorRoot>>,
) {
    let driver = *follow == CameraFollowMode::DriverCam;
    if !driver {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }

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

    let Ok(lead_car) = lead_car.single() else {
        return;
    };

    let tex_dirs: Vec<PathBuf> = texture_search_dirs_for_shape(&cab_shape, &assets.route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let mut texture_cache = HashMap::new();
    let Some(asset) = load_shape_render_asset_from_path(
        &cab_shape,
        &tex_refs,
        Some(2.0),
        &mut meshes,
        &mut images,
        &mut materials,
        &mut texture_cache,
        Color::srgb(0.35, 0.38, 0.42),
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
    for (pi, part) in asset.parts.iter().enumerate() {
        if part.has_texture {
            continue;
        }
        let extent = crate::shapes::mesh_aabb(meshes.get(&part.mesh).expect("cab part mesh"))
            .map(|(mn, mx)| mx - mn);
        viewer_log!(
            "openrailsrs-viewer3d: cab part {pi} prim={} no texture (ext={extent:?})",
            part.prim_state_idx,
        );
    }
    viewer_log!(
        "openrailsrs-viewer3d: cab interior from {} ({} part(s), {} textured, lead-car attached)",
        cab_shape.display(),
        asset.parts.len(),
        textured,
    );

    commands.entity(lead_car).with_children(|cab_root| {
        cab_root
            .spawn((
                CabInteriorRoot,
                CabInteriorMarker,
                NotShadowCaster,
                Transform::default(),
                Visibility::Visible,
                Name::new("cab:interior:root"),
            ))
            .with_children(|root| {
                root.spawn((
                    PointLight {
                        intensity: 120_000.0,
                        range: 12.0,
                        shadows_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(0.0, 2.5, 4.0),
                    Name::new("cab:interior:light"),
                ));
                for (pi, part) in asset.parts.iter().enumerate() {
                    let material = cab_interior_material(part, &mut materials);
                    root.spawn((
                        CabInteriorMarker,
                        NotShadowCaster,
                        Mesh3d(part.mesh.clone()),
                        MeshMaterial3d(material),
                        Transform::default(),
                        Visibility::Visible,
                        Name::new(format!("cab:interior:part:{pi}")),
                    ));
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rolling_stock::ConsistVehicleVisual;
    use std::path::PathBuf;

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
    fn chiltern_or_pullman_cab_shape_loads_when_content_present() {
        let content = PathBuf::from("/home/cristian/Documentos/Open Rails/Content");
        if !content.is_dir() {
            return;
        }
        let trainset = content.join("Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman");
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
            }],
        );
        let root = lead_trainset_root(&consist, &route).expect("trainset root");
        assert!(root.ends_with("RF_Blue_Pullman"));
    }

    #[test]
    fn resolve_cab_shape_for_consist_finds_or_content() {
        let content = PathBuf::from("/home/cristian/Documentos/Open Rails/Content");
        if !content.is_dir() {
            return;
        }
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
            }],
        );
        let shape = resolve_cab_shape_for_consist(&consist, &route).expect("OR cab shape");
        assert!(shape.ends_with("PULLMAN_GR.s"));
    }

    #[test]
    fn parse_pullman_orts_3d_cab_head_pos() {
        let eng = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/RF_WP_DMBSA.eng",
        );
        if !eng.is_file() {
            return;
        }
        let config = parse_orts_3d_cab_from_eng(&eng).expect("ORTS3DCab");
        assert!((config.head_pos_msts.x + 0.8).abs() < 1e-3);
        assert!((config.head_pos_msts.y - 2.875).abs() < 1e-3);
        assert!((config.head_pos_msts.z - 8.60).abs() < 1e-2);
        let vehicle_t = Transform {
            rotation: msts_shape_to_train_rotation(),
            ..default()
        };
        let head = config.head_pos_in_train(vehicle_t);
        assert!((head.x - 8.60).abs() < 1e-2);
        assert!((head.y - 2.875).abs() < 1e-3);
    }

    #[test]
    fn pullman_cab_geometry_diagnostic() {
        let cab = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        let ext = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s");
        if !cab.is_file() || !ext.is_file() {
            return;
        }
        let loaded = crate::shapes::load_shape_from_path(&cab, Some(2.0)).unwrap();
        let el = crate::shapes::load_shape_from_path(&ext, Some(50.0)).unwrap();
        let vehicle_t = crate::shapes::vehicle_shape_local_transform(&el.mesh, 0.0, 20.879);
        let head_msts = Vec3::new(-0.8, 2.875, 8.60);
        let head = vehicle_t.transform_point(head_msts);
        eprintln!("vehicle head train-local: {head:?}");
        assert!(
            loaded.parts.iter().all(|p| p.texture_file.is_some()),
            "cab textures"
        );
        let mut min_all = Vec3::splat(f32::INFINITY);
        let mut max_all = Vec3::splat(f32::NEG_INFINITY);
        for part in &loaded.parts {
            if let Some((mn, mx)) = crate::shapes::mesh_aabb(&part.mesh) {
                min_all = min_all.min(mn);
                max_all = max_all.max(mx);
            }
        }
        let head_shape = head_msts;
        assert!(
            head_shape.x >= min_all.x && head_shape.x <= max_all.x,
            "head x in cab"
        );
        assert!(
            head_shape.y >= min_all.y && head_shape.y <= max_all.y,
            "head y in cab"
        );
        assert!(
            head_shape.z >= min_all.z && head_shape.z <= max_all.z,
            "head z in cab"
        );
        let _ = head;
    }

    #[test]
    fn pullman_cab_render_asset_textures_load_from_or_content() {
        let cab = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !cab.is_file() {
            return;
        }
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let tex_dirs: Vec<PathBuf> = crate::shapes::texture_search_dirs_for_shape(&cab, &route);
        let tex_refs: Vec<&std::path::Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
        let mut meshes = bevy::prelude::Assets::<bevy::prelude::Mesh>::default();
        let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
        let mut materials = bevy::prelude::Assets::<bevy::prelude::StandardMaterial>::default();
        let mut texture_cache = std::collections::HashMap::new();
        let asset = crate::shapes::load_shape_render_asset_from_path(
            &cab,
            &tex_refs,
            Some(2.0),
            &mut meshes,
            &mut images,
            &mut materials,
            &mut texture_cache,
            bevy::prelude::Color::srgb(0.35, 0.38, 0.42),
        )
        .expect("cab render asset");
        let textured = asset.parts.iter().filter(|p| p.has_texture).count();
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
    }

    #[test]
    fn cab_transform_on_camera_places_head_at_origin() {
        let head = Vec3::new(-0.8, 2.875, 8.60);
        let vehicle = crate::shapes::vehicle_shape_local_transform(
            &{
                let ext = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s");
                if !ext.is_file() {
                    return;
                }
                crate::shapes::load_shape_from_path(&ext, Some(50.0))
                    .unwrap()
                    .mesh
            },
            0.0,
            20.879,
        );
        let local = cab_transform_on_camera(head, vehicle);
        let eye = local.transform_point(head);
        assert!(eye.length() < 1e-3, "eye={eye:?}");
        // Windshield direction (+Z in shape) must point along camera -Z
        let forward = local.rotation * Vec3::Z;
        assert!(
            (forward.z + 1.0).abs() < 1e-3 && forward.x.abs() < 1e-3,
            "forward={forward:?}"
        );
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
            }],
        );
        let mut state = CabInteriorState::default();
        log_cab_missing_once(&mut state, &consist, &route);
        assert_eq!(state.lookup, CabInteriorLookup::Missing);
        log_cab_missing_once(&mut state, &consist, &route);
        assert_eq!(state.lookup, CabInteriorLookup::Missing);
    }
}

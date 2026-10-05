//! Cab render diagnostics (CAB-A) and `RenderLayers` isolation (CAB-B).
//!
//! - **Layer 0** — mundo (terreno, vía, escena; default Bevy).
//! - **Layer 1** — exterior del consist (`LiveTrainBody`).
//! - **Layer 2** — interior cabina (`CabInteriorMarker`).
//!
//! En modo conductor la cámara renderiza capas **0 + 2** (mundo + cabina), nunca la **1**.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::cab_diag::cab_occluder_debug_enabled;
use crate::cab_view::{CabInteriorMarker, CabLeadVehicle, CabPartInfo};
use crate::camera::{
    CameraFollowMode, DRIVER_FOV_DEG, DRIVER_NEAR_CLIP_M, LiveDriverCab, driver_eye_from_lead,
};
use crate::live::LiveTrainBody;
use crate::shapes::mesh_aabb;
use crate::viewer_log;

/// Mundo / escena (default Bevy).
pub const LAYER_WORLD: usize = 0;
/// Casco y partes exteriores del tren en live.
pub const LAYER_TRAIN_EXTERIOR: usize = 1;
/// Mesh interior `CABVIEW3D`.
pub const LAYER_CAB_INTERIOR: usize = 2;

pub fn train_exterior_layers() -> RenderLayers {
    RenderLayers::layer(LAYER_TRAIN_EXTERIOR)
}

pub fn cab_interior_layers() -> RenderLayers {
    // Layer 0 ensures Bevy's main opaque pass always draws cab geometry; layer 2 keeps
    // isolation from train exterior (layer 1) in driver view.
    RenderLayers::from_layers(&[LAYER_WORLD, LAYER_CAB_INTERIOR])
}

/// Chase / órbita / replay: mundo + exterior.
pub fn camera_layers_outdoor() -> RenderLayers {
    RenderLayers::from_layers(&[LAYER_WORLD, LAYER_TRAIN_EXTERIOR])
}

/// Vista conductor: mundo (ventanas) + cabina; sin capa exterior.
pub fn camera_layers_driver() -> RenderLayers {
    RenderLayers::from_layers(&[LAYER_WORLD, LAYER_CAB_INTERIOR])
}

/// One-line cab render summary for the HUD (driver view).
#[derive(Resource, Default, Clone, Debug)]
pub struct CabRenderDiagnostic {
    pub hud_line: Option<String>,
    pub report: Option<CabViewReport>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct CabViewReport {
    pub camera_eye: [f32; 3],
    pub expected_eye: Option<[f32; 3]>,
    pub eye_error_m: Option<f32>,
    pub camera_inside_geometry: Option<bool>,
    pub mesh_parts: usize,
    pub visible_mesh_parts: usize,
}

#[derive(Resource, Default, Debug)]
pub struct CabRenderDiagLatch {
    pub was_driver: bool,
    /// True while the last logged mode was outdoor (Chase / Orbit / Off).
    pub was_outdoor: bool,
    pub last_eye: Option<Vec3>,
    last_sample_at: Option<std::time::Instant>,
}

impl CabRenderDiagLatch {
    fn should_sample(&mut self, now: std::time::Instant, entering: bool) -> bool {
        if !entering
            && self.last_sample_at.is_some_and(|last| {
                now.duration_since(last) < std::time::Duration::from_millis(500)
            })
        {
            return false;
        }
        self.last_sample_at = Some(now);
        true
    }
}

/// Tag new train exterior meshes (live consist bodies).
#[allow(clippy::type_complexity)]
pub fn tag_train_exterior_render_layers(
    mut commands: Commands,
    untagged: Query<
        Entity,
        (
            With<LiveTrainBody>,
            Without<CabInteriorMarker>,
            Without<RenderLayers>,
        ),
    >,
) {
    for entity in &untagged {
        commands.entity(entity).insert(train_exterior_layers());
    }
}

/// Tag cab interior parts when spawned.
pub fn tag_cab_interior_render_layers(
    mut commands: Commands,
    untagged: Query<Entity, (With<CabInteriorMarker>, Without<RenderLayers>)>,
) {
    for entity in &untagged {
        commands.entity(entity).insert(cab_interior_layers());
    }
}

/// Keep camera layer mask in sync with follow mode (CAB-B).
pub fn sync_camera_render_layers(
    follow: Res<CameraFollowMode>,
    mut cameras: Query<&mut RenderLayers, With<Camera3d>>,
) {
    // Cab2d needs world through ACE window alpha; hide train exterior via visibility.
    let target = if *follow == CameraFollowMode::DriverCam || follow.is_cab2d() {
        camera_layers_driver()
    } else {
        camera_layers_outdoor()
    };
    for mut layers in &mut cameras {
        if *layers != target {
            *layers = target.clone();
        }
    }
}

fn count_exterior_visibility(
    exterior: &Query<
        (&Visibility, Option<&RenderLayers>),
        (With<LiveTrainBody>, Without<CabInteriorMarker>),
    >,
) -> (usize, usize, usize) {
    let mut exterior_total = 0usize;
    let mut exterior_visible = 0usize;
    let mut exterior_layer1 = 0usize;
    for (vis, layers) in exterior {
        exterior_total += 1;
        if *vis != Visibility::Hidden {
            exterior_visible += 1;
        }
        if layers.is_some_and(|l| l.iter().any(|n| n == LAYER_TRAIN_EXTERIOR)) {
            exterior_layer1 += 1;
        }
    }
    (exterior_total, exterior_visible, exterior_layer1)
}

/// Log + HUD line for driver view (CAB-A) and chase/outdoor exterior visibility (#168).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_cab_render_diagnostic(
    follow: Res<CameraFollowMode>,
    mut state: ResMut<CabRenderDiagLatch>,
    mut diag: ResMut<CabRenderDiagnostic>,
    driver_cab: Option<Res<LiveDriverCab>>,
    lead_car: Query<&GlobalTransform, With<CabLeadVehicle>>,
    camera_q: Query<(&GlobalTransform, &Projection), With<Camera3d>>,
    cab_parts: Query<
        (&GlobalTransform, &Mesh3d, Option<&CabPartInfo>),
        (With<CabInteriorMarker>, With<Mesh3d>),
    >,
    cab_visibility: Query<&ViewVisibility, (With<CabInteriorMarker>, With<Mesh3d>)>,
    meshes: Res<Assets<Mesh>>,
    exterior: Query<
        (&Visibility, Option<&RenderLayers>),
        (With<LiveTrainBody>, Without<CabInteriorMarker>),
    >,
) {
    let driver = *follow == CameraFollowMode::DriverCam;
    let outdoor = !driver && !follow.is_cab2d();

    if follow.is_cab2d() {
        state.was_driver = false;
        state.was_outdoor = false;
        diag.hud_line = None;
        diag.report = None;
        return;
    }

    let Ok((cam_global, projection)) = camera_q.single() else {
        return;
    };
    let cam_eye = cam_global.translation();
    let cam_forward = cam_global.forward().as_vec3().normalize_or_zero();

    // Chase / Orbit / Off: single log when entering outdoor from cab (#168).
    // Do not re-log on free-fly eye motion — that floods the console in Off mode.
    if outdoor {
        diag.hud_line = None;
        diag.report = None;
        if state.was_outdoor {
            return;
        }
        state.was_outdoor = true;
        state.was_driver = false;
        let (exterior_total, exterior_visible, exterior_layer1) =
            count_exterior_visibility(&exterior);
        let mode = follow.hud_label();
        viewer_log!(
            "openrailsrs-viewer3d: {mode} render diag — eye=({:.2},{:.2},{:.2}) | ext vis {exterior_visible}/{exterior_total} on_L1={exterior_layer1} | camera layers [0,1]",
            cam_eye.x,
            cam_eye.y,
            cam_eye.z,
        );
        return;
    }

    // DriverCam (CAB-A)
    if cab_parts.is_empty() {
        diag.report = None;
        return;
    }

    let cab_res = driver_cab.as_deref();
    let expected_eye = lead_car
        .iter()
        .next()
        .and_then(|lead| cab_res.and_then(|cab| driver_eye_from_lead(lead, cab)));
    let eye = cam_eye;

    let entering = !state.was_driver;
    if !state.should_sample(std::time::Instant::now(), entering) {
        return;
    }
    state.was_driver = true;
    state.was_outdoor = false;
    state.last_eye = Some(eye);

    let (exterior_total, exterior_visible, exterior_layer1) = count_exterior_visibility(&exterior);

    let mesh_local_aabb = cab_interior_mesh_local_aabb(&cab_parts, &meshes);
    let head_inside_local = cab_res
        .and_then(|c| c.head_lead_local)
        .zip(mesh_local_aabb)
        .map(|(head, (min, max))| point_in_aabb(head, min, max));

    let cab_world_aabb = cab_interior_world_aabb(&cab_parts, &meshes);
    // The authored head can be inside the local mesh even when the actual
    // camera or the propagated cab hierarchy has moved elsewhere.
    let head_inside = cab_world_aabb.map(|(min, max)| point_in_aabb(eye, min, max));

    let cab_part_count = cab_parts.iter().count();
    let visible_mesh_parts = cab_visibility.iter().filter(|v| v.get()).count();
    let eye_error_m = expected_eye.map(|head| head.distance(eye));
    diag.report = Some(CabViewReport {
        camera_eye: eye.to_array(),
        expected_eye: expected_eye.map(|eye| eye.to_array()),
        eye_error_m,
        camera_inside_geometry: head_inside,
        mesh_parts: cab_part_count,
        visible_mesh_parts,
    });
    let aabb_line = mesh_local_aabb
        .map(|(min, max)| {
            format!(
                "cab_mesh Y[{:.1},{:.1}] Z[{:.1},{:.1}]",
                min.y, max.y, min.z, max.z
            )
        })
        .unwrap_or_else(|| "cab_mesh (none)".into());

    let inside_label = head_inside
        .map(|b| if b { "inside" } else { "OUTSIDE" })
        .unwrap_or("n/a");
    let inside_detail = head_inside_local
        .map(|b| if b { "lead-local" } else { "lead-local OUT" })
        .unwrap_or("");

    let (near_m, fov_deg) = match projection {
        Projection::Perspective(p) => (p.near, p.fov.to_degrees()),
        _ => (DRIVER_NEAR_CLIP_M, DRIVER_FOV_DEG),
    };

    let orts_hint = driver_cab
        .as_ref()
        .and_then(|c| c.head_msts)
        .map(|h| format!(" ORTS=({:.1},{:.1},{:.1})", h.x, h.y, h.z))
        .unwrap_or_default();

    let occluder = cab_occluder_debug_enabled()
        .then(|| closest_cab_occluder_along_ray(eye, cam_forward, near_m, &cab_parts, &meshes))
        .flatten();
    let occluder_hud = occluder
        .as_ref()
        .map(|o| {
            format!(
                " | hit prim={} sort={} tex={} t={:.3}m{}",
                o.prim_state_idx,
                o.sort_index,
                o.texture,
                o.distance_m,
                if o.eye_inside { " EYE_INSIDE" } else { "" }
            )
        })
        .unwrap_or_default();

    let hud_line = format!(
        "cab: eye({:.1},{:.1},{:.1}){orts_hint} {inside_label} {inside_detail} | near={near_m:.2}m fov={fov_deg:.0}° | ext vis {exterior_visible}/{exterior_total} L1={exterior_layer1} | parts={cab_part_count} cam=L0+L2{occluder_hud}",
        eye.x, eye.y, eye.z,
    );
    diag.hud_line = Some(hud_line.clone());

    viewer_log!(
        "openrailsrs-viewer3d: cab render diag — eye=({:.2},{:.2},{:.2}) {aabb_line} {inside_label} | near={near_m:.3} fov={fov_deg:.1}° | exterior visible={exterior_visible}/{exterior_total} on_L1={exterior_layer1} | cab_parts={visible_mesh_parts}/{cab_part_count} visible | eye_error={eye_error_m:?} | camera layers [0,2]",
        eye.x,
        eye.y,
        eye.z,
    );
    if cab_occluder_debug_enabled() {
        if let Some(o) = occluder {
            viewer_log!(
                "openrailsrs-viewer3d: cab occluder (#167) — first hit along look: prim_state={} sort_index={} tex={} shader={} matrix={:?} dist={:.3}m eye_inside_aabb={} aabb=({:.2},{:.2},{:.2})..({:.2},{:.2},{:.2})",
                o.prim_state_idx,
                o.sort_index,
                o.texture,
                o.shader,
                o.cab_matrix_idx,
                o.distance_m,
                o.eye_inside,
                o.aabb_min.x,
                o.aabb_min.y,
                o.aabb_min.z,
                o.aabb_max.x,
                o.aabb_max.y,
                o.aabb_max.z,
            );
        } else {
            viewer_log!(
                "openrailsrs-viewer3d: cab occluder (#167) — no cab AABB hit along look (near={near_m:.3}m)"
            );
        }
    }
}

#[allow(clippy::type_complexity)]
fn cab_interior_mesh_local_aabb(
    parts: &Query<
        (&GlobalTransform, &Mesh3d, Option<&CabPartInfo>),
        (With<CabInteriorMarker>, With<Mesh3d>),
    >,
    meshes: &Assets<Mesh>,
) -> Option<(Vec3, Vec3)> {
    let mut min_all = Vec3::splat(f32::INFINITY);
    let mut max_all = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for (_global, mesh3d, _) in parts {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };
        let Some((mn, mx)) = mesh_aabb(mesh) else {
            continue;
        };
        min_all = min_all.min(mn);
        max_all = max_all.max(mx);
        any = true;
    }
    any.then_some((min_all, max_all))
}

#[allow(clippy::type_complexity)]
fn cab_interior_world_aabb(
    parts: &Query<
        (&GlobalTransform, &Mesh3d, Option<&CabPartInfo>),
        (With<CabInteriorMarker>, With<Mesh3d>),
    >,
    meshes: &Assets<Mesh>,
) -> Option<(Vec3, Vec3)> {
    let mut min_all = Vec3::splat(f32::INFINITY);
    let mut max_all = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for (global, mesh3d, _) in parts {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };
        let Some((mn, mx)) = mesh_aabb(mesh) else {
            continue;
        };
        for corner in aabb_corners(mn, mx) {
            let w = global.transform_point(corner);
            min_all = min_all.min(w);
            max_all = max_all.max(w);
            any = true;
        }
    }
    any.then_some((min_all, max_all))
}

struct CabOccluderHit {
    prim_state_idx: i32,
    sort_index: u32,
    texture: String,
    shader: String,
    cab_matrix_idx: Option<usize>,
    distance_m: f32,
    eye_inside: bool,
    aabb_min: Vec3,
    aabb_max: Vec3,
}

/// First cab part whose world AABB is hit by a ray from the eye along look (#167).
#[allow(clippy::type_complexity)]
fn closest_cab_occluder_along_ray(
    eye: Vec3,
    forward: Vec3,
    near_m: f32,
    parts: &Query<
        (&GlobalTransform, &Mesh3d, Option<&CabPartInfo>),
        (With<CabInteriorMarker>, With<Mesh3d>),
    >,
    meshes: &Assets<Mesh>,
) -> Option<CabOccluderHit> {
    if forward.length_squared() < 1e-8 {
        return None;
    }
    let dir = forward.normalize();
    let mut best: Option<CabOccluderHit> = None;
    for (global, mesh3d, info) in parts {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };
        let Some((mn, mx)) = mesh_aabb(mesh) else {
            continue;
        };
        let mut wmin = Vec3::splat(f32::INFINITY);
        let mut wmax = Vec3::splat(f32::NEG_INFINITY);
        for corner in aabb_corners(mn, mx) {
            let w = global.transform_point(corner);
            wmin = wmin.min(w);
            wmax = wmax.max(w);
        }
        let eye_inside = point_in_aabb(eye, wmin, wmax);
        let Some(t) = ray_aabb_intersect(eye, dir, wmin, wmax) else {
            if !eye_inside {
                continue;
            }
            // Eye inside: treat as distance 0 candidate (near-plane plate risk).
            let hit = CabOccluderHit {
                prim_state_idx: info.map(|i| i.prim_state_idx).unwrap_or(-1),
                sort_index: info.map(|i| i.sort_index).unwrap_or(0),
                texture: info
                    .and_then(|i| i.texture_name.clone())
                    .unwrap_or_else(|| "?".into()),
                shader: info
                    .and_then(|i| i.shader_name.clone())
                    .unwrap_or_else(|| "?".into()),
                cab_matrix_idx: info.and_then(|i| i.cab_matrix_idx),
                distance_m: 0.0,
                eye_inside: true,
                aabb_min: wmin,
                aabb_max: wmax,
            };
            if best.as_ref().is_none_or(|b| hit.distance_m < b.distance_m) {
                best = Some(hit);
            }
            continue;
        };
        if t < near_m * 0.5 {
            // Behind / at near: still report if closer than current best.
        }
        if t < 0.0 {
            continue;
        }
        let hit = CabOccluderHit {
            prim_state_idx: info.map(|i| i.prim_state_idx).unwrap_or(-1),
            sort_index: info.map(|i| i.sort_index).unwrap_or(0),
            texture: info
                .and_then(|i| i.texture_name.clone())
                .unwrap_or_else(|| "?".into()),
            shader: info
                .and_then(|i| i.shader_name.clone())
                .unwrap_or_else(|| "?".into()),
            cab_matrix_idx: info.and_then(|i| i.cab_matrix_idx),
            distance_m: t,
            eye_inside,
            aabb_min: wmin,
            aabb_max: wmax,
        };
        if best.as_ref().is_none_or(|b| hit.distance_m < b.distance_m) {
            best = Some(hit);
        }
    }
    best
}

/// Ray–AABB slab test; returns entry `t` along unit `dir` when the ray hits.
fn ray_aabb_intersect(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    for i in 0..3 {
        let o = origin[i];
        let d = dir[i];
        let (mn, mx) = (min[i], max[i]);
        if d.abs() < 1e-8 {
            if o < mn || o > mx {
                return None;
            }
            continue;
        }
        let mut t1 = (mn - o) / d;
        let mut t2 = (mx - o) / d;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmin > tmax {
            return None;
        }
    }
    if tmax < 0.0 {
        return None;
    }
    Some(if tmin >= 0.0 { tmin } else { 0.0 })
}

fn aabb_corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}

fn point_in_aabb(point: Vec3, min: Vec3, max: Vec3) -> bool {
    point.x >= min.x
        && point.x <= max.x
        && point.y >= min.y
        && point.y <= max.y
        && point.z >= min.z
        && point.z <= max.z
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn diagnostic_checks_actual_camera_even_when_authored_head_is_inside() {
        let mut app = App::new();
        app.insert_resource(CameraFollowMode::DriverCam)
            .insert_resource(LiveDriverCab {
                head_lead_local: Some(Vec3::ZERO),
                ..Default::default()
            })
            .init_resource::<CabRenderDiagLatch>()
            .init_resource::<CabRenderDiagnostic>()
            .init_resource::<Assets<Mesh>>();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(2.0, 2.0, 2.0));
        app.world_mut()
            .spawn((CabLeadVehicle, GlobalTransform::IDENTITY));
        app.world_mut().spawn((
            Camera3d::default(),
            Projection::default(),
            GlobalTransform::from_translation(Vec3::Y * 9.0),
        ));
        app.world_mut()
            .spawn((CabInteriorMarker, Mesh3d(mesh), GlobalTransform::IDENTITY));
        app.world_mut()
            .run_system_once(update_cab_render_diagnostic)
            .unwrap();
        let diag = app.world().resource::<CabRenderDiagnostic>();
        let report = diag.report.as_ref().unwrap();
        assert_eq!(report.camera_inside_geometry, Some(false));
        assert_eq!(report.eye_error_m, Some(9.0));
        assert!(diag.hud_line.as_ref().unwrap().contains("OUTSIDE"));
    }

    #[test]
    fn moving_cab_diagnostic_is_bounded_and_camera_transition_is_immediate() {
        let mut latch = CabRenderDiagLatch::default();
        let start = std::time::Instant::now();
        assert!(latch.should_sample(start, true));
        for frame in 1..30 {
            assert!(
                !latch.should_sample(start + std::time::Duration::from_millis(frame * 16), false)
            );
        }
        assert!(latch.should_sample(start + std::time::Duration::from_millis(500), false));
        assert!(latch.should_sample(start + std::time::Duration::from_millis(501), true));
    }

    #[test]
    fn layer_masks_exclude_exterior_in_driver_view() {
        let outdoor = camera_layers_outdoor();
        let driver = camera_layers_driver();
        let exterior = train_exterior_layers();
        let cab = cab_interior_layers();

        assert!(outdoor.intersects(&exterior));
        assert!(!driver.intersects(&exterior));
        assert!(driver.intersects(&cab));
        assert!(cab.intersects(&RenderLayers::layer(LAYER_WORLD)));
        assert!(!outdoor.intersects(&RenderLayers::layer(LAYER_CAB_INTERIOR)));
    }

    #[test]
    fn ray_aabb_reports_entry_distance_along_look() {
        let min = Vec3::new(-1.0, -1.0, 2.0);
        let max = Vec3::new(1.0, 1.0, 3.0);
        let t = ray_aabb_intersect(Vec3::ZERO, Vec3::Z, min, max).expect("hit");
        assert!((t - 2.0).abs() < 1e-4, "t={t}");
        assert!(ray_aabb_intersect(Vec3::ZERO, -Vec3::Z, min, max).is_none());
    }
}

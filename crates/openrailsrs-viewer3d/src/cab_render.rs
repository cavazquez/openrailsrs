//! Cab render diagnostics (CAB-A) and `RenderLayers` isolation (CAB-B).
//!
//! - **Layer 0** — mundo (terreno, vía, escena; default Bevy).
//! - **Layer 1** — exterior del consist (`LiveTrainBody`).
//! - **Layer 2** — interior cabina (`CabInteriorMarker`).
//!
//! En modo conductor la cámara renderiza capas **0 + 2** (mundo + cabina), nunca la **1**.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::cab_view::CabInteriorMarker;
use crate::cab_view::CabLeadVehicle;
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
}

#[derive(Resource, Default, Debug)]
pub struct CabRenderDiagLatch {
    pub was_driver: bool,
    pub last_eye: Option<Vec3>,
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

/// Log + HUD line once each time driver view is entered (CAB-A).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_cab_render_diagnostic(
    follow: Res<CameraFollowMode>,
    mut state: ResMut<CabRenderDiagLatch>,
    mut diag: ResMut<CabRenderDiagnostic>,
    driver_cab: Option<Res<LiveDriverCab>>,
    lead_car: Query<&GlobalTransform, With<CabLeadVehicle>>,
    camera_q: Query<(&GlobalTransform, &Projection), With<Camera3d>>,
    cab_parts: Query<(&GlobalTransform, &Mesh3d), (With<CabInteriorMarker>, With<Mesh3d>)>,
    meshes: Res<Assets<Mesh>>,
    exterior: Query<
        (&Visibility, Option<&RenderLayers>),
        (With<LiveTrainBody>, Without<CabInteriorMarker>),
    >,
) {
    let driver = *follow == CameraFollowMode::DriverCam;
    if !driver {
        state.was_driver = false;
        diag.hud_line = None;
        return;
    }

    if cab_parts.is_empty() {
        return;
    }

    let Ok((cam_global, projection)) = camera_q.single() else {
        return;
    };
    let cab_res = driver_cab.as_deref();
    let eye = lead_car
        .iter()
        .next()
        .and_then(|lead| cab_res.and_then(|cab| driver_eye_from_lead(lead, cab)))
        .unwrap_or_else(|| cam_global.translation());

    if !state.was_driver {
        state.was_driver = true;
    } else if state.last_eye.is_some_and(|last| last.distance(eye) < 0.05) {
        return;
    }
    state.last_eye = Some(eye);

    let mesh_local_aabb = cab_interior_mesh_local_aabb(&cab_parts, &meshes);
    let head_inside_local = cab_res
        .and_then(|c| c.head_lead_local)
        .zip(mesh_local_aabb)
        .map(|(head, (min, max))| point_in_aabb(head, min, max));

    let cab_world_aabb = cab_interior_world_aabb(&cab_parts, &meshes);
    let head_inside_world = cab_world_aabb.map(|(min, max)| point_in_aabb(eye, min, max));
    let head_inside = head_inside_local.or(head_inside_world);

    let mut exterior_total = 0usize;
    let mut exterior_visible = 0usize;
    let mut exterior_layer1 = 0usize;
    for (vis, layers) in &exterior {
        exterior_total += 1;
        if *vis == Visibility::Visible {
            exterior_visible += 1;
        }
        if layers.is_some_and(|l| l.iter().any(|n| n == LAYER_TRAIN_EXTERIOR)) {
            exterior_layer1 += 1;
        }
    }

    let cab_part_count = cab_parts.iter().count();
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

    let hud_line = format!(
        "cab: eye({:.1},{:.1},{:.1}){orts_hint} {inside_label} {inside_detail} | near={near_m:.2}m fov={fov_deg:.0}° | ext vis {exterior_visible}/{exterior_total} L1={exterior_layer1} | parts={cab_part_count} cam=L0+L2",
        eye.x, eye.y, eye.z,
    );
    diag.hud_line = Some(hud_line.clone());

    viewer_log!(
        "openrailsrs-viewer3d: cab render diag — eye=({:.2},{:.2},{:.2}) {aabb_line} {inside_label} | near={near_m:.3} fov={fov_deg:.1}° | exterior visible={exterior_visible}/{exterior_total} on_L1={exterior_layer1} | cab_parts={cab_part_count} | camera layers [0,2]",
        eye.x,
        eye.y,
        eye.z,
    );
}

#[allow(clippy::type_complexity)]
fn cab_interior_mesh_local_aabb(
    parts: &Query<(&GlobalTransform, &Mesh3d), (With<CabInteriorMarker>, With<Mesh3d>)>,
    meshes: &Assets<Mesh>,
) -> Option<(Vec3, Vec3)> {
    let mut min_all = Vec3::splat(f32::INFINITY);
    let mut max_all = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for (_global, mesh3d) in parts {
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
    parts: &Query<(&GlobalTransform, &Mesh3d), (With<CabInteriorMarker>, With<Mesh3d>)>,
    meshes: &Assets<Mesh>,
) -> Option<(Vec3, Vec3)> {
    let mut min_all = Vec3::splat(f32::INFINITY);
    let mut max_all = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for (global, mesh3d) in parts {
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
}

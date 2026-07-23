//! Live-train diagnostics: consist inventory and per-vehicle transform audit.

use std::path::Path;

use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    debug_consist_enabled, debug_vehicle_transforms_enabled, msts_shape_to_train_rotation,
};
use openrailsrs_formats::{
    ConsistEntry, ConsistFile, parse_from_first_paren, read_msts_file_to_string,
};
use openrailsrs_train::{
    Vehicle, consist_asset_root, load_consist_with_asset_root, load_engine_from_path,
};

use crate::rolling_stock::ConsistVehicleVisual;
use crate::viewer_log;

fn mat3_det_from_transform(t: &Transform) -> f32 {
    t.to_matrix().determinant()
}

fn log_transform_audit(
    vi: usize,
    vehicle_name: &str,
    shape_file: Option<&str>,
    local: &Transform,
    train_world: &Transform,
) {
    let local_det = mat3_det_from_transform(local);
    let world = train_world.mul_transform(*local);
    let world_det = mat3_det_from_transform(&world);

    let msts_forward = Vec3::Z;
    let msts_right = Vec3::X;
    let train_rot = msts_shape_to_train_rotation();
    let local_fwd = (local.rotation * train_rot * msts_forward).normalize_or_zero();
    let local_right = (local.rotation * train_rot * msts_right).normalize_or_zero();
    let world_fwd = (train_world.rotation * local_fwd).normalize_or_zero();
    let world_right = (train_world.rotation * local_right).normalize_or_zero();

    let mirrored = local_det < 0.0
        || world_det < 0.0
        || local.scale.x < 0.0
        || local.scale.y < 0.0
        || local.scale.z < 0.0;

    viewer_log!(
        "openrailsrs-viewer3d: [transform] car={vi} name={vehicle_name} shape={:?} \
         local_t=({:.3},{:.3},{:.3}) local_r=({:.1},{:.1},{:.1}) scale=({:.4},{:.4},{:.4}) \
         det_local={local_det:.6} det_world={world_det:.6} mirrored={mirrored} \
         fwd_local=({:.3},{:.3},{:.3}) right_local=({:.3},{:.3},{:.3}) \
         fwd_world=({:.3},{:.3},{:.3}) right_world=({:.3},{:.3},{:.3}) \
         msts_to_train=+90°Y (shape +Z → train +X)",
        shape_file,
        local.translation.x,
        local.translation.y,
        local.translation.z,
        local.rotation.to_euler(EulerRot::YXZ).0.to_degrees(),
        local.rotation.to_euler(EulerRot::YXZ).1.to_degrees(),
        local.rotation.to_euler(EulerRot::YXZ).2.to_degrees(),
        local.scale.x,
        local.scale.y,
        local.scale.z,
        local_fwd.x,
        local_fwd.y,
        local_fwd.z,
        local_right.x,
        local_right.y,
        local_right.z,
        world_fwd.x,
        world_fwd.y,
        world_fwd.z,
        world_right.x,
        world_right.y,
        world_right.z,
    );
}

/// Log `.con` vehicle inventory when `OPENRAILSRS_DEBUG_CONSIST=1`.
pub fn log_consist_diagnostic(
    scenario_dir: &Path,
    consist_rel: &str,
    visuals: &[ConsistVehicleVisual],
    train_pos: Vec3,
    train_yaw_deg: f32,
) {
    if !debug_consist_enabled() {
        return;
    }

    let con_path = scenario_dir.join(consist_rel);
    let asset_root = consist_asset_root(&con_path);
    viewer_log!(
        "openrailsrs-viewer3d: [consist] file={} asset_root={} train_pos=({:.1},{:.1},{:.1}) yaw={:.1}°",
        con_path.display(),
        asset_root.display(),
        train_pos.x,
        train_pos.y,
        train_pos.z,
        train_yaw_deg,
    );

    let Ok(consist) = load_consist_with_asset_root(&con_path, asset_root) else {
        viewer_log!(
            "openrailsrs-viewer3d: [consist] ERROR: could not parse {}",
            con_path.display()
        );
        return;
    };

    let raw_entries = read_consist_entries(&con_path);

    let mut engine_count = 0usize;
    let mut wagon_count = 0usize;
    let mut powered_engines = 0usize;

    for (i, vehicle) in consist.vehicles.iter().enumerate() {
        let visual = visuals.get(i);
        let entry_path = raw_entries.get(i).map(|(_, p)| p.as_str());

        match vehicle {
            Vehicle::Loco(l) => {
                engine_count += 1;
                let has_power = l.max_power_w > 1.0 || l.diesel_traction.is_some();
                if has_power {
                    powered_engines += 1;
                }
                let (eng_path, has_cab, cab_note) = entry_path
                    .map(|p| engine_cab_summary(asset_root, p))
                    .unwrap_or((None, false, String::new()));

                viewer_log!(
                    "openrailsrs-viewer3d: [consist] idx={i} type=Engine name={} \
                     eng={:?} shape={:?} folder={:?} offset_m={:.2} length_m={:.2} \
                     max_power_w={:.0} has_diesel_traction={} has_cab={} {cab_note} \
                     role={}",
                    l.name,
                    eng_path,
                    l.wagon_shape,
                    entry_path.and_then(trainset_folder),
                    visual.map(|v| v.offset_m).unwrap_or(0.0),
                    l.length_m,
                    l.max_power_w,
                    l.diesel_traction.is_some(),
                    has_cab,
                    if i == 0 {
                        "lead_driving_motor_car"
                    } else if i == consist.vehicles.len() - 1 {
                        "trail_driving_motor_car_or_dvt"
                    } else {
                        "powered_engine_mid_consist"
                    },
                );
            }
            Vehicle::Wagon(w) => {
                wagon_count += 1;
                viewer_log!(
                    "openrailsrs-viewer3d: [consist] idx={i} type=Wagon name={} \
                     wag={:?} shape={:?} folder={:?} offset_m={:.2} length_m={:.2}",
                    w.name,
                    entry_path,
                    w.wagon_shape,
                    entry_path.and_then(trainset_folder),
                    visual.map(|v| v.offset_m).unwrap_or(0.0),
                    w.length_m,
                );
            }
        }
    }

    viewer_log!(
        "openrailsrs-viewer3d: [consist] summary: {} vehicles ({} Engine, {} Wagon), {} powered engine(s)",
        consist.vehicles.len(),
        engine_count,
        wagon_count,
        powered_engines,
    );

    if engine_count >= 2 && wagon_count > 0 && powered_engines >= 1 {
        viewer_log!(
            "openrailsrs-viewer3d: [consist] NOTE: Blue Pullman style EMU — no separate classic locomotive; \
             driving motor cars at consist ends (DMBSA/DMBSH .eng entries)."
        );
    } else if engine_count == 1 {
        viewer_log!(
            "openrailsrs-viewer3d: [consist] NOTE: single Engine entry — lead unit is the locomotive/power car."
        );
    }
}

fn read_consist_entries(con_path: &Path) -> Vec<(ConsistEntry, String)> {
    let Ok(text) = read_msts_file_to_string(con_path) else {
        return Vec::new();
    };
    let Ok(ast) = parse_from_first_paren(&text) else {
        return Vec::new();
    };
    let Ok(file) = ConsistFile::from_ast(&ast) else {
        return Vec::new();
    };
    file.entries
        .into_iter()
        .map(|e| {
            let path = match &e {
                ConsistEntry::Engine { path, .. } | ConsistEntry::Wagon { path, .. } => path.clone(),
            };
            (e, path)
        })
        .collect()
}

fn trainset_folder(entry_path: &str) -> Option<String> {
    let p = Path::new(entry_path);
    p.parent()
        .and_then(|d| d.file_name())
        .map(|s| s.to_string_lossy().into_owned())
}

fn engine_cab_summary(asset_root: &Path, eng_rel: &str) -> (Option<String>, bool, String) {
    let eng_path = asset_root.join(eng_rel);
    let eng_display = eng_path.display().to_string();
    let Ok(loc) = load_engine_from_path(&eng_path) else {
        return (Some(eng_display), false, String::new());
    };
    // Re-read typed cab fields from `.eng` AST for cabview / ORTS3D cab paths.
    let cab_note = if let Ok(text) = read_msts_file_to_string(&eng_path) {
        if let Ok(ast) = parse_from_first_paren(&text) {
            if let Ok(eng) = openrailsrs_formats::EngineFile::from_ast(&ast) {
                let mut parts = Vec::new();
                if let Some(cvf) = &eng.cab.cab_view_file {
                    parts.push(format!("CabView={cvf}"));
                }
                if let Some(shape) = &eng.cab.orts_3d_cab_shape {
                    parts.push(format!("ORTS3DCab={shape}"));
                }
                parts.join(" ")
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let has_cab = !cab_note.is_empty() || loc.max_power_w > 0.0;
    (Some(eng_display), has_cab, cab_note)
}

/// Per-car transform audit when `OPENRAILSRS_DEBUG_VEHICLE_TRANSFORMS=1`.
pub fn log_vehicle_transform_if_enabled(
    vi: usize,
    vehicle: &ConsistVehicleVisual,
    local: &Transform,
    train_world: &Transform,
) {
    if !debug_vehicle_transforms_enabled() {
        return;
    }
    log_transform_audit(
        vi,
        &vehicle.name,
        vehicle.shape_file.as_deref(),
        local,
        train_world,
    );
}

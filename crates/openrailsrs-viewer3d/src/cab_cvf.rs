//! CVF-driven cab control animation (Open Rails `ThreeDimentionCabViewer` subset).
//!
//! Maps shape matrix names (`THROTTLE:0:0`, `TRAIN_BRAKE:0:0`, …) to parsed `.cvf`
//! controls and applies live telemetry in driver view.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{
    animation_pose_matrices, lever_entity_transform_at_mesh_center, lever_entity_transform_rebased,
};
use openrailsrs_formats::{
    CabControl, CabViewFile, ControlType, ResolvedCabAssets, ShapeFile, parse_msts_file,
    resolve_cab_assets,
};
use openrailsrs_sim::CabTelemetry;

use crate::cab_view::{CabInteriorMarker, CabInteriorRoot};
use crate::camera::CameraFollowMode;
use crate::coordinates::matrix43_to_transform;
use crate::live::LiveDrive;
use crate::viewer_log;

/// Cached cab CVF + shape animation bindings for the active interior.
#[derive(Resource, Default, Debug)]
pub struct CabCvfState {
    pub cvf_path: Option<PathBuf>,
    pub runtime: Option<CabCvfRuntime>,
    /// Smoothed lever animation keys (matrix index → shape anim frame).
    pub lever_keys: HashMap<usize, f32>,
}

/// Parsed CVF + shape matrix bindings.
#[derive(Clone, Debug)]
pub struct CabCvfRuntime {
    pub cvf: CabViewFile,
    pub shape: ShapeFile,
    /// Matrix index → control driver (from matrix naming convention).
    pub matrix_drivers: HashMap<usize, MatrixDriver>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MatrixDriver {
    /// Shape-animated lever (`THROTTLE:0:0`, …).
    Lever {
        control: ControlType,
        /// Nth CVF control of this type (OR `TYPE:Order`).
        order: u32,
        anim_node: Option<usize>,
    },
    /// OR `AnimatedPartMultiState` — dial needles / POINTER gauges / discrete parts.
    MultiState {
        control: ControlType,
        order: u32,
        /// Matrix `param1` (often 0; POINTER sub-index for `TRAIN_BRAKE:0:0` / `:0:1`).
        param1: u32,
        sub_part: u32,
        anim_node: Option<usize>,
    },
    /// OR `ThreeDimCabGaugeNative` — solid colour quad (width×length mm).
    GaugeNative {
        control: ControlType,
        order: u32,
        width_mm: f32,
        length_mm: f32,
    },
    /// OR `ThreeDimCabDigit` — ACE font digits at the matrix pivot.
    Digit {
        control: ControlType,
        order: u32,
        /// Font height in mm (`SPEEDOMETER:1:14`).
        height_mm: f32,
        /// Optional custom font ACE stem (`CLOCK:1:15:CLOCKS` → `CLOCKS`).
        font_ace: Option<String>,
    },
}

/// Parsed `TYPE:Order[:P1[:P2]][-PartN]` matrix name (OR `ThreeDimentionCabViewer`).
#[derive(Clone, Debug, PartialEq)]
pub struct MatrixControlName {
    pub control: ControlType,
    pub order: u32,
    pub param1: String,
    pub param2: String,
    pub sub_part: u32,
}

/// Marks a cab mesh part driven by a shape matrix index.
#[derive(Component, Clone, Copy, Debug)]
pub struct CabCvfPart {
    pub matrix_idx: usize,
    /// Fixed cab-local pivot (3D handwheel far from CVF matrix).
    pub pivot_at_mesh: Option<Vec3>,
    /// Local rotation axis override for fallback animation.
    pub local_spin_axis: Option<Vec3>,
}

/// Build matrix → control bindings from MSTS/OR matrix names.
pub fn build_cab_cvf_runtime(cvf: CabViewFile, shape: ShapeFile) -> CabCvfRuntime {
    let mut matrix_drivers = HashMap::new();
    for (idx, matrix) in shape.matrices.iter().enumerate() {
        if let Some(driver) = matrix_driver_from_name(&matrix.name, &shape, idx, &cvf) {
            matrix_drivers.insert(idx, driver);
        }
    }
    CabCvfRuntime {
        cvf,
        shape,
        matrix_drivers,
    }
}

/// Parse OR matrix naming `TYPE:Order[:P1[:P2]][-PartN]`.
pub fn parse_matrix_control_name(name: &str) -> Option<MatrixControlName> {
    let head = name.split('-').next()?.trim();
    let parts: Vec<&str> = head.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let control = control_type_from_matrix_prefix(parts[0].trim())?;
    let order: u32 = parts[1].trim().parse().ok()?;
    let param1 = parts.get(2).map(|s| s.trim().to_string()).unwrap_or_default();
    let param2 = parts.get(3).map(|s| s.trim().to_string()).unwrap_or_default();
    let sub_part = name
        .split('-')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some(MatrixControlName {
        control,
        order,
        param1,
        param2,
        sub_part,
    })
}

/// Nth CVF control matching `control` (OR `ControlMap` key order).
pub fn cvf_control_at_order<'a>(
    cvf: &'a CabViewFile,
    control: &ControlType,
    order: u32,
) -> Option<&'a CabControl> {
    let mut seen = 0u32;
    for cab in &cvf.controls {
        let Some(ct) = cab.control_type() else {
            continue;
        };
        if !types_match(ct, control) {
            continue;
        }
        if seen == order {
            return Some(cab);
        }
        seen += 1;
    }
    None
}

fn matrix_driver_from_name(
    name: &str,
    shape: &ShapeFile,
    matrix_idx: usize,
    cvf: &CabViewFile,
) -> Option<MatrixDriver> {
    let parsed = parse_matrix_control_name(name)?;
    let anim_node = shape.animations.first().and_then(|anim| {
        if matrix_idx < anim.nodes.len() && !anim.nodes[matrix_idx].controllers.is_empty() {
            Some(matrix_idx)
        } else {
            anim.nodes
                .iter()
                .position(|n| n.name.eq_ignore_ascii_case(name))
        }
    });

    let style = cvf_control_at_order(cvf, &parsed.control, parsed.order);
    match style {
        Some(CabControl::Digital { .. }) => {
            let height_mm = parsed.param1.parse().unwrap_or(14.0);
            let font_ace = if parsed.param2.is_empty() {
                None
            } else {
                Some(parsed.param2.clone())
            };
            Some(MatrixDriver::Digit {
                control: parsed.control,
                order: parsed.order,
                height_mm,
                font_ace,
            })
        }
        Some(CabControl::Unknown { kind }) if kind.eq_ignore_ascii_case("GAUGE") => {
            // formats::CabControl::Gauge TBD; non-POINTER → solid native (OR default).
            Some(MatrixDriver::GaugeNative {
                control: parsed.control,
                order: parsed.order,
                width_mm: parsed.param1.parse().unwrap_or(10.0),
                length_mm: parsed.param2.parse().unwrap_or(100.0),
            })
        }
        _ if control_is_lever(&parsed.control) => Some(MatrixDriver::Lever {
            control: parsed.control,
            order: parsed.order,
            anim_node,
        }),
        _ => Some(MatrixDriver::MultiState {
            control: parsed.control,
            order: parsed.order,
            param1: parsed.param1.parse().unwrap_or(0),
            sub_part: parsed.sub_part,
            anim_node,
        }),
    }
}

fn control_type_from_matrix_prefix(prefix: &str) -> Option<ControlType> {
    let normalized = prefix.replace('_', " ").to_ascii_uppercase();
    Some(match normalized.as_str() {
        "THROTTLE" | "THROTTLE DISPLAY" | "THROTTLE LEVER" => ControlType::Throttle,
        "TRAIN BRAKE" | "TRAIN BRAKE LEVER" => ControlType::TrainBrake,
        "DYNAMIC BRAKE" | "DYNAMIC BRAKE DISPLAY" => ControlType::DynamicBrakeDisplay,
        "DIRECTION" | "DIRECTION DISPLAY" => ControlType::DirectionDisplay,
        "SPEEDOMETER" => ControlType::Speedometer,
        "MAIN RES" => ControlType::MainRes,
        "BRAKE CYL" => ControlType::BrakeCyl,
        "BRAKE PIPE" => ControlType::BrakePipe,
        "AMMETER" => ControlType::Ammeter,
        "HORN" => ControlType::Generic("HORN".into()),
        "WIPERS" | "EXTERNALWIPERS" | "EXTERNAL WIPERS" => ControlType::Generic("WIPERS".into()),
        other => ControlType::Generic(other.to_string()),
    })
}

fn control_is_lever(control: &ControlType) -> bool {
    matches!(
        control,
        ControlType::Throttle
            | ControlType::TrainBrake
            | ControlType::DynamicBrakeDisplay
            | ControlType::DirectionDisplay
    )
}

/// Matrices whose bound meshes are rebaked to bone-local space (3D levers only).
pub fn cab_rebase_matrix_indices(runtime: &CabCvfRuntime) -> HashSet<usize> {
    runtime
        .matrix_drivers
        .iter()
        .filter_map(|(idx, driver)| matches!(driver, MatrixDriver::Lever { .. }).then_some(*idx))
        .collect()
}

/// Matrix indices with dedicated 3D lever meshes (Pullman: M4, M8, M9, M10).
pub fn cab_lever_matrix_indices(runtime: &CabCvfRuntime) -> HashSet<usize> {
    cab_rebase_matrix_indices(runtime)
}
/// Normalized 0–1 value for a cab control from live telemetry.
pub fn control_value(control: &ControlType, tel: &CabTelemetry) -> f64 {
    match control {
        ControlType::Throttle | ControlType::ThrottleDisplay => tel.throttle_pct / 100.0,
        ControlType::TrainBrake => tel.brake_pct / 100.0,
        // Distinct from train brake; no dedicated telemetry yet.
        ControlType::DynamicBrakeDisplay => 0.0,
        ControlType::DirectionDisplay => tel.direction,
        ControlType::Speedometer => {
            let scale = tel.limit_kmh.max(40.0);
            (tel.speed_kmh / scale).clamp(0.0, 1.0)
        }
        ControlType::MainRes => (tel.main_res_bar / 10.0).clamp(0.0, 1.0),
        ControlType::BrakeCyl | ControlType::BrakePipe => {
            let bar = match control {
                ControlType::BrakeCyl => tel.brake_cyl_bar,
                _ => tel.brake_pipe_bar,
            };
            (bar / 5.0).clamp(0.0, 1.0)
        }
        ControlType::Generic(name) if name.eq_ignore_ascii_case("HORN") && tel.horn_active => 1.0,
        ControlType::Generic(name) if name.contains("WIPER") && tel.wiper_active => 1.0,
        ControlType::Ammeter => tel
            .diesel_rpm
            .map(|r| (r / 1500.0).clamp(0.0, 1.0))
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Absolute dial reading in CVF `ScaleRange` units (Open Rails gauge value).
pub fn dial_control_value(
    control: &ControlType,
    dial: &openrailsrs_formats::CabDialParams,
    tel: &CabTelemetry,
) -> f64 {
    let units = dial.units.as_deref().unwrap_or("");
    match control {
        ControlType::Speedometer => {
            if units.eq_ignore_ascii_case("MILES_PER_HOUR") {
                tel.speed_kmh * 0.621_371
            } else {
                tel.speed_kmh
            }
        }
        ControlType::MainRes | ControlType::BrakeCyl | ControlType::BrakePipe => {
            let bar = match control {
                ControlType::MainRes => tel.main_res_bar,
                ControlType::BrakeCyl => tel.brake_cyl_bar,
                _ => tel.brake_pipe_bar,
            };
            if units.eq_ignore_ascii_case("PSI") {
                bar * 14.503_773_8
            } else {
                bar
            }
        }
        ControlType::Ammeter => tel.diesel_rpm.unwrap_or(0.0),
        _ => {
            let n = control_value(control, tel);
            dial.scale_min + n * (dial.scale_max - dial.scale_min)
        }
    }
}

/// Absolute digital readout in CVF `ScaleRange` units (`CabViewDigitalRenderer`).
pub fn digital_control_value(
    control: &ControlType,
    digital: &openrailsrs_formats::CabDigitalParams,
    tel: &CabTelemetry,
) -> f64 {
    let dial = openrailsrs_formats::CabDialParams {
        scale_min: digital.scale_min,
        scale_max: digital.scale_max,
        units: digital.units.clone(),
        ..Default::default()
    };
    dial_control_value(control, &dial, tel)
}

/// Load CVF + shape runtime when cab interior assets are known.
pub fn load_cab_cvf_runtime(
    state: &mut CabCvfState,
    trainset_root: &Path,
    cab: &openrailsrs_formats::EngineCabView,
    cab_shape: &Path,
) {
    let assets = resolve_cab_assets(trainset_root, cab)
        .or_else(|| fallback_cab_assets_from_shape(cab_shape));
    let Some(assets) = assets else {
        viewer_log!(
            "openrailsrs-viewer3d: cab CVF — no .cvf resolved under {}",
            trainset_root.display()
        );
        return;
    };
    if !assets.cvf_path.is_file() {
        viewer_log!(
            "openrailsrs-viewer3d: cab CVF missing {}",
            assets.cvf_path.display()
        );
        return;
    }
    if state.cvf_path.as_deref() == Some(assets.cvf_path.as_path()) && state.runtime.is_some() {
        return;
    }
    let Ok(openrailsrs_formats::MstsFile::CabView(cvf)) = parse_msts_file(&assets.cvf_path) else {
        viewer_log!(
            "openrailsrs-viewer3d: failed to parse cab CVF {}",
            assets.cvf_path.display()
        );
        return;
    };
    let Ok(shape) = ShapeFile::from_path(cab_shape) else {
        viewer_log!(
            "openrailsrs-viewer3d: failed to parse cab shape for CVF {}",
            cab_shape.display()
        );
        return;
    };
    let runtime = build_cab_cvf_runtime(cvf, shape);
    let lever_count = runtime
        .matrix_drivers
        .values()
        .filter(|d| matches!(d, MatrixDriver::Lever { .. }))
        .count();
    viewer_log!(
        "openrailsrs-viewer3d: cab CVF {} — {} controls, {} matrix bindings ({} levers)",
        assets.cvf_path.display(),
        runtime.cvf.controls.len(),
        runtime.matrix_drivers.len(),
        lever_count,
    );
    state.cvf_path = Some(assets.cvf_path);
    state.runtime = Some(runtime);
    state.lever_keys.clear();
}

fn fallback_cab_assets_from_shape(cab_shape: &Path) -> Option<ResolvedCabAssets> {
    if !cab_shape.is_file() {
        return None;
    }
    let cvf_path = cab_shape.with_extension("cvf");
    if !cvf_path.is_file() {
        return None;
    }
    Some(ResolvedCabAssets {
        cab_dir: cab_shape.parent()?.to_path_buf(),
        shape_path: cab_shape.to_path_buf(),
        cvf_path,
    })
}

pub fn reset_cab_cvf_state(state: &mut CabCvfState) {
    *state = CabCvfState::default();
}

/// Primary matrix index for a `prim_state` (leaf `vtx_state.matrix_idx`).
pub fn matrix_idx_for_prim_state(shape: &ShapeFile, prim_state_idx: i32) -> Option<usize> {
    let ps = shape.prim_states.get(prim_state_idx.max(0) as usize)?;
    let vtx = shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize)?;
    if vtx.matrix_idx >= 0 {
        Some(vtx.matrix_idx as usize)
    } else {
        None
    }
}

/// Static bone transform for cab lever spawn (M0-baked mesh, pivot at matrix translation).
pub fn static_matrix_transform(shape: &ShapeFile, matrix_idx: usize) -> Transform {
    shape
        .matrices
        .get(matrix_idx)
        .map(|m| matrix43_to_transform(&m.matrix))
        .unwrap_or(Transform::IDENTITY)
}

/// Spawn transform for a rebased cab lever (optional mesh-center pivot).
pub fn static_lever_transform(
    shape: &ShapeFile,
    matrix_idx: usize,
    pivot_at_mesh: Option<Vec3>,
) -> Transform {
    let mut t = static_matrix_transform(shape, matrix_idx);
    if let Some(center) = pivot_at_mesh {
        t.translation = center;
    }
    t
}

/// True when the shape exposes MSTS animation controllers for this lever bone.
pub fn lever_has_authored_animation(shape: &ShapeFile, anim_node: Option<usize>) -> bool {
    shape.animations.first().is_some_and(|anim| {
        anim_node
            .and_then(|i| anim.nodes.get(i))
            .is_some_and(|n| !n.controllers.is_empty())
    })
}

fn anim_key_for_lever(shape: &ShapeFile, anim_node: Option<usize>, value: f64) -> f32 {
    let Some(anim) = shape.animations.first() else {
        return value as f32;
    };
    let Some(node_idx) = anim_node else {
        return value as f32 * anim.frame_count.max(1) as f32;
    };
    let Some(node) = anim.nodes.get(node_idx) else {
        return value as f32;
    };
    let max_frame = node
        .controllers
        .iter()
        .filter_map(|c| match c {
            openrailsrs_formats::AnimController::LinearPos { keys } => keys.last().map(|k| k.0),
            openrailsrs_formats::AnimController::SlerpRot { keys }
            | openrailsrs_formats::AnimController::TcbRot { keys } => keys.last().map(|k| k.0),
        })
        .fold(1.0f32, f32::max);
    (value as f32 * max_frame).clamp(0.0, max_frame)
}

/// Apply CVF / matrix animation from live telemetry.
pub fn update_cab_cvf_controls(
    time: Res<Time>,
    follow: Res<CameraFollowMode>,
    live: Option<Res<LiveDrive>>,
    mut cvf_state: Option<ResMut<CabCvfState>>,
    interior: Query<Entity, With<CabInteriorRoot>>,
    mut parts: Query<(&CabCvfPart, &mut Transform, &mut Visibility), With<CabInteriorMarker>>,
) {
    if *follow != CameraFollowMode::DriverCam {
        return;
    }
    let Some(live) = live else {
        return;
    };
    let Some(cvf_state) = cvf_state.as_mut() else {
        return;
    };
    let Some(runtime) = cvf_state.runtime.clone() else {
        return;
    };
    if interior.is_empty() {
        return;
    }

    let tel = live.session.cab_telemetry();
    let smooth = 1.0 - (-12.0_f32 * time.delta_secs()).exp();

    for (part, mut transform, mut visibility) in &mut parts {
        let Some(driver) = runtime.matrix_drivers.get(&part.matrix_idx) else {
            continue;
        };
        match driver {
            MatrixDriver::Lever {
                control,
                anim_node,
                ..
            } => {
                *visibility = Visibility::Visible;
                let has_anim = lever_has_authored_animation(&runtime.shape, *anim_node);
                if !has_anim {
                    // OR leaves static 3D meshes when the shape has no controllers;
                    // CVF 2D overlay provides the visible lever animation (#147/#148).
                    *transform = static_lever_transform(
                        &runtime.shape,
                        part.matrix_idx,
                        part.pivot_at_mesh,
                    );
                    continue;
                }
                let value = control_value(control, &tel);
                let target_key = anim_key_for_lever(&runtime.shape, *anim_node, value);
                let key = cvf_state
                    .lever_keys
                    .entry(part.matrix_idx)
                    .and_modify(|current| *current += (target_key - *current) * smooth)
                    .or_insert(target_key);
                let pose_mats = animation_pose_matrices(&runtime.shape, *key);
                *transform = if let Some(center) = part.pivot_at_mesh {
                    lever_entity_transform_at_mesh_center(
                        &runtime.shape,
                        part.matrix_idx,
                        center,
                        &pose_mats,
                    )
                } else {
                    lever_entity_transform_rebased(&runtime.shape, part.matrix_idx, &pose_mats)
                };
            }
            MatrixDriver::MultiState {
                control,
                order,
                anim_node,
                ..
            } => {
                *visibility = Visibility::Visible;
                let value = multi_state_normalized_value(&runtime.cvf, control, *order, &tel);
                if lever_has_authored_animation(&runtime.shape, *anim_node) {
                    let target_key = anim_key_for_lever(&runtime.shape, *anim_node, value);
                    let key = cvf_state
                        .lever_keys
                        .entry(part.matrix_idx)
                        .and_modify(|current| *current += (target_key - *current) * smooth)
                        .or_insert(target_key);
                    let pose_mats = animation_pose_matrices(&runtime.shape, *key);
                    *transform = if let Some(center) = part.pivot_at_mesh {
                        lever_entity_transform_at_mesh_center(
                            &runtime.shape,
                            part.matrix_idx,
                            center,
                            &pose_mats,
                        )
                    } else {
                        lever_entity_transform_rebased(
                            &runtime.shape,
                            part.matrix_idx,
                            &pose_mats,
                        )
                    };
                } else if let Some(CabControl::Dial { dial, .. }) =
                    cvf_control_at_order(&runtime.cvf, control, *order)
                {
                    // Needle rotation when the shape has no anim controllers (OR MultiState
                    // still drives dials via GetRangeFraction when frames exist).
                    let reading = dial_control_value(control, dial, &tel);
                    let angle = dial.rotation_radians(reading);
                    let mut base = static_lever_transform(
                        &runtime.shape,
                        part.matrix_idx,
                        part.pivot_at_mesh,
                    );
                    let axis = part.local_spin_axis.unwrap_or(Vec3::NEG_Z);
                    base.rotation *= Quat::from_axis_angle(axis, angle);
                    *transform = base;
                } else {
                    *transform = static_lever_transform(
                        &runtime.shape,
                        part.matrix_idx,
                        part.pivot_at_mesh,
                    );
                }
            }
            MatrixDriver::GaugeNative { .. } | MatrixDriver::Digit { .. } => {
                // Quads spawned by cab native instruments (#157 follow-up); matrix pivot only.
            }
        }
    }
}

fn multi_state_normalized_value(
    cvf: &CabViewFile,
    control: &ControlType,
    order: u32,
    tel: &CabTelemetry,
) -> f64 {
    match cvf_control_at_order(cvf, control, order) {
        Some(CabControl::Dial { dial, .. }) => dial.range_fraction(dial_control_value(control, dial, tel)),
        Some(CabControl::Digital { digital, .. }) => {
            let v = digital_control_value(control, digital, tel);
            if (digital.scale_max - digital.scale_min).abs() < f64::EPSILON {
                0.0
            } else {
                ((v - digital.scale_min) / (digital.scale_max - digital.scale_min)).clamp(0.0, 1.0)
            }
        }
        Some(CabControl::MultiStateDisplay { .. }) => {
            let v = control_value(control, tel);
            let idx = pick_multi_state_index(cvf, control, v);
            // Approximate frame fraction from discrete index.
            (idx as f64 / 7.0).clamp(0.0, 1.0)
        }
        _ => control_value(control, tel),
    }
}

pub fn pick_multi_state_index(cvf: &CabViewFile, control: &ControlType, value: f64) -> usize {
    for cab_control in &cvf.controls {
        let (ctrl_type, states) = match cab_control {
            CabControl::MultiStateDisplay {
                control_type,
                states,
                ..
            } if control_type == control || types_match(control_type, control) => {
                (control_type, states)
            }
            _ => continue,
        };
        let _ = ctrl_type;
        if states.is_empty() {
            return 0;
        }
        let mut best = 0usize;
        let mut best_dist = f64::INFINITY;
        for (i, state) in states.iter().enumerate() {
            let dist = (state.switch_val - value).abs();
            if dist < best_dist {
                best_dist = dist;
                best = i;
            }
        }
        return best;
    }
    ((value * 8.0).round() as usize).min(7)
}

pub fn types_match(a: &ControlType, b: &ControlType) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
        || a.as_str().eq_ignore_ascii_case(b.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::ControlState;
    use std::collections::HashSet;

    #[test]
    fn control_value_maps_throttle_and_brake() {
        let tel = CabTelemetry {
            speed_kmh: 50.0,
            limit_kmh: 80.0,
            throttle_pct: 75.0,
            brake_pct: 25.0,
            direction: 0.5,
            horn_active: false,
            wiper_active: false,
            main_res_bar: 8.0,
            brake_pipe_bar: 4.0,
            brake_cyl_bar: 1.0,
            brake_force_kn: 80.0,
            diesel_rpm: Some(900.0),
            boiler_bar: None,
            overspeed: false,
        };
        assert!((control_value(&ControlType::Throttle, &tel) - 0.75).abs() < 1e-6);
        assert!((control_value(&ControlType::TrainBrake, &tel) - 0.25).abs() < 1e-6);
        assert!((control_value(&ControlType::DirectionDisplay, &tel) - 0.5).abs() < 1e-6);
        assert!((control_value(&ControlType::Generic("WIPERS".into()), &tel) - 0.0).abs() < 1e-6);
        let mut tel_w = tel.clone();
        tel_w.wiper_active = true;
        assert!((control_value(&ControlType::Generic("WIPERS".into()), &tel_w) - 1.0).abs() < 1e-6);
        let dial = openrailsrs_formats::CabDialParams {
            scale_min: 0.0,
            scale_max: 100.0,
            units: Some("MILES_PER_HOUR".into()),
            ..Default::default()
        };
        let mph = dial_control_value(&ControlType::Speedometer, &dial, &tel);
        assert!((mph - 50.0 * 0.621_371).abs() < 1e-3);
    }

    fn empty_rect() -> openrailsrs_formats::ScreenRect {
        openrailsrs_formats::ScreenRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    #[test]
    fn matrix_driver_parses_or_throttle_name() {
        let shape = ShapeFile::default();
        let cvf = CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![],
        };
        let driver = matrix_driver_from_name("THROTTLE:0:0", &shape, 8, &cvf).expect("driver");
        assert!(matches!(
            driver,
            MatrixDriver::Lever {
                control: ControlType::Throttle,
                order: 0,
                ..
            }
        ));
    }

    #[test]
    fn parse_matrix_control_name_type_order_params_and_subpart() {
        let p = parse_matrix_control_name("SPEEDOMETER:1:14").expect("parse");
        assert_eq!(p.control, ControlType::Speedometer);
        assert_eq!(p.order, 1);
        assert_eq!(p.param1, "14");
        assert!(p.param2.is_empty());
        assert_eq!(p.sub_part, 0);

        let g = parse_matrix_control_name("AMMETER:1:10:100").expect("gauge");
        assert_eq!(g.param1, "10");
        assert_eq!(g.param2, "100");

        let brake = parse_matrix_control_name("TRAIN_BRAKE:0:0-1").expect("sub");
        assert_eq!(brake.order, 0);
        assert_eq!(brake.sub_part, 1);
    }

    #[test]
    fn matrix_driver_digital_cvf_becomes_digit() {
        let shape = ShapeFile::default();
        let cvf = CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![CabControl::Digital {
                control_type: ControlType::Speedometer,
                position: empty_rect(),
                digital: Default::default(),
            }],
        };
        let driver =
            matrix_driver_from_name("SPEEDOMETER:0:14", &shape, 3, &cvf).expect("digit driver");
        assert_eq!(
            driver,
            MatrixDriver::Digit {
                control: ControlType::Speedometer,
                order: 0,
                height_mm: 14.0,
                font_ace: None,
            }
        );
    }

    #[test]
    fn cvf_control_at_order_skips_other_types() {
        let cvf = CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![
                CabControl::Dial {
                    control_type: ControlType::Ammeter,
                    position: empty_rect(),
                    graphic: String::new(),
                    dial: Default::default(),
                },
                CabControl::Dial {
                    control_type: ControlType::Speedometer,
                    position: empty_rect(),
                    graphic: String::new(),
                    dial: Default::default(),
                },
                CabControl::Dial {
                    control_type: ControlType::Speedometer,
                    position: openrailsrs_formats::ScreenRect {
                        x: 1.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    graphic: "b.ace".into(),
                    dial: Default::default(),
                },
            ],
        };
        let second = cvf_control_at_order(&cvf, &ControlType::Speedometer, 1).expect("order 1");
        match second {
            CabControl::Dial { graphic, .. } => assert_eq!(graphic, "b.ace"),
            other => panic!("expected Dial, got {other:?}"),
        }
    }

    #[test]
    fn pick_multi_state_selects_closest_switch_val() {
        let cvf = CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![CabControl::MultiStateDisplay {
                control_type: ControlType::ThrottleDisplay,
                position: openrailsrs_formats::ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                graphic: "t.ace".into(),
                states: vec![
                    ControlState {
                        style: 0,
                        switch_val: 0.0,
                    },
                    ControlState {
                        style: 0,
                        switch_val: 0.5,
                    },
                    ControlState {
                        style: 0,
                        switch_val: 1.0,
                    },
                ],
            }],
        };
        assert_eq!(
            pick_multi_state_index(&cvf, &ControlType::ThrottleDisplay, 0.48),
            1
        );
    }

    #[test]
    fn pullman_cab_cvf_matrix_probe_when_content_present() {
        let shape_path = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        let cvf_path = shape_path.with_extension("cvf");
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(shape_path).expect("shape");
        let cvf = match parse_msts_file(&cvf_path).expect("cvf") {
            openrailsrs_formats::MstsFile::CabView(cvf) => cvf,
            other => panic!("expected CabView, got {other:?}"),
        };
        let runtime = build_cab_cvf_runtime(cvf, shape);
        assert!(
            !runtime.matrix_drivers.is_empty(),
            "expected matrix drivers from Pullman cab"
        );
        let levers = runtime
            .matrix_drivers
            .values()
            .filter(|d| matches!(d, MatrixDriver::Lever { .. }))
            .count();
        assert!(levers >= 1, "expected at least one lever matrix");
    }

    #[test]
    fn pullman_cvf_lever_binding_diagnostics() {
        let shape_path = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(shape_path).expect("shape");
        let cvf_path = shape_path.with_extension("cvf");
        let cvf = match parse_msts_file(&cvf_path).expect("cvf") {
            openrailsrs_formats::MstsFile::CabView(cvf) => cvf,
            other => panic!("expected CabView, got {other:?}"),
        };
        let runtime = build_cab_cvf_runtime(cvf, shape.clone());
        eprintln!("=== Pullman CVF levers ===");
        for (idx, driver) in &runtime.matrix_drivers {
            eprintln!("matrix {idx}: {driver:?}");
            if let MatrixDriver::Lever { anim_node, .. } = driver {
                eprintln!("  anim_node={anim_node:?}");
            }
        }
        eprintln!("vtx_states count={}", shape.vtx_states.len());
        for (i, v) in shape.vtx_states.iter().enumerate() {
            eprintln!("  vtx_state {i}: matrix_idx={}", v.matrix_idx);
        }
        for (pi, ps) in shape.prim_states.iter().enumerate() {
            let m = shape
                .vtx_states
                .get(ps.vertex_state_idx.max(0) as usize)
                .map(|v| v.matrix_idx)
                .unwrap_or(-1);
            eprintln!(
                "prim_state {pi} vtx_state {} matrix_idx={m}",
                ps.vertex_state_idx
            );
        }
        eprintln!("=== matrix names ===");
        for (i, m) in shape.matrices.iter().enumerate() {
            eprintln!("  {i}: {}", m.name);
        }
        for (pi, ps) in shape.prim_states.iter().enumerate() {
            let Some(vtx) = shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize) else {
                continue;
            };
            let name = shape
                .matrices
                .get(vtx.matrix_idx.max(0) as usize)
                .map(|m| m.name.as_str())
                .unwrap_or("?");
            eprintln!("prim {pi:2} matrix {} ({name})", vtx.matrix_idx);
        }
        eprintln!("=== prim → matrix (lever matrices only) ===");
        let lever_idxs: std::collections::HashSet<usize> = runtime
            .matrix_drivers
            .iter()
            .filter_map(|(i, d)| matches!(d, MatrixDriver::Lever { .. }).then_some(*i))
            .collect();
        for (pi, ps) in shape.prim_states.iter().enumerate() {
            let Some(vtx) = shape.vtx_states.get(ps.vertex_state_idx.max(0) as usize) else {
                continue;
            };
            if vtx.matrix_idx >= 0 && lever_idxs.contains(&(vtx.matrix_idx as usize)) {
                let name = shape
                    .matrices
                    .get(vtx.matrix_idx as usize)
                    .map(|m| m.name.as_str())
                    .unwrap_or("?");
                eprintln!("prim {pi} matrix {} ({name})", vtx.matrix_idx);
            }
        }
        if let Some(anim) = shape.animations.first() {
            eprintln!(
                "anim frame_count={} nodes={}",
                anim.frame_count,
                anim.nodes.len()
            );
        }
        if let Some(level) = shape
            .lod_controls
            .first()
            .and_then(|c| c.distance_levels.first())
        {
            let levers: HashSet<usize> = runtime
                .matrix_drivers
                .iter()
                .filter_map(|(i, d)| matches!(d, MatrixDriver::Lever { .. }).then_some(*i))
                .collect();
            for (mi, m) in shape.matrices.iter().enumerate() {
                if levers.contains(&mi) {
                    let r = &m.matrix.rows[3];
                    let row0 = &m.matrix.rows[0];
                    eprintln!(
                        "matrix {mi} {} pivot=({:.3},{:.3},{:.3}) ax0=({:.2},{:.2},{:.2})",
                        m.name, r[0], r[1], r[2], row0[0], row0[1], row0[2]
                    );
                }
            }
            eprintln!("=== cab_matrix_for_prim bindings (levers) ===");
            let parts = crate::shapes::build_mesh_parts_from_shape_lod_cab(&shape, level, &levers);
            for part in &parts {
                if let Some(m) = part.cab_matrix_idx {
                    if levers.contains(&m) {
                        let tex = part.texture_file.as_deref().unwrap_or("?");
                        let (c, h) = (
                            part.bounds_center.unwrap_or(Vec3::ZERO),
                            part.bounds_half_extent.unwrap_or(Vec3::ZERO),
                        );
                        let pivot =
                            crate::shapes::matrix_pivot_bevy(&shape, m).unwrap_or(Vec3::ZERO);
                        eprintln!(
                            "sub {} prim {} -> matrix {m} ({}) tex={tex} center=({:.3},{:.3},{:.3}) r={:.3} dist={:.3}",
                            part.sub_object_idx,
                            part.prim_state_idx,
                            shape.matrices[m].name,
                            c.x,
                            c.y,
                            c.z,
                            h.max_element(),
                            c.distance(pivot),
                        );
                    }
                }
            }
            eprintln!("hierarchy: {:?}", level.hierarchy);
            for (si, sub) in level.sub_objects.iter().enumerate() {
                eprintln!(
                    "sub_object {si}: verts={} prims={}",
                    sub.vertices.len(),
                    sub.primitives.len()
                );
                for prim in &sub.primitives {
                    let ps = shape
                        .prim_states
                        .get(prim.prim_state_idx.max(0) as usize)
                        .map(|p| p.vertex_state_idx)
                        .unwrap_or(-1);
                    eprintln!("  prim_state {} vtx_state_idx={ps}", prim.prim_state_idx);
                }
            }
        }
    }

    /// Pullman has no authored lever geometry (`vtx_state` → MAIN); no 3D rebase.
    #[test]
    fn pullman_static_cab_has_no_dynamic_lever_bindings() {
        let shape_path = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(shape_path).expect("shape");
        assert!(
            shape.animations.is_empty(),
            "Pullman cab shape must remain animation-free for this regression"
        );
        let cvf_path = shape_path.with_extension("cvf");
        let cvf = match parse_msts_file(&cvf_path).expect("cvf") {
            openrailsrs_formats::MstsFile::CabView(cvf) => cvf,
            other => panic!("expected CabView, got {other:?}"),
        };
        let runtime = build_cab_cvf_runtime(cvf, shape.clone());
        let lever_matrices = cab_lever_matrix_indices(&runtime);
        let level = shape
            .lod_controls
            .first()
            .and_then(|c| c.distance_levels.first())
            .expect("lod0");
        let parts =
            crate::shapes::build_mesh_parts_from_shape_lod_cab(&shape, level, &lever_matrices);
        assert!(
            parts.iter().all(|p| p.cab_matrix_idx.is_none()),
            "Pullman MAIN geometry must stay static (#146); got {:?}",
            parts
                .iter()
                .filter_map(|p| p.cab_matrix_idx)
                .collect::<Vec<_>>()
        );
        for driver in runtime.matrix_drivers.values() {
            if let MatrixDriver::Lever { anim_node, .. } = driver {
                assert!(
                    !lever_has_authored_animation(&runtime.shape, *anim_node),
                    "Pullman levers must not invent 3D animation (#147)"
                );
            }
        }
    }

    /// Scan cab LOD0 primitives near lever matrix pivots (content diagnostic).
    #[test]
    fn pullman_scan_lever_mesh_candidates() {
        use openrailsrs_bevy_scenery::shapes::{
            MeshBuffers, append_primitive_mesh_buffers, mesh_buffers_bounds, texture_for_prim_state,
        };
        use openrailsrs_formats::Vec3 as ShapeVec3;

        let shape_path = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(shape_path).expect("shape");
        let level = shape
            .lod_controls
            .first()
            .and_then(|c| c.distance_levels.first())
            .expect("lod0");
        let default_normal = shape.normals.first().copied().unwrap_or(ShapeVec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        });
        for matrix_idx in [4usize, 8, 9, 10] {
            let pivot = crate::shapes::matrix_pivot_bevy(&shape, matrix_idx).unwrap();
            eprintln!(
                "=== near matrix {matrix_idx} ({}) ===",
                shape.matrices[matrix_idx].name
            );
            for (sub_idx, sub) in level.sub_objects.iter().enumerate() {
                for (prim_ord, prim) in sub.primitives.iter().enumerate() {
                    let mut buffers = MeshBuffers::default();
                    append_primitive_mesh_buffers(
                        &shape,
                        level,
                        sub,
                        prim,
                        default_normal,
                        &mut buffers,
                        None,
                        false,
                    );
                    let (center, half) = mesh_buffers_bounds(&buffers);
                    let r = half.max_element();
                    if r < 0.03 {
                        continue;
                    }
                    let dist = center.distance(pivot);
                    if dist > 1.5 {
                        continue;
                    }
                    let tex =
                        texture_for_prim_state(&shape, prim.prim_state_idx).unwrap_or_default();
                    eprintln!(
                        "  sub {sub_idx} prim {prim_ord} ps={} tex={tex} r={r:.3} dist={dist:.3} center=({:.3},{:.3},{:.3})",
                        prim.prim_state_idx, center.x, center.y, center.z,
                    );
                }
            }
        }
    }

    #[test]
    fn pullman_named_lever_matrices_exist_but_stay_unbound() {
        let shape_path = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let shape = ShapeFile::from_path(shape_path).expect("shape");
        let cvf_path = shape_path.with_extension("cvf");
        let cvf = match parse_msts_file(&cvf_path).expect("cvf") {
            openrailsrs_formats::MstsFile::CabView(cvf) => cvf,
            other => panic!("expected CabView, got {other:?}"),
        };
        let runtime = build_cab_cvf_runtime(cvf, shape.clone());
        let lever_matrices = cab_lever_matrix_indices(&runtime);
        assert!(lever_matrices.contains(&4));
        assert!(lever_matrices.contains(&8));
        assert!(lever_matrices.contains(&9));
        let level = shape
            .lod_controls
            .first()
            .and_then(|c| c.distance_levels.first())
            .expect("lod0");
        let parts =
            crate::shapes::build_mesh_parts_from_shape_lod_cab(&shape, level, &lever_matrices);
        let bound: HashSet<usize> = parts.iter().filter_map(|p| p.cab_matrix_idx).collect();
        assert!(
            bound.is_empty(),
            "authored MAIN cab must not bind lever bones by texture (#146): {bound:?}"
        );
        assert!(
            !runtime.cvf.controls.is_empty(),
            "Pullman CVF should drive gauges/levers via 2D overlay"
        );
    }

    #[test]
    fn lever_has_authored_animation_false_without_controllers() {
        let shape = ShapeFile::default();
        assert!(!lever_has_authored_animation(&shape, None));
        assert!(!lever_has_authored_animation(&shape, Some(0)));
    }
}

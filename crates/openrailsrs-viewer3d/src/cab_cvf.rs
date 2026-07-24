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
use crate::coordinates::static_hierarchy_chain_transform_cab;
use crate::live::LiveDrive;
use crate::viewer_log;

/// Cached cab CVF + shape animation bindings for the active interior.
#[derive(Resource, Default, Debug)]
pub struct CabCvfState {
    pub cvf_path: Option<PathBuf>,
    pub runtime: Option<CabCvfRuntime>,
    /// Smoothed control positions (matrix index → shape frame or normalized fallback).
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
    let param1 = parts
        .get(2)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let param2 = parts
        .get(3)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
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

pub(crate) fn matrix_driver_from_name(
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
        Some(CabControl::Gauge { gauge, .. }) if gauge.is_pointer() => {
            Some(MatrixDriver::MultiState {
                control: parsed.control,
                order: parsed.order,
                param1: parsed.param1.parse().unwrap_or(0),
                sub_part: parsed.sub_part,
                anim_node,
            })
        }
        Some(CabControl::Gauge { .. }) => Some(MatrixDriver::GaugeNative {
            control: parsed.control,
            order: parsed.order,
            width_mm: parsed.param1.parse().unwrap_or(10.0),
            length_mm: parsed.param2.parse().unwrap_or(100.0),
        }),
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

/// Matrices whose bound meshes are rebaked to bone-local space (entity transform).
///
/// Includes 3D levers and MultiState dials/needles so omit-leaf bake +
/// `static_lever_transform` apply once (no double hierarchy after #172).
pub fn cab_rebase_matrix_indices(runtime: &CabCvfRuntime) -> HashSet<usize> {
    runtime
        .matrix_drivers
        .iter()
        .filter_map(|(idx, driver)| {
            matches!(
                driver,
                MatrixDriver::Lever { .. } | MatrixDriver::MultiState { .. }
            )
            .then_some(*idx)
        })
        .collect()
}

/// Matrix indices rebaked for CVF-driven cab parts (levers + MultiState).
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

/// Static bone transform for cab lever spawn (full leaf→root hierarchy, #171).
pub fn static_matrix_transform(shape: &ShapeFile, matrix_idx: usize) -> Transform {
    if matrix_idx >= shape.matrices.len() {
        return Transform::IDENTITY;
    }
    static_hierarchy_chain_transform_cab(shape, matrix_idx)
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

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProceduralCabMotion {
    local_axis: Vec3,
    angle_radians: f32,
    local_translation: Vec3,
}

/// Conservative motion for cab controls whose `.s` bone has no animation controller.
///
/// The authored matrix remains the rest pose. Continuous levers rotate around the
/// bone-local vertical axis; the reverser is centred at neutral and the horn button
/// is depressed along its local vertical axis.
fn procedural_cab_motion(
    control: &ControlType,
    normalized_value: f64,
) -> Option<ProceduralCabMotion> {
    let value = normalized_value.clamp(0.0, 1.0) as f32;
    let (angle_radians, local_translation) = match control {
        ControlType::Throttle | ControlType::ThrottleDisplay => {
            (-50.0_f32.to_radians() * value, Vec3::ZERO)
        }
        ControlType::TrainBrake => (65.0_f32.to_radians() * value, Vec3::ZERO),
        ControlType::DynamicBrakeDisplay => (-50.0_f32.to_radians() * value, Vec3::ZERO),
        ControlType::DirectionDisplay => (70.0_f32.to_radians() * (value - 0.5), Vec3::ZERO),
        ControlType::Generic(name) if name.eq_ignore_ascii_case("HORN") => {
            (0.0, Vec3::NEG_Y * (0.018 * value))
        }
        _ => return None,
    };
    Some(ProceduralCabMotion {
        local_axis: Vec3::Y,
        angle_radians,
        local_translation,
    })
}

fn procedural_control_transform(
    shape: &ShapeFile,
    matrix_idx: usize,
    pivot_at_mesh: Option<Vec3>,
    local_axis_override: Option<Vec3>,
    control: &ControlType,
    normalized_value: f64,
) -> Option<Transform> {
    let motion = procedural_cab_motion(control, normalized_value)?;
    let mut transform = static_lever_transform(shape, matrix_idx, pivot_at_mesh);
    let axis = local_axis_override
        .unwrap_or(motion.local_axis)
        .try_normalize()
        .unwrap_or(motion.local_axis);
    transform.rotation *= Quat::from_axis_angle(axis, motion.angle_radians);
    transform.translation += transform.rotation * motion.local_translation;
    Some(transform)
}

fn smoothed_control_value(
    lever_keys: &mut HashMap<usize, f32>,
    matrix_idx: usize,
    target: f32,
    smooth: f32,
) -> f32 {
    *lever_keys
        .entry(matrix_idx)
        .and_modify(|current| *current += (target - *current) * smooth)
        .or_insert(target)
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
    let CabCvfState {
        runtime,
        lever_keys,
        ..
    } = &mut **cvf_state;
    let Some(runtime) = runtime.as_ref() else {
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
                control, anim_node, ..
            } => {
                visibility.set_if_neq(Visibility::Visible);
                let has_anim = lever_has_authored_animation(&runtime.shape, *anim_node);
                if !has_anim {
                    let target = control_value(control, &tel) as f32;
                    let value = smoothed_control_value(lever_keys, part.matrix_idx, target, smooth);
                    let next_transform = procedural_control_transform(
                        &runtime.shape,
                        part.matrix_idx,
                        part.pivot_at_mesh,
                        part.local_spin_axis,
                        control,
                        value as f64,
                    )
                    .unwrap_or_else(|| {
                        static_lever_transform(&runtime.shape, part.matrix_idx, part.pivot_at_mesh)
                    });
                    transform.set_if_neq(next_transform);
                    continue;
                }
                let value = control_value(control, &tel);
                let target_key = anim_key_for_lever(&runtime.shape, *anim_node, value);
                let key = smoothed_control_value(lever_keys, part.matrix_idx, target_key, smooth);
                let pose_mats = animation_pose_matrices(&runtime.shape, key);
                let next_transform = if let Some(center) = part.pivot_at_mesh {
                    lever_entity_transform_at_mesh_center(
                        &runtime.shape,
                        part.matrix_idx,
                        center,
                        &pose_mats,
                    )
                } else {
                    lever_entity_transform_rebased(&runtime.shape, part.matrix_idx, &pose_mats)
                };
                transform.set_if_neq(next_transform);
            }
            MatrixDriver::MultiState {
                control,
                order,
                anim_node,
                ..
            } => {
                visibility.set_if_neq(Visibility::Visible);
                let value = multi_state_normalized_value(&runtime.cvf, control, *order, &tel);
                if lever_has_authored_animation(&runtime.shape, *anim_node) {
                    let target_key = anim_key_for_lever(&runtime.shape, *anim_node, value);
                    let key =
                        smoothed_control_value(lever_keys, part.matrix_idx, target_key, smooth);
                    let pose_mats = animation_pose_matrices(&runtime.shape, key);
                    let next_transform = if let Some(center) = part.pivot_at_mesh {
                        lever_entity_transform_at_mesh_center(
                            &runtime.shape,
                            part.matrix_idx,
                            center,
                            &pose_mats,
                        )
                    } else {
                        lever_entity_transform_rebased(&runtime.shape, part.matrix_idx, &pose_mats)
                    };
                    transform.set_if_neq(next_transform);
                } else if let Some(CabControl::Dial { dial, .. }) =
                    cvf_control_at_order(&runtime.cvf, control, *order)
                {
                    // Needle rotation when the shape has no anim controllers (OR MultiState
                    // still drives dials via GetRangeFraction when frames exist).
                    let reading = dial_control_value(control, dial, &tel);
                    let angle = dial.rotation_radians(reading);
                    let mut base =
                        static_lever_transform(&runtime.shape, part.matrix_idx, part.pivot_at_mesh);
                    let axis = part.local_spin_axis.unwrap_or(Vec3::NEG_Z);
                    base.rotation *= Quat::from_axis_angle(axis, angle);
                    transform.set_if_neq(base);
                } else {
                    // A discrete CVF display uses frame_index / frame_count, which
                    // is not the physical 0/1 state needed by a horn push button.
                    let procedural_target = control_value(control, &tel);
                    if procedural_cab_motion(control, procedural_target).is_none() {
                        transform.set_if_neq(static_lever_transform(
                            &runtime.shape,
                            part.matrix_idx,
                            part.pivot_at_mesh,
                        ));
                        continue;
                    }
                    let procedural_value = smoothed_control_value(
                        lever_keys,
                        part.matrix_idx,
                        procedural_target as f32,
                        smooth,
                    );
                    let next_transform = procedural_control_transform(
                        &runtime.shape,
                        part.matrix_idx,
                        part.pivot_at_mesh,
                        part.local_spin_axis,
                        control,
                        procedural_value as f64,
                    )
                    .expect("motion was checked above");
                    transform.set_if_neq(next_transform);
                }
            }
            MatrixDriver::GaugeNative { .. } | MatrixDriver::Digit { .. } => {
                // Quads: `cab_native_instruments` (own entities, not shape parts).
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
        Some(CabControl::Dial { dial, .. }) => {
            dial.range_fraction(dial_control_value(control, dial, tel))
        }
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
    use bevy::ecs::system::RunSystemOnce;
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
        assert!((control_value(&ControlType::Generic("HORN".into()), &tel) - 0.0).abs() < 1e-6);
        let mut tel_w = tel.clone();
        tel_w.wiper_active = true;
        assert!((control_value(&ControlType::Generic("WIPERS".into()), &tel_w) - 1.0).abs() < 1e-6);
        let mut tel_h = tel.clone();
        tel_h.horn_active = true;
        assert!((control_value(&ControlType::Generic("HORN".into()), &tel_h) - 1.0).abs() < 1e-6);
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
        assert!(matches!(
            runtime.matrix_drivers.get(&5),
            Some(MatrixDriver::MultiState {
                control: ControlType::Generic(name),
                anim_node: None,
                ..
            }) if name.eq_ignore_ascii_case("HORN")
        ));
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

    /// Pullman: desk MAIN stays static; authored vtx_state binds levers (#146 / #172).
    #[test]
    fn pullman_static_cab_desk_stays_unbound_authored_levers_bind() {
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
        assert_eq!(
            shape.vtx_states.get(10).map(|v| v.matrix_idx),
            Some(8),
            "parser must decode vtx_state imatrix as i32 (#172)"
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
        // MAIN-authored primitives must stay static; CVF bones bind only via vtx_state (#146).
        for part in &parts {
            let authored = matrix_idx_for_prim_state(&shape, part.prim_state_idx);
            if authored.is_none_or(|m| m == 0) {
                assert!(
                    part.cab_matrix_idx.is_none(),
                    "MAIN part sub={} prim={} must stay static (#146), got {:?}",
                    part.sub_object_idx,
                    part.prim_state_idx,
                    part.cab_matrix_idx
                );
            } else if let Some(m) = authored {
                if lever_matrices.contains(&m) {
                    assert_eq!(
                        part.cab_matrix_idx,
                        Some(m),
                        "authored M{m} part sub={} prim={} must bind",
                        part.sub_object_idx,
                        part.prim_state_idx
                    );
                }
            }
        }
        let bound: HashSet<usize> = parts.iter().filter_map(|p| p.cab_matrix_idx).collect();
        assert!(
            [4, 5, 8, 9, 10].into_iter().all(|m| bound.contains(&m)),
            "authored vtx_state must bind reverser, horn, throttle and brake matrices, got {bound:?}"
        );
        for driver in runtime.matrix_drivers.values() {
            if let MatrixDriver::Lever { anim_node, .. } = driver {
                assert!(
                    !lever_has_authored_animation(&runtime.shape, *anim_node),
                    "Pullman levers must remain classified as animation-free"
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
    fn pullman_authored_lever_sticks_rest_near_pivots() {
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
        let lever_only: HashSet<usize> = runtime
            .matrix_drivers
            .iter()
            .filter_map(|(i, d)| matches!(d, MatrixDriver::Lever { .. }).then_some(*i))
            .collect();
        let rebase = cab_lever_matrix_indices(&runtime);
        let level = shape
            .lod_controls
            .first()
            .and_then(|c| c.distance_levels.first())
            .expect("lod0");
        let parts = crate::shapes::build_mesh_parts_from_shape_lod_cab(&shape, level, &rebase);
        // Lever-bound sticks: authored vtx_state hierarchy places rest pose on the pivot.
        // bounds_center comes from a full-hierarchy bake (world / MAIN space).
        let mut checked = 0usize;
        for part in parts.iter().filter(|p| {
            p.cab_matrix_idx.is_some_and(|m| lever_only.contains(&m))
                && p.bounds_half_extent
                    .is_some_and(|h| h.max_element() < 0.25 && h.max_element() > 0.02)
        }) {
            let m = part.cab_matrix_idx.expect("bound");
            let world = part.bounds_center.unwrap_or(Vec3::ZERO);
            let pivot = crate::shapes::matrix_pivot_bevy(&shape, m).unwrap();
            assert!(
                world.distance(pivot) < 0.35,
                "lever stick sub={} → M{m} rest ({:.2},{:.2},{:.2}) far from pivot ({:.2},{:.2},{:.2})",
                part.sub_object_idx,
                world.x,
                world.y,
                world.z,
                pivot.x,
                pivot.y,
                pivot.z
            );
            checked += 1;
        }
        assert!(
            checked >= 4,
            "expected ≥4 authored lever sticks near pivots, checked {checked}"
        );
    }

    #[test]
    fn lever_has_authored_animation_false_without_controllers() {
        let shape = ShapeFile::default();
        assert!(!lever_has_authored_animation(&shape, None));
        assert!(!lever_has_authored_animation(&shape, Some(0)));
    }

    #[test]
    fn procedural_levers_move_from_the_authored_rest_pose() {
        let shape = ShapeFile::default();
        let rest = static_lever_transform(&shape, 0, None);

        let throttle =
            procedural_control_transform(&shape, 0, None, None, &ControlType::Throttle, 1.0)
                .expect("throttle fallback");
        let brake =
            procedural_control_transform(&shape, 0, None, None, &ControlType::TrainBrake, 1.0)
                .expect("brake fallback");
        assert!(
            rest.rotation.dot(throttle.rotation).abs() < 0.95,
            "full throttle must visibly rotate its authored bone"
        );
        assert!(
            rest.rotation.dot(brake.rotation).abs() < 0.90,
            "full brake must visibly rotate its authored bone"
        );

        let neutral = procedural_control_transform(
            &shape,
            0,
            None,
            None,
            &ControlType::DirectionDisplay,
            0.5,
        )
        .expect("reverser fallback");
        let reverse = procedural_control_transform(
            &shape,
            0,
            None,
            None,
            &ControlType::DirectionDisplay,
            0.0,
        )
        .expect("reverser reverse");
        let forward = procedural_control_transform(
            &shape,
            0,
            None,
            None,
            &ControlType::DirectionDisplay,
            1.0,
        )
        .expect("reverser forward");
        assert!(neutral.rotation.dot(rest.rotation).abs() > 0.9999);
        assert!(reverse.rotation.dot(forward.rotation).abs() < 0.85);
    }

    #[test]
    fn procedural_horn_depresses_and_returns_to_rest() {
        let shape = ShapeFile::default();
        let released = procedural_control_transform(
            &shape,
            0,
            None,
            None,
            &ControlType::Generic("HORN".into()),
            0.0,
        )
        .expect("horn fallback");
        let pressed = procedural_control_transform(
            &shape,
            0,
            None,
            None,
            &ControlType::Generic("horn".into()),
            1.0,
        )
        .expect("horn fallback");
        assert_eq!(released, Transform::IDENTITY);
        assert!((pressed.translation.y + 0.018).abs() < 1e-6);
        assert_eq!(pressed.rotation, released.rotation);
    }

    #[test]
    fn pullman_live_telemetry_moves_all_animation_free_controls() {
        let shape_path = std::path::Path::new(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s",
        );
        if !shape_path.is_file() {
            return;
        }
        let Some(mut live) = crate::test_harness::try_smoke_live_drive() else {
            return;
        };
        let shape = ShapeFile::from_path(shape_path).expect("shape");
        let cvf = match parse_msts_file(shape_path.with_extension("cvf")).expect("cvf") {
            openrailsrs_formats::MstsFile::CabView(cvf) => cvf,
            other => panic!("expected CabView, got {other:?}"),
        };
        let runtime = build_cab_cvf_runtime(cvf, shape.clone());

        live.session.driver_throttle = 0.8;
        live.session.driver_brake = 0.6;
        live.session.driver_direction = 1.0;
        live.session.trigger_horn(0.35);

        let mut app = crate::test_harness::minimal_app();
        app.insert_resource(live);
        app.insert_resource(CameraFollowMode::DriverCam);
        app.insert_resource(CabCvfState {
            runtime: Some(runtime),
            lever_keys: HashMap::from([
                (4, 0.5), // reverser starts neutral
                (5, 0.0),
                (8, 0.0),
                (9, 0.0),
                (10, 0.0),
            ]),
            ..Default::default()
        });
        app.world_mut().spawn(CabInteriorRoot);

        let mut entities = HashMap::new();
        for matrix_idx in [4usize, 5, 8, 9, 10] {
            let entity = app
                .world_mut()
                .spawn((
                    CabInteriorMarker,
                    CabCvfPart {
                        matrix_idx,
                        pivot_at_mesh: None,
                        local_spin_axis: None,
                    },
                    static_lever_transform(&shape, matrix_idx, None),
                    Visibility::Hidden,
                ))
                .id();
            entities.insert(matrix_idx, entity);
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(100));
        app.world_mut()
            .run_system_once(update_cab_cvf_controls)
            .expect("CVF control system");

        for matrix_idx in [4usize, 8, 9, 10] {
            let entity = entities[&matrix_idx];
            let actual = app
                .world()
                .entity(entity)
                .get::<Transform>()
                .copied()
                .expect("control transform");
            let rest = static_lever_transform(&shape, matrix_idx, None);
            assert!(
                actual.rotation.dot(rest.rotation).abs() < 0.99,
                "M{matrix_idx} must rotate from its authored rest pose"
            );
        }

        let horn = *app
            .world()
            .entity(entities[&5])
            .get::<Transform>()
            .expect("horn transform");
        let horn_rest = static_lever_transform(&shape, 5, None);
        assert!(
            horn.translation.distance(horn_rest.translation) > 0.01,
            "M5 horn must visibly depress while telemetry says it is active"
        );
    }
}

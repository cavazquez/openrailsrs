//! CVF-driven cab control animation (Open Rails `ThreeDimentionCabViewer` subset).
//!
//! Maps shape matrix names (`THROTTLE:0:0`, `TRAIN_BRAKE:0:0`, …) to parsed `.cvf`
//! controls and applies live telemetry in driver view.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_formats::{
    AnimController, CabControl, CabViewFile, ControlType, Matrix43, ResolvedCabAssets, ShapeFile,
    parse_msts_file, resolve_cab_assets,
};
use openrailsrs_sim::CabTelemetry;

use crate::cab_view::{CabInteriorMarker, CabInteriorRoot};
use crate::camera::CameraFollowMode;
use crate::coordinates::{hierarchy_chain_transform, static_hierarchy_chain_transform};
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
    Lever {
        control: ControlType,
        anim_node: Option<usize>,
    },
    MultiState {
        control: ControlType,
        state_index: u32,
        sub_part: u32,
    },
}

/// Marks a cab mesh part driven by a shape matrix index.
#[derive(Component, Clone, Copy, Debug)]
pub struct CabCvfPart {
    pub matrix_idx: usize,
}

/// Build matrix → control bindings from MSTS/OR matrix names.
pub fn build_cab_cvf_runtime(cvf: CabViewFile, shape: ShapeFile) -> CabCvfRuntime {
    let mut matrix_drivers = HashMap::new();
    for (idx, matrix) in shape.matrices.iter().enumerate() {
        if let Some(driver) = matrix_driver_from_name(&matrix.name, &shape, idx) {
            matrix_drivers.insert(idx, driver);
        }
    }
    let _ = &cvf; // CVF validates control types exist; matrix names are authoritative for 3D.
    CabCvfRuntime {
        cvf,
        shape,
        matrix_drivers,
    }
}

fn matrix_driver_from_name(
    name: &str,
    shape: &ShapeFile,
    matrix_idx: usize,
) -> Option<MatrixDriver> {
    let head = name.split('-').next()?.trim();
    let parts: Vec<&str> = head.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let control = control_type_from_matrix_prefix(parts[0].trim())?;
    let sub_part = name
        .split('-')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let state_index = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    let anim_node = shape.animations.first().and_then(|anim| {
        if matrix_idx < anim.nodes.len() && !anim.nodes[matrix_idx].controllers.is_empty() {
            Some(matrix_idx)
        } else {
            anim.nodes
                .iter()
                .position(|n| n.name.eq_ignore_ascii_case(name))
        }
    });

    if control_is_lever(&control) {
        Some(MatrixDriver::Lever { control, anim_node })
    } else {
        Some(MatrixDriver::MultiState {
            control,
            state_index,
            sub_part,
        })
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

/// Normalized 0–1 value for a cab control from live telemetry.
pub fn control_value(control: &ControlType, tel: &CabTelemetry) -> f64 {
    match control {
        ControlType::Throttle | ControlType::ThrottleDisplay => tel.throttle_pct / 100.0,
        ControlType::TrainBrake => tel.brake_pct / 100.0,
        ControlType::DynamicBrakeDisplay => tel.brake_pct / 100.0,
        ControlType::DirectionDisplay => 0.5,
        ControlType::Speedometer => {
            let scale = tel.limit_kmh.max(40.0);
            (tel.speed_kmh / scale).clamp(0.0, 1.0)
        }
        ControlType::MainRes => 0.8,
        ControlType::BrakeCyl | ControlType::BrakePipe => {
            (tel.brake_force_kn / 200.0).clamp(0.0, 1.0)
        }
        ControlType::Ammeter => tel
            .diesel_rpm
            .map(|r| (r / 1500.0).clamp(0.0, 1.0))
            .unwrap_or(0.0),
        _ => 0.0,
    }
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

fn animation_pose_matrices(shape: &ShapeFile, key: f32) -> Vec<Matrix43> {
    let mut pose: Vec<Matrix43> = shape.matrices.iter().map(|m| m.matrix).collect();
    let Some(anim) = shape.animations.first() else {
        return pose;
    };
    for (i, node) in anim.nodes.iter().enumerate() {
        if node.controllers.is_empty() || i >= pose.len() {
            continue;
        }
        pose[i] = animate_matrix(pose[i], &node.controllers, key);
    }
    pose
}

fn animate_matrix(base: Matrix43, controllers: &[AnimController], key: f32) -> Matrix43 {
    let mut m = base;
    for controller in controllers {
        m = apply_controller(m, controller, key);
    }
    m
}

fn apply_controller(mut m: Matrix43, controller: &AnimController, key: f32) -> Matrix43 {
    match controller {
        AnimController::LinearPos { keys } => {
            let Some((frame1, p1, frame2, p2)) = bracket_keys(keys, key) else {
                return m;
            };
            let t = if (frame2 - frame1).abs() > 1e-6 {
                ((key - frame1) / (frame2 - frame1)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let pos = lerp3(p1, p2, t);
            m.rows[3] = [pos[0] as f64, pos[1] as f64, pos[2] as f64];
            m
        }
        AnimController::SlerpRot { keys } | AnimController::TcbRot { keys } => {
            let Some((frame1, q1, frame2, q2)) = bracket_quat_keys(keys, key) else {
                return m;
            };
            let t = if (frame2 - frame1).abs() > 1e-6 {
                ((key - frame1) / (frame2 - frame1)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = Quat::from_xyzw(q1[0], q1[1], -q1[2], q1[3])
                .slerp(Quat::from_xyzw(q2[0], q2[1], -q2[2], q2[3]), t);
            set_matrix_rotation(&mut m, q);
            m
        }
    }
}

fn bracket_keys(keys: &[(f32, [f32; 3])], key: f32) -> Option<(f32, [f32; 3], f32, [f32; 3])> {
    if keys.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for (i, (frame, _)) in keys.iter().enumerate() {
        if *frame <= key {
            index = i;
        } else {
            break;
        }
    }
    let (frame1, p1) = keys[index];
    let (frame2, p2) = keys.get(index + 1).copied().unwrap_or(keys[index]);
    Some((frame1, p1, frame2, p2))
}

fn bracket_quat_keys(keys: &[(f32, [f32; 4])], key: f32) -> Option<(f32, [f32; 4], f32, [f32; 4])> {
    if keys.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for (i, (frame, _)) in keys.iter().enumerate() {
        if *frame <= key {
            index = i;
        } else {
            break;
        }
    }
    let (frame1, q1) = keys[index];
    let (frame2, q2) = keys.get(index + 1).copied().unwrap_or(keys[index]);
    Some((frame1, q1, frame2, q2))
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn set_matrix_rotation(m: &mut Matrix43, q: Quat) {
    let (x, y, z, w) = q.into();
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    m.rows[0] = [
        (1.0 - 2.0 * (yy + zz)) as f64,
        (2.0 * (xy + wz)) as f64,
        (2.0 * (xz - wy)) as f64,
    ];
    m.rows[1] = [
        (2.0 * (xy - wz)) as f64,
        (1.0 - 2.0 * (xx + zz)) as f64,
        (2.0 * (yz + wx)) as f64,
    ];
    m.rows[2] = [
        (2.0 * (xz + wy)) as f64,
        (2.0 * (yz - wx)) as f64,
        (1.0 - 2.0 * (xx + yy)) as f64,
    ];
}

/// Static bone transform for cab lever spawn (before CVF animation).
pub fn static_matrix_transform(shape: &ShapeFile, matrix_idx: usize) -> Transform {
    static_hierarchy_chain_transform(shape, matrix_idx)
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
            AnimController::LinearPos { keys } => keys.last().map(|k| k.0),
            AnimController::SlerpRot { keys } | AnimController::TcbRot { keys } => {
                keys.last().map(|k| k.0)
            }
        })
        .fold(1.0f32, f32::max);
    (value as f32 * max_frame).clamp(0.0, max_frame)
}

fn fallback_lever_rotation(
    _shape: &ShapeFile,
    _matrix_idx: usize,
    control: &ControlType,
    value: f64,
) -> Quat {
    let angle = match control {
        ControlType::Throttle => -0.75 + value * 0.75,
        ControlType::TrainBrake => -1.25 + value * 1.25,
        _ => value * 0.5 - 0.25,
    };
    // MSTS cab wheels / regulator: spin around local Y (vertical axis through pivot).
    let local_axis = match control {
        ControlType::Throttle | ControlType::TrainBrake => Vec3::Y,
        _ => Vec3::X,
    };
    Quat::from_axis_angle(local_axis, angle as f32)
}

fn lever_pose_from_fallback(
    shape: &ShapeFile,
    matrix_idx: usize,
    control: &ControlType,
    value: f64,
) -> Transform {
    let static_t = static_hierarchy_chain_transform(shape, matrix_idx);
    Transform {
        translation: static_t.translation,
        rotation: static_t.rotation * fallback_lever_rotation(shape, matrix_idx, control, value),
        scale: Vec3::ONE,
    }
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
    let mut lever_poses: HashMap<usize, Transform> = HashMap::new();

    for (matrix_idx, driver) in &runtime.matrix_drivers {
        if let MatrixDriver::Lever { control, anim_node } = driver {
            let value = control_value(control, &tel);
            let target_key = anim_key_for_lever(&runtime.shape, *anim_node, value);
            let key = cvf_state
                .lever_keys
                .entry(*matrix_idx)
                .and_modify(|current| *current += (target_key - *current) * smooth)
                .or_insert(target_key);
            let pose_mats = animation_pose_matrices(&runtime.shape, *key);
            let has_anim = runtime.shape.animations.first().is_some_and(|anim| {
                anim_node
                    .and_then(|i| anim.nodes.get(i))
                    .is_some_and(|n| !n.controllers.is_empty())
            });
            if has_anim {
                lever_poses.insert(
                    *matrix_idx,
                    hierarchy_chain_transform(&runtime.shape, *matrix_idx, &pose_mats),
                );
            } else {
                lever_poses.insert(
                    *matrix_idx,
                    lever_pose_from_fallback(&runtime.shape, *matrix_idx, control, value),
                );
            }
        }
    }

    for (part, mut transform, mut visibility) in &mut parts {
        let Some(driver) = runtime.matrix_drivers.get(&part.matrix_idx) else {
            continue;
        };
        match driver {
            MatrixDriver::Lever { control, .. } => {
                *visibility = Visibility::Visible;
                if let Some(pose) = lever_poses.get(&part.matrix_idx) {
                    *transform = *pose;
                } else {
                    let value = control_value(control, &tel);
                    *transform =
                        lever_pose_from_fallback(&runtime.shape, part.matrix_idx, control, value);
                }
            }
            MatrixDriver::MultiState {
                control,
                state_index,
                sub_part,
            } => {
                let value = control_value(control, &tel);
                let active = pick_multi_state_index(&runtime.cvf, control, value);
                let show = active == *state_index as usize && *sub_part == 0;
                *visibility = if show {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

fn pick_multi_state_index(cvf: &CabViewFile, control: &ControlType, value: f64) -> usize {
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

fn types_match(a: &ControlType, b: &ControlType) -> bool {
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
            brake_force_kn: 80.0,
            diesel_rpm: Some(900.0),
            boiler_bar: None,
            overspeed: false,
        };
        assert!((control_value(&ControlType::Throttle, &tel) - 0.75).abs() < 1e-6);
        assert!((control_value(&ControlType::TrainBrake, &tel) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn matrix_driver_parses_or_throttle_name() {
        let shape = ShapeFile::default();
        let driver = matrix_driver_from_name("THROTTLE:0:0", &shape, 8).expect("driver");
        assert!(matches!(
            driver,
            MatrixDriver::Lever {
                control: ControlType::Throttle,
                ..
            }
        ));
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
            let parts = crate::shapes::build_mesh_parts_from_shape_lod_cab(
                &shape,
                level,
                &runtime.matrix_drivers.keys().copied().collect(),
                &levers,
            );
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
}

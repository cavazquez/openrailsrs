//! MSTS shape matrix animation (OR `PrepareFrame` subset) — reusable for cab, bogies, world.

use bevy::prelude::*;
use openrailsrs_formats::{AnimController, Matrix43, ShapeFile};
use openrailsrs_or_shader::coordinates::{hierarchy_chain_transform, matrix43_to_transform};

/// Build animated pose matrices for all shape bones at animation key `key`.
pub fn animation_pose_matrices(shape: &ShapeFile, key: f32) -> Vec<Matrix43> {
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

/// World-space bone transform for rebased cab meshes (M0 bake + single-matrix pivot).
pub fn lever_entity_transform_rebased(
    shape: &ShapeFile,
    matrix_idx: usize,
    pose_mats: &[Matrix43],
) -> Transform {
    let Some(rest) = shape.matrices.get(matrix_idx) else {
        return Transform::IDENTITY;
    };
    let rest_t = matrix43_to_transform(&rest.matrix);
    let Some(animated) = pose_mats.get(matrix_idx) else {
        return rest_t;
    };
    let anim_t = matrix43_to_transform(animated);
    let delta_rot = rest_t.rotation.inverse() * anim_t.rotation;
    Transform {
        translation: rest_t.translation,
        rotation: rest_t.rotation * delta_rot,
        scale: Vec3::ONE,
    }
}

/// Like [`lever_entity_transform_rebased`] but keeps translation at baked mesh center (far 3D wheel).
pub fn lever_entity_transform_at_mesh_center(
    shape: &ShapeFile,
    matrix_idx: usize,
    mesh_center: Vec3,
    pose_mats: &[Matrix43],
) -> Transform {
    let Some(rest) = shape.matrices.get(matrix_idx) else {
        return Transform::from_translation(mesh_center);
    };
    let rest_t = matrix43_to_transform(&rest.matrix);
    let anim_t = pose_mats
        .get(matrix_idx)
        .map(matrix43_to_transform)
        .unwrap_or(rest_t);
    let delta_rot = rest_t.rotation.inverse() * anim_t.rotation;
    Transform {
        translation: mesh_center,
        rotation: rest_t.rotation * delta_rot,
        scale: Vec3::ONE,
    }
}

/// Full hierarchy transform (non-rebased meshes / world objects).
pub fn animated_hierarchy_transform(
    shape: &ShapeFile,
    matrix_idx: usize,
    pose_mats: &[Matrix43],
) -> Transform {
    hierarchy_chain_transform(shape, matrix_idx, pose_mats)
}

/// Runtime state for a generic MSTS shape animation driven by a scalar key.
#[derive(Component, Clone, Debug)]
pub struct ShapeAnimState {
    pub key: f32,
    pub matrix_idx: usize,
}

/// World / bogie pilot: advance animation key and apply hierarchy pose each frame.
pub fn update_world_shape_anim(
    time: Res<Time>,
    mut query: Query<(&mut ShapeAnimState, &ShapeAnimBinding, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut state, binding, mut transform) in &mut query {
        state.key += dt * binding.speed;
        let pose = animation_pose_matrices(&binding.shape, state.key);
        *transform = animated_hierarchy_transform(&binding.shape, state.matrix_idx, &pose);
    }
}

/// Binds a cloned shape + matrix index for generic world animation.
#[derive(Component, Clone)]
pub struct ShapeAnimBinding {
    pub shape: ShapeFile,
    pub matrix_idx: usize,
    pub speed: f32,
}

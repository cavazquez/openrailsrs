//! MSTS dynamic track segments from `.w` `Dyntrack` items (order 9 / issue #8).
//!
//! Each world `Dyntrack` anchor spawns a short oriented rail segment (sleepers +
//! two rail heads) along local +Z. Profile XML, `.tdb` linkage and curved sections
//! are out of scope for this first pass.

use bevy::prelude::*;

use crate::track::{SceneBounds, TrackScene};
use crate::world::WorldScene;

/// Standard gauge (metres).
pub const STD_GAUGE_M: f32 = 1.435;
const HALF_GAUGE_M: f32 = STD_GAUGE_M * 0.5;
const SLEEPER_WIDTH_M: f32 = 2.6;
const SLEEPER_HEIGHT_M: f32 = 0.22;
const RAIL_HEAD_H_M: f32 = 0.18;
const RAIL_HEAD_W_M: f32 = 0.08;

const COLOR_SLEEPER: Color = Color::srgb(0.42, 0.28, 0.18);
const COLOR_RAIL: Color = Color::srgb(0.55, 0.56, 0.60);

/// Default straight segment length derived from route extent (metres).
pub fn default_segment_length_m(bounds: &SceneBounds) -> f32 {
    (bounds.half_extent * 0.02).clamp(12.0, 40.0)
}

/// World-space end point of a segment anchored at `position` with `rotation`.
pub fn segment_end_world(position: Vec3, rotation: Quat, length_m: f32) -> Vec3 {
    position + rotation * Vec3::new(0.0, 0.0, length_m)
}

/// Transform for a unit cube scaled and placed in the segment's local frame.
pub fn part_transform(anchor: Vec3, rotation: Quat, local_center: Vec3, scale: Vec3) -> Transform {
    Transform {
        translation: anchor + rotation * local_center,
        rotation,
        scale,
    }
}

/// One-shot: spawn oriented rail segments for every `Dyntrack` in the world scene.
pub fn spawn_dyntrack_segments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    track: Res<TrackScene>,
) {
    let dyntracks: Vec<_> = world
        .items
        .iter()
        .filter(|obj| obj.kind == "Dyntrack")
        .collect();
    if dyntracks.is_empty() {
        return;
    }

    let count = dyntracks.len();
    let length = default_segment_length_m(&track.bounds);
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let sleeper_material = materials.add(StandardMaterial {
        base_color: COLOR_SLEEPER,
        perceptual_roughness: 0.9,
        metallic: 0.05,
        ..default()
    });
    let rail_material = materials.add(StandardMaterial {
        base_color: COLOR_RAIL,
        perceptual_roughness: 0.45,
        metallic: 0.65,
        ..default()
    });

    let rail_y = SLEEPER_HEIGHT_M + RAIL_HEAD_H_M * 0.5;
    let half_len = length * 0.5;

    for obj in &dyntracks {
        let sleeper = part_transform(
            obj.position,
            obj.rotation,
            Vec3::new(0.0, SLEEPER_HEIGHT_M * 0.5, half_len),
            Vec3::new(SLEEPER_WIDTH_M, SLEEPER_HEIGHT_M, length),
        );
        commands.spawn((
            Mesh3d(unit.clone()),
            MeshMaterial3d(sleeper_material.clone()),
            sleeper,
            Name::new(format!("dyntrack:{}:sleepers", obj.label)),
        ));

        for (side, name) in [(-HALF_GAUGE_M, "left"), (HALF_GAUGE_M, "right")] {
            let rail = part_transform(
                obj.position,
                obj.rotation,
                Vec3::new(side, rail_y, half_len),
                Vec3::new(RAIL_HEAD_W_M, RAIL_HEAD_H_M, length),
            );
            commands.spawn((
                Mesh3d(unit.clone()),
                MeshMaterial3d(rail_material.clone()),
                rail,
                Name::new(format!("dyntrack:{}:rail:{name}", obj.label)),
            ));
        }
    }

    eprintln!("openrailsrs-viewer3d: {count} dyntrack segment(s)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::world::load_world_from_route_dir;

    #[test]
    fn default_segment_length_clamps() {
        let small = SceneBounds::default_sandbox();
        assert_eq!(default_segment_length_m(&small), 12.0);

        let large = SceneBounds {
            half_extent: 50_000.0,
            ..SceneBounds::default_sandbox()
        };
        assert_eq!(default_segment_length_m(&large), 40.0);
    }

    #[test]
    fn segment_extends_along_local_z() {
        let end = segment_end_world(Vec3::new(10.0, 0.0, 5.0), Quat::IDENTITY, 20.0);
        assert_eq!(end, Vec3::new(10.0, 0.0, 25.0));
    }

    #[test]
    fn segment_respects_yaw_rotation() {
        let yaw = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let end = segment_end_world(Vec3::ZERO, yaw, 10.0);
        assert!((end.x - 10.0).abs() < 1e-4);
        assert!(end.z.abs() < 1e-4);
    }

    #[test]
    fn smoke_route_has_dyntrack_near_yard() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        let dyntrack = scene
            .items
            .iter()
            .find(|o| o.kind == "Dyntrack")
            .expect("dyntrack");
        assert!((dyntrack.position.x - 220.0).abs() < 0.1);
        assert!((dyntrack.position.z - 5.0).abs() < 0.1);
    }
}

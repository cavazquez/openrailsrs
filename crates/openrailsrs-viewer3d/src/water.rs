//! MSTS `HWater` horizontal surfaces from `.w` tiles (order 11 / issue #8).

use bevy::prelude::*;

use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::world::WorldScene;

const COLOR_WATER: Color = Color::srgba(0.10, 0.42, 0.68, 0.62);

/// Resolve the water surface height: explicit MSTS `Position.y`, else terrain sample.
pub fn water_surface_y(anchor: Vec3, terrain: Option<&TerrainElevation>, explicit_y: f32) -> f32 {
    if explicit_y.abs() > 1e-4 {
        return explicit_y;
    }
    terrain
        .and_then(|t| t.sample_world_y(anchor.x, anchor.z))
        .unwrap_or(0.0)
}

/// Spawn translucent planes for every `HWater` in the world scene.
pub fn spawn_water_patches(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    track: Res<TrackScene>,
    terrain: Option<Res<TerrainElevation>>,
) {
    let patches: Vec<_> = world
        .items
        .iter()
        .filter(|obj| obj.kind == "HWater" && obj.water.is_some())
        .collect();
    if patches.is_empty() {
        return;
    }

    let terrain_ref = terrain.as_deref();
    let material = materials.add(StandardMaterial {
        base_color: COLOR_WATER,
        emissive: LinearRgba::from(Color::srgb(0.05, 0.18, 0.32)) * 0.25,
        perceptual_roughness: 0.08,
        metallic: 0.0,
        reflectance: 0.65,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        ..default()
    });

    let mut spawned = 0usize;
    for obj in patches {
        let patch = obj.water.as_ref().expect("filtered");
        let y = water_surface_y(obj.position, terrain_ref, patch.surface_y);
        let width = patch.half_x * 2.0;
        let depth = patch.half_z * 2.0;
        let mesh = meshes.add(Plane3d::default().mesh().size(width, depth));
        let lift = track.bounds.edge_radius() * 0.05;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(obj.position.x, y + lift, obj.position.z),
            Name::new(format!("water:{}:{}", obj.label, patch.uid)),
        ));
        spawned += 1;
    }

    eprintln!("openrailsrs-viewer3d: {spawned} water patch(es)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::terrain::TerrainElevation;
    use crate::world::load_world_from_route_dir;

    #[test]
    fn explicit_y_overrides_terrain() {
        let y = water_surface_y(Vec3::new(10.0, 0.0, 10.0), None, 12.5);
        assert!((y - 12.5).abs() < 1e-5);
    }

    #[test]
    fn smoke_route_has_water_patch() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        let water = scene
            .items
            .iter()
            .find(|o| o.kind == "HWater")
            .expect("hwater");
        let patch = water.water.as_ref().expect("water meta");
        assert_eq!(patch.uid, 6);
        assert!((patch.half_x - 25.0).abs() < 0.1);
    }

    #[test]
    fn samples_terrain_when_y_zero() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let elev = TerrainElevation::load_from_route_dir(&route_dir);
        let y = water_surface_y(Vec3::new(100.0, 0.0, 100.0), Some(&elev), 0.0);
        assert!(y.is_finite());
    }
}

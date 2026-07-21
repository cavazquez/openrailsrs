//! MSTS `HWater` horizontal surfaces from `.w` tiles (order 11 / issue #8, PR2 waves).

use bevy::prelude::*;

use crate::shapes::{RouteAssets, load_ace_image};
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::viewer_log;
use crate::world::WorldScene;

const COLOR_WATER: Color = Color::srgba(0.08, 0.38, 0.62, 0.68);
const COLOR_WATER_REFLECT: Color = Color::srgba(0.04, 0.22, 0.38, 0.28);

/// Resolve the water surface height: explicit absolute `.w` Y (#64), else terrain sample.
pub fn water_surface_y(
    anchor: Vec3,
    terrain: Option<&TerrainElevation>,
    explicit_y: f32,
    focus: &crate::world::RouteFocus,
) -> f32 {
    if explicit_y.abs() > 1e-4 {
        return focus.scenery_y_to_msl(explicit_y);
    }
    terrain
        .and_then(|t| t.sample_world_y(anchor.x, anchor.z))
        .unwrap_or(focus.height_origin)
}

#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct WaterSurface {
    /// Surface height in render space (after [`crate::world::RouteFocus::to_render_surface`]).
    render_base_y: f32,
    phase: f32,
    is_reflection: bool,
}

/// Bevy approximation of OR `RenderItem.Comparer` water Y tie-break (#107).
///
/// Higher surfaces get a more negative `depth_bias` so they win over lower
/// HWater layers when distances are nearly equal and the camera is above water.
/// Exact OR sort (distance + Y sign) is not available in Bevy's transparent pass.
fn water_depth_bias_for_surface_y(surface_y: f32) -> f32 {
    -surface_y * 0.002
}

fn water_material(
    materials: &mut Assets<StandardMaterial>,
    texture: Option<Handle<Image>>,
    surface_y: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: COLOR_WATER,
        base_color_texture: texture,
        emissive: LinearRgba::from(Color::srgb(0.08, 0.24, 0.42)) * 0.45,
        perceptual_roughness: 0.06,
        metallic: 0.05,
        reflectance: 0.75,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        depth_bias: water_depth_bias_for_surface_y(surface_y),
        ..default()
    })
}

fn reflection_material(
    materials: &mut Assets<StandardMaterial>,
    surface_y: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: COLOR_WATER_REFLECT,
        emissive: LinearRgba::from(Color::srgb(0.05, 0.16, 0.28)) * 0.2,
        perceptual_roughness: 0.02,
        reflectance: 0.85,
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        // Slightly behind the main surface at the same Y.
        depth_bias: water_depth_bias_for_surface_y(surface_y) + 0.0005,
        ..default()
    })
}

/// Spawn translucent planes for every `HWater` in the world scene.
#[allow(clippy::too_many_arguments)]
pub fn spawn_water_patches(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    track: Res<TrackScene>,
    assets: Res<RouteAssets>,
    terrain: Option<Res<TerrainElevation>>,
    focus: Res<crate::world::RouteFocus>,
) {
    spawn_water_objects(
        &mut commands,
        &mut meshes,
        &mut images,
        &mut materials,
        &world.items,
        &track,
        terrain.as_deref(),
        &assets,
        &focus,
    );
}

/// Spawn water for a slice of world objects (tile streaming).
#[allow(clippy::too_many_arguments)]
pub fn spawn_water_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    items: &[crate::world::WorldObject],
    track: &TrackScene,
    terrain: Option<&TerrainElevation>,
    assets: &RouteAssets,
    focus: &crate::world::RouteFocus,
) {
    let patches: Vec<_> = items
        .iter()
        .filter(|obj| obj.kind == "HWater" && obj.water.is_some())
        .collect();
    if patches.is_empty() {
        return;
    }

    let mut texture_cache: std::collections::HashMap<String, Handle<Image>> =
        std::collections::HashMap::new();

    let mut spawned = 0usize;
    let mut textured = 0usize;
    for obj in patches {
        let patch = obj.water.as_ref().expect("filtered");
        let y = water_surface_y(obj.position, terrain, patch.surface_y, focus);
        let width = patch.half_x * 2.0;
        let depth = patch.half_z * 2.0;
        let mesh = meshes.add(Plane3d::default().mesh().size(width, depth));
        let lift = track.bounds.edge_radius() * 0.05;
        let base_y = y + lift;
        let phase = (patch.uid as f32 * 0.73).fract() * std::f32::consts::TAU;

        let texture = patch.texture_file.as_ref().and_then(|name| {
            if !name.to_ascii_lowercase().ends_with(".ace") {
                return None;
            }
            if let Some(handle) = texture_cache.get(name) {
                return Some(handle.clone());
            }
            let image = load_ace_image(&assets.route_dir, name)?;
            let handle = images.add(image);
            texture_cache.insert(name.clone(), handle.clone());
            Some(handle)
        });

        let material = if texture.is_some() {
            textured += 1;
            water_material(materials, texture, base_y)
        } else {
            water_material(materials, None, base_y)
        };
        let reflect_mat = reflection_material(materials, base_y);

        let render = focus.to_render_surface(Vec3::new(obj.position.x, base_y, obj.position.z));
        commands.spawn((
            WaterSurface {
                render_base_y: render.y,
                phase,
                is_reflection: false,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(render),
            Name::new(format!("water:{}:{}", obj.label, patch.uid)),
        ));

        let reflect_render =
            focus.to_render_surface(Vec3::new(obj.position.x, base_y - 0.05, obj.position.z));
        commands.spawn((
            WaterSurface {
                render_base_y: reflect_render.y,
                phase: phase + 1.1,
                is_reflection: true,
            },
            Mesh3d(mesh),
            MeshMaterial3d(reflect_mat),
            Transform::from_translation(reflect_render)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
            Name::new(format!("water-reflect:{}:{}", obj.label, patch.uid)),
        ));

        spawned += 1;
    }

    viewer_log!(
        "openrailsrs-viewer3d: {spawned} water patch(es){}",
        if textured > 0 {
            format!(" ({textured} textured)")
        } else {
            String::new()
        }
    );
}

pub(crate) fn update_water_patches(
    time: Res<Time>,
    mut surfaces: Query<(&mut Transform, &WaterSurface)>,
) {
    let t = time.elapsed_secs();
    for (mut transform, surface) in &mut surfaces {
        let amp = if surface.is_reflection { 0.018 } else { 0.07 };
        let wave = (t * 1.65 + surface.phase).sin() * amp;
        transform.translation.y = surface.render_base_y + wave;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::terrain::TerrainElevation;
    use crate::world::load_world_from_route_dir;

    #[test]
    fn higher_water_gets_more_negative_depth_bias() {
        let low = water_depth_bias_for_surface_y(10.0);
        let high = water_depth_bias_for_surface_y(20.0);
        assert!(high < low, "higher Y must sort nearer from above (OR #107)");
    }

    #[test]
    fn explicit_y_overrides_terrain() {
        let focus = crate::world::RouteFocus {
            center: Vec3::new(0.0, 10.0, 0.0),
            height_origin: 5.0,
        };
        let y = water_surface_y(Vec3::new(10.0, 0.0, 10.0), None, 12.5, &focus);
        // Explicit `.w` Y is absolute (#64); no `(y - center.y) + height_origin` remap.
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
        assert_eq!(patch.texture_file.as_deref(), Some("yard.ace"));
    }

    #[test]
    fn samples_terrain_when_y_zero() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let elev = TerrainElevation::load_from_route_dir(&route_dir);
        let focus = crate::world::RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let y = water_surface_y(Vec3::new(100.0, 0.0, 100.0), Some(&elev), 0.0, &focus);
        assert!(y.is_finite());
    }

    #[test]
    fn update_water_uses_render_local_y_not_msl() {
        use crate::world::RouteFocus;

        let focus = RouteFocus {
            center: Vec3::new(1_000_000.0, 80.0, 2_000_000.0),
            height_origin: 1_050.0,
        };
        let msl_y = 1_060.0;
        let render_y = focus
            .to_render_surface(Vec3::new(1_000_000.0, msl_y, 2_000_000.0))
            .y;
        assert!((render_y - 10.0).abs() < 1e-3);

        let surface = WaterSurface {
            render_base_y: render_y,
            phase: 0.0,
            is_reflection: false,
        };
        let mut tf = Transform::from_translation(Vec3::new(0.0, render_y, 0.0));
        let wave = 0.05_f32;
        tf.translation.y = surface.render_base_y + wave;
        assert!(
            tf.translation.y.abs() < 100.0,
            "wave update must stay in render space, got {}",
            tf.translation.y
        );
        assert!((tf.translation.y - (render_y + wave)).abs() < 1e-5);
    }

    #[test]
    fn water_wave_oscillates() {
        let surface = WaterSurface {
            render_base_y: 5.0,
            phase: 0.0,
            is_reflection: false,
        };
        let wave_a = (0.5_f32 * 1.65 + surface.phase).sin() * 0.07;
        let wave_b = (1.0_f32 * 1.65 + surface.phase).sin() * 0.07;
        assert_ne!(wave_a, wave_b);
    }
}

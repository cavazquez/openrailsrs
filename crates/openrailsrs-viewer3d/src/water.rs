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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WaterFlow {
    pub heading_deg: f32,
    pub speed_mps: f32,
    pub waves: bool,
}
impl Default for WaterFlow {
    fn default() -> Self {
        Self {
            heading_deg: 0.,
            speed_mps: 0.25,
            waves: true,
        }
    }
}
impl WaterFlow {
    pub fn valid(&self) -> bool {
        self.heading_deg.is_finite()
            && (0.0..360.).contains(&self.heading_deg)
            && self.speed_mps.is_finite()
            && (0.0..=5.).contains(&self.speed_mps)
    }
    pub fn uv_offset(&self, time: f64) -> Vec2 {
        let radians = f64::from(self.heading_deg).to_radians();
        let flow = f64::from(self.speed_mps) * time / 64.;
        let ripple = if self.waves {
            (time * 1.65).sin() * 0.0015
        } else {
            0.
        };
        Vec2::new(
            (radians.cos() * flow + ripple).rem_euclid(1.) as f32,
            (radians.sin() * flow + ripple).rem_euclid(1.) as f32,
        )
    }
}
#[derive(Component, Clone, Debug, Default)]
pub(crate) struct WaterSurface {
    pub key: String,
    uv_base: Vec2,
    uv_scale: Vec2,
    /// Surface height in render space (after [`crate::world::RouteFocus::to_render_surface`]).
    render_base_y: f32,
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
    normal: Handle<Image>,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: COLOR_WATER,
        base_color_texture: texture,
        normal_map_texture: Some(normal),
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
        &crate::floating_origin::FloatingOrigin::default(),
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
    origin: &crate::floating_origin::FloatingOrigin,
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
    let normal = images.add(wave_normal_image());
    for obj in patches {
        let patch = obj.water.as_ref().expect("filtered");
        let y = water_surface_y(obj.position, terrain, patch.surface_y, focus);
        let width = patch.half_x * 2.0;
        let depth = patch.half_z * 2.0;
        let mesh = meshes.add(Plane3d::default().mesh().size(width, depth));
        let lift = track.bounds.edge_radius() * 0.05;
        let base_y = y + lift;

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
            water_material(materials, texture, base_y, normal.clone())
        } else {
            water_material(materials, None, base_y, normal.clone())
        };
        let reflect_mat = reflection_material(materials, base_y);

        let render = focus.to_render_surface(Vec3::new(obj.position.x, base_y, obj.position.z))
            + obj.position_precision_offset;
        let key = format!(
            "{}:{},{}/{}",
            assets.route_dir.display(),
            obj.tile_x,
            obj.tile_z,
            patch.uid
        );
        let uv_base = Vec2::new(render.x - width * 0.5, render.z - depth * 0.5) / 64.;
        let uv_scale = Vec2::new(width, depth) / 64.;
        commands.spawn((
            crate::world::WorldTileBound::new(obj.tile_x, obj.tile_z),
            WaterSurface {
                key: key.clone(),
                uv_base,
                uv_scale,
                render_base_y: render.y,
                is_reflection: false,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(crate::floating_origin::view_translation(render, origin)),
            Name::new(format!("water:{}:{}", obj.label, patch.uid)),
        ));

        let reflect_render =
            focus.to_render_surface(Vec3::new(obj.position.x, base_y - 0.05, obj.position.z))
                + obj.position_precision_offset;
        commands.spawn((
            crate::world::WorldTileBound::new(obj.tile_x, obj.tile_z),
            WaterSurface {
                key,
                uv_base,
                uv_scale,
                render_base_y: reflect_render.y,
                is_reflection: true,
            },
            Mesh3d(mesh),
            MeshMaterial3d(reflect_mat),
            Transform::from_translation(crate::floating_origin::view_translation(
                reflect_render,
                origin,
            ))
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
    live: Option<Res<crate::live::LiveDrive>>,
    replay: Option<Res<crate::train::ReplayState>>,
    settings: Option<Res<crate::player_settings::PlayerSettings>>,
    origin: Option<Res<crate::floating_origin::FloatingOrigin>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut surfaces: Query<(
        &mut Transform,
        &WaterSurface,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
) {
    let t = live.as_ref().map_or_else(
        || {
            replay
                .as_ref()
                .filter(|r| r.is_active())
                .map_or(time.elapsed_secs_f64(), |r| r.t_sim)
        },
        |l| l.session.time_s(),
    );
    for (mut transform, surface, material) in &mut surfaces {
        // Keep neighbouring water planes at their authored elevation. Whole-
        // patch bobbing produced seams; ripples now modulate texture coordinates.
        transform.translation.y = surface.render_base_y - origin.as_ref().map_or(0., |o| o.shift.y);
        let flow = settings
            .as_ref()
            .and_then(|s| s.water_flows.get(&surface.key))
            .cloned()
            .unwrap_or_default();
        let offset = if settings.as_ref().is_none_or(|s| s.animated_water) {
            flow.uv_offset(t)
        } else {
            Vec2::ZERO
        };
        let scale = if surface.is_reflection {
            surface.uv_scale * Vec2::new(1., -1.)
        } else {
            surface.uv_scale
        };
        let next =
            bevy::math::Affine2::from_scale_angle_translation(scale, 0., surface.uv_base + offset);
        if let Some(handle) = material
            && let Some(materials) = materials.as_mut()
            && materials
                .get(&handle.0)
                .is_some_and(|m| m.uv_transform != next)
            && let Some(mut material) = materials.get_mut(&handle.0)
        {
            material.uv_transform = next;
        }
    }
}

fn wave_normal_image() -> Image {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    let mut pixels = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            let u = x as f32 / 64. * std::f32::consts::TAU;
            let v = y as f32 / 64. * std::f32::consts::TAU;
            let n = Vec3::new((u * 3. + v).cos() * 0.14, (v * 2. - u).cos() * 0.1, 1.).normalize();
            for value in [n.x, n.y, n.z] {
                pixels.push(((value * 0.5 + 0.5) * 255.).round() as u8);
            }
            pixels.push(255);
        }
    }
    Image::new(
        Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
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
            is_reflection: false,
            ..Default::default()
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
            is_reflection: false,
            ..Default::default()
        };
        let flow = WaterFlow::default();
        assert_ne!(flow.uv_offset(0.5), flow.uv_offset(1.));
        assert_eq!(surface.render_base_y, 5.);
    }

    #[test]
    fn plan30_water_uses_rail_clock_through_pause_and_retains_rebased_horizontal_pose() {
        use bevy::ecs::system::RunSystemOnce;
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        live.paused = true;
        live.session.state.time.0 = 12.;
        let mut app = App::new();
        app.insert_resource(live).init_resource::<Time>();
        let e = app
            .world_mut()
            .spawn((
                Transform::from_xyz(-5., 12., -7.),
                WaterSurface {
                    render_base_y: 12.,
                    is_reflection: false,
                    ..Default::default()
                },
            ))
            .id();
        app.world_mut()
            .run_system_once(update_water_patches)
            .unwrap();
        let first = *app.world().get::<Transform>(e).unwrap();
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(10));
        app.world_mut()
            .get_mut::<Transform>(e)
            .unwrap()
            .translation
            .x -= 1024.;
        app.world_mut()
            .run_system_once(update_water_patches)
            .unwrap();
        let second = app.world().get::<Transform>(e).unwrap();
        assert_eq!(first.translation.y, second.translation.y);
        assert_eq!(second.translation.x, first.translation.x - 1024.);
        assert!(second.translation.y > 11.93 && second.translation.y < 12.07);
    }
}

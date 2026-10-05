//! A stable star field on the celestial sphere. One mesh/material, no lights or
//! shadows. Weather and the route sun determine visibility, not wall-clock time.
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_bevy_scenery::SkyDome;

use crate::{
    live::LiveDrive,
    player_launch::{ActivePlayerContent, PlayerWeather},
    route_lighting::RouteSunState,
};

#[derive(Component)]
pub struct StarField;

pub fn star_visibility(sun_y: f32, weather: PlayerWeather) -> f32 {
    if weather != PlayerWeather::Clear {
        return 0.0;
    }
    let t = ((-sun_y - 0.02) / 0.16).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn star_mesh() -> Mesh {
    let count = 2400;
    let mut positions = Vec::<[f32; 3]>::with_capacity(count * 4);
    let mut colors = Vec::<[f32; 4]>::with_capacity(count * 4);
    let mut normals = Vec::<[f32; 3]>::with_capacity(count * 4);
    let mut uvs = Vec::<[f32; 2]>::with_capacity(count * 4);
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count {
        let seed = i as u32 + 811;
        let random = |channel| crate::precipitation::rain_rng01(seed, channel);
        // Uniform solid-angle sampling, avoiding a cluster at either pole.
        let y = random(0) * 2.0 - 1.0;
        let angle = random(1) * std::f32::consts::TAU;
        let r = (1.0 - y * y).sqrt();
        let direction = Vec3::new(r * angle.cos(), y, r * angle.sin());
        let right = direction.any_orthonormal_vector();
        let up = direction.cross(right);
        let brightness = 0.25 + 0.75 * random(2).powi(3);
        let size = 0.00035 + 0.0011 * brightness;
        let warmth = random(3);
        let color = [
            brightness,
            brightness * (0.82 + 0.18 * warmth),
            brightness * (0.75 + 0.25 * (1.0 - warmth)),
            1.0,
        ];
        let base = positions.len() as u32;
        for (x, y, uv) in [
            (-1.0, -1.0, [0.0, 0.0]),
            (1.0, -1.0, [1.0, 0.0]),
            (1.0, 1.0, [1.0, 1.0]),
            (-1.0, 1.0, [0.0, 1.0]),
        ] {
            positions.push((direction + (right * x + up * y) * size).to_array());
            normals.push((-direction).to_array());
            colors.push(color);
            uvs.push(uv);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

pub fn spawn_stars(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<crate::track::TrackScene>,
    mode: Res<crate::launch::ViewerSceneryMode>,
) {
    commands.spawn((
        StarField,
        Name::new("night-stars"),
        Mesh3d(meshes.add(star_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            fog_enabled: false,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        })),
        Transform::from_scale(Vec3::splat(
            crate::sky::sky_dome_radius(&scene, &mode) * 0.98,
        )),
        Visibility::Hidden,
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

pub fn update_stars(
    sun: Option<Res<RouteSunState>>,
    live: Option<Res<LiveDrive>>,
    content: Res<ActivePlayerContent>,
    environment: Option<Res<crate::environment::LiveEnvironment>>,
    cameras: Query<&Transform, (With<Camera3d>, Without<StarField>, Without<SkyDome>)>,
    mut stars: Query<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (With<StarField>, Without<Camera3d>, Without<SkyDome>),
    >,
    mut domes: Query<&mut Transform, (With<SkyDome>, Without<Camera3d>, Without<StarField>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(camera) = cameras.single() else { return };
    let opacity = star_visibility(sun.as_ref().map_or(1.0, |s| s.direction.y), content.weather);
    // Keep both backgrounds centred on the camera throughout the streamed route.
    for mut dome in &mut domes {
        dome.translation = camera.translation;
    }
    for (mut transform, mut visibility, material) in &mut stars {
        *visibility = if opacity > 0.001 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        transform.translation = camera.translation;
        let latitude = sun.as_ref().map_or(0.9, |s| s.position.latitude as f32);
        let rotation = environment
            .as_ref()
            .and_then(|e| e.real_clock(content.environment))
            .map_or_else(
                || {
                    live.as_ref().map_or(0.0, |l| {
                        (l.clock_time_s() / 86164.0) as f32 * std::f32::consts::TAU
                    })
                },
                |utc| {
                    ((utc.timestamp() as f64).rem_euclid(86164.0) / 86164.0) as f32
                        * std::f32::consts::TAU
                },
            );
        transform.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2 - latitude)
            * Quat::from_rotation_y(rotation);
        if let Some(mut m) = materials.get_mut(&material.0) {
            m.base_color = Color::WHITE.with_alpha(opacity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stars_follow_twilight_and_weather() {
        assert_eq!(star_visibility(0.5, PlayerWeather::Clear), 0.0);
        let twilight = star_visibility(-0.1, PlayerWeather::Clear);
        assert!(twilight > 0.0 && twilight < 1.0);
        assert_eq!(star_visibility(-0.3, PlayerWeather::Clear), 1.0);
        assert_eq!(star_visibility(-0.3, PlayerWeather::Rain), 0.0);
        assert_eq!(star_visibility(-0.3, PlayerWeather::Fog), 0.0);
    }
}

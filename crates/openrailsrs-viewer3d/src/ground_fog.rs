//! Optional native Bevy volumetric fog. A height-dependent density texture keeps
//! mist near the railway; the cheaper DistanceFog remains the default.
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{FogVolume, VolumetricFog, VolumetricLight};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::{
    live::LiveTrainMarker,
    player_launch::{ActivePlayerContent, PlayerWeather},
    player_settings::PlayerSettings,
    route_lighting::RouteSunLight,
    sky::FogState,
};

#[derive(Component)]
pub struct GroundFog;

fn density_texture() -> Image {
    const SIZE: u32 = 32;
    let mut data = Vec::with_capacity((SIZE * SIZE * SIZE) as usize);
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let h = y as f32 / (SIZE - 1) as f32;
                let noise = crate::precipitation::rain_rng01(x + z * SIZE, 6);
                data.push((255.0 * (-h * 6.0).exp() * (0.8 + noise * 0.2)) as u8);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: SIZE,
        },
        TextureDimension::D3,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::Repeat,
        ..default()
    });
    image
}

pub fn spawn_ground_fog(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        GroundFog,
        Name::new("railway-ground-fog"),
        FogVolume {
            density_texture: Some(images.add(density_texture())),
            density_factor: 0.0,
            scattering_asymmetry: 0.5,
            ..default()
        },
        Transform::from_scale(Vec3::new(12_000.0, 100.0, 12_000.0)),
        Visibility::Hidden,
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn update_ground_fog(
    mut commands: Commands,
    settings: Res<PlayerSettings>,
    fog_state: Res<FogState>,
    content: Res<ActivePlayerContent>,
    cameras: Query<
        (Entity, &Transform, Option<&VolumetricFog>),
        (With<Camera3d>, Without<GroundFog>),
    >,
    trains: Query<&Transform, (With<LiveTrainMarker>, Without<GroundFog>, Without<Camera3d>)>,
    sun: Query<Entity, (With<RouteSunLight>, Without<VolumetricLight>)>,
    mut volumes: Query<
        (&mut Transform, &mut FogVolume, &mut Visibility),
        (With<GroundFog>, Without<Camera3d>, Without<LiveTrainMarker>),
    >,
) {
    let Ok((entity, camera, current)) = cameras.single() else {
        return;
    };
    let steps = if fog_state.enabled {
        settings.fog_quality.steps()
    } else {
        None
    };
    if let Some(step_count) = steps {
        if current.is_none_or(|v| v.step_count != step_count) {
            commands.entity(entity).insert(VolumetricFog {
                step_count,
                ambient_intensity: 0.0,
                jitter: 0.0,
                ..default()
            });
        }
        for e in &sun {
            commands.entity(e).insert(VolumetricLight);
        }
    } else if current.is_some() {
        commands.entity(entity).remove::<VolumetricFog>();
    }
    let rail_y = trains.iter().next().map_or(0.0, |t| t.translation.y);
    for (mut transform, mut volume, mut visibility) in &mut volumes {
        transform.translation =
            Vec3::new(camera.translation.x, rail_y + 35.0, camera.translation.z);
        *visibility = if steps.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        let density = if steps.is_some() {
            match content.weather {
                PlayerWeather::Clear => 0.00008,
                PlayerWeather::Rain => 0.0006,
                PlayerWeather::Fog => 0.003,
            }
        } else {
            0.0
        };
        if volume.density_factor != density {
            volume.density_factor = density;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_settings::FogQuality;
    #[test]
    fn legacy_preferences_keep_the_low_cost_fog_model() {
        let settings: PlayerSettings = serde_json::from_str("{\"fog\":true}").unwrap();
        assert_eq!(settings.fog_quality, FogQuality::Distance);
        assert_eq!(FogQuality::Volumetric32.steps(), Some(32));
    }
}

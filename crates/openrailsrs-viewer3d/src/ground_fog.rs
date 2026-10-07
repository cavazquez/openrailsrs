//! Dense local fog and headlight scattering, with a distance-only alternative.
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

pub fn weather_visibility(weather: PlayerWeather) -> f32 {
    match weather {
        PlayerWeather::Fog => 120.0,
        PlayerWeather::Snow => 500.0,
        PlayerWeather::Storm => 4000.0,
        PlayerWeather::Rain => 7000.0,
        _ => crate::sky::CLEAR_WEATHER_VISIBILITY_M,
    }
}

#[derive(Resource, Default)]
pub struct FogDiagnostics {
    pub volumetric: bool,
    pub visibility_m: f32,
    pub density: f32,
    pub shader_fix_applied: bool,
}
impl FogDiagnostics {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"volumetric":self.volumetric,"visibility_m":self.visibility_m,
            "density_factor":self.density,"punctual_light_attenuation_corrected":self.shader_fix_applied})
    }
}

#[derive(Resource)]
pub(crate) struct FogShaderFix {
    handle: Handle<bevy::shader::Shader>,
    done: bool,
}

pub fn prepare_shader_fix(mut commands: Commands, server: Option<Res<AssetServer>>) {
    if let Some(server) = server {
        commands.insert_resource(FogShaderFix {
            handle: server.load("embedded://bevy_pbr/volumetric_fog/volumetric_fog.wgsl"),
            done: false,
        });
    }
}

/// Bevy 0.19.1 charges punctual lights the entire fog-volume radius and omits
/// their normalized scattering phase. Correct that loop only; keep directional
/// lighting. Otherwise distant lamps disappear, then the repaired beam clips
/// white when viewed from the cab.
/// Upstream: https://github.com/bevyengine/bevy/issues/25282
fn corrected_shader(source: &str) -> Option<String> {
    let old =
        "let light_attenuation = exp(-density * bounding_radius * (absorption + scattering));";
    let start = source.find("let light_to_frag = (*light).position_radius.xyz - P_world;")?;
    let pos = start + source[start..].find(old)?;
    let attenuation = format!(
        "{}{}{}",
        &source[..pos],
        "let light_attenuation = exp(-density * length(light_to_frag) * (absorption + scattering));",
        &source[pos + old.len()..]
    );
    let old_color = "let light_color_per_step = (*light).color_inverse_square_range.rgb * light_factors_per_step;";
    let pos = attenuation.find(old_color)?;
    let punctual = format!(
        "{}{}{}",
        &attenuation[..pos],
        "let light_color_per_step = (*light).color_inverse_square_range.rgb * henyey_greenstein(dot(L, V)) * light_factors_per_step;",
        &attenuation[pos + old_color.len()..]
    );
    let old_ambient = "var accumulated_color = exp(-ray_length_view * (absorption + scattering)) * ambient_color *\n        ambient_intensity;";
    let old_return = "return vec4(accumulated_color, 1.0 - background_alpha);";
    if !punctual.contains(old_ambient) || !punctual.contains(old_return) {
        return None;
    }
    // Homogeneous sky illumination has the bounded integral L_sky * albedo *
    // (1 - T). Use the transmittance already sampled through the density field,
    // rather than a radius-dependent term that turns daytime mist black.
    Some(punctual.replace(old_ambient, "var accumulated_color = vec3<f32>(0.0);").replace(old_return,
        "let ambient_scatter = ambient_color * ambient_intensity * scattering / max(absorption + scattering, 0.0001) * (1.0 - background_alpha);\n    return vec4(accumulated_color + ambient_scatter, 1.0 - background_alpha);"))
}

pub(crate) fn apply_shader_fix(
    fix: Option<ResMut<FogShaderFix>>,
    shaders: Option<ResMut<Assets<bevy::shader::Shader>>>,
    mut diagnostics: ResMut<FogDiagnostics>,
) {
    let (Some(mut fix), Some(mut shaders)) = (fix, shaders) else {
        return;
    };
    if fix.done {
        return;
    }
    let Some(mut shader) = shaders.get_mut(&fix.handle) else {
        return;
    };
    if let bevy::shader::Source::Wgsl(source) = &mut shader.source {
        if let Some(corrected) = corrected_shader(source) {
            *source = corrected.into();
            diagnostics.shader_fix_applied = true;
        } else {
            crate::viewer_log!(
                "fog: pinned Bevy punctual-light workaround not applied; shader layout changed"
            );
        }
    }
    fix.done = true;
}

#[derive(Component)]
pub struct GroundFog;

/// Smooth compact support eliminates the vertical wall produced by a uniform
/// 12 km box. DistanceFog handles distant extinction; this local layer is for
/// ground mist and headlight shafts in the local 400 m volume.
pub fn ground_density(uvw: Vec3) -> f32 {
    let radial = Vec2::new(uvw.x - 0.5, uvw.z - 0.5).length() * 2.0;
    let edge = ((1.0 - radial) / 0.35).clamp(0.0, 1.0);
    let edge = edge * edge * (3.0 - 2.0 * edge);
    let above_rail = (uvw.y * 45.0 - 8.0 - 4.0).max(0.0);
    (-above_rail / 18.0).exp() * edge
}
fn density_texture() -> Image {
    const SIZE: u32 = 32;
    let mut data = Vec::with_capacity((SIZE * SIZE * SIZE) as usize);
    for z in 0..SIZE {
        for y in 0..SIZE {
            for x in 0..SIZE {
                let uvw = Vec3::new(x as f32, y as f32, z as f32) / (SIZE - 1) as f32;
                data.push((255.0 * ground_density(uvw)).round() as u8);
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
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
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
            scattering_asymmetry: 0.65,
            absorption: 0.05,
            scattering: 0.35,
            ..default()
        },
        Transform::from_scale(Vec3::new(400.0, 45.0, 400.0)),
        Visibility::Hidden,
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn update_ground_fog(
    mut commands: Commands,
    settings: Res<PlayerSettings>,
    fog_state: Res<FogState>,
    content: Res<ActivePlayerContent>,
    weather: Option<Res<crate::weather_state::WeatherState>>,
    sun_state: Option<Res<crate::route_lighting::RouteSunState>>,
    live: Option<Res<crate::live::LiveDrive>>,
    pipelines: Option<Res<crate::performance::ScenePipelineStatus>>,
    mut diagnostics: ResMut<FogDiagnostics>,
    mut cameras: Query<
        (Entity, &Transform, Option<&mut VolumetricFog>),
        (With<Camera3d>, Without<GroundFog>),
    >,
    trains: Query<&Transform, (With<LiveTrainMarker>, Without<GroundFog>, Without<Camera3d>)>,
    sun: Query<Entity, (With<RouteSunLight>, Without<VolumetricLight>)>,
    mut volumes: Query<
        (&mut Transform, &mut FogVolume, &mut Visibility),
        (With<GroundFog>, Without<Camera3d>, Without<LiveTrainMarker>),
    >,
) {
    let Ok((entity, camera, mut current)) = cameras.single_mut() else {
        return;
    };
    let quality = std::env::var("OPENRAILSRS_FOG_QUALITY")
        .ok()
        .and_then(|value| match value.as_str() {
            "auto" => Some(crate::player_settings::FogQuality::Auto),
            "distance" => Some(crate::player_settings::FogQuality::Distance),
            "volumetric32" => Some(crate::player_settings::FogQuality::Volumetric32),
            "volumetric64" => Some(crate::player_settings::FogQuality::Volumetric64),
            _ => None,
        })
        .unwrap_or(settings.fog_quality);
    let auto = quality == crate::player_settings::FogQuality::Auto;
    let hardware = pipelines
        .as_ref()
        .and_then(|p| p.device())
        .is_some_and(|d| d.hardware);
    let steps = if fog_state.enabled
        && (!auto
            || hardware
                && weather.as_ref().map_or(
                    matches!(
                        content.weather,
                        PlayerWeather::Fog
                            | PlayerWeather::Storm
                            | PlayerWeather::Snow
                            | PlayerWeather::Rain
                    ),
                    |s| s.atmosphere.fog_density > 0.0001,
                )) {
        quality.steps()
    } else {
        None
    };
    if let Some(step_count) = steps {
        let direction = sun_state.as_ref().map_or(Vec3::Y, |s| s.direction);
        let mut palette = crate::sky::sky_parameters(
            direction.y,
            content.weather,
            direction,
            live.as_ref().map_or(0.0, |l| l.clock_time_s()),
        );
        if let Some(weather) = weather.as_ref() {
            palette = crate::sky::atmosphere_parameters(
                direction.y,
                &weather.atmosphere,
                direction,
                live.as_ref().map_or(0.0, |l| l.clock_time_s()),
            );
        }
        let ambient_color =
            Color::linear_rgb(palette.horizon.x, palette.horizon.y, palette.horizon.z);
        if let Some(volume) = current.as_mut() {
            volume.step_count = step_count;
            volume.ambient_color = ambient_color;
            volume.ambient_intensity = 1.0;
        } else {
            commands.entity(entity).insert(VolumetricFog {
                step_count,
                ambient_color,
                ambient_intensity: 1.0,
                jitter: 0.0,
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
            Vec3::new(camera.translation.x, rail_y + 14.5, camera.translation.z);
        *visibility = if steps.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        let mut density = if steps.is_some() {
            match content.weather {
                PlayerWeather::Clear => 0.00004,
                PlayerWeather::Rain => 0.0003,
                PlayerWeather::Overcast => 0.00008,
                PlayerWeather::Storm => 0.0005,
                PlayerWeather::Fog => 0.065,
                PlayerWeather::Snow => 0.0008,
            }
        } else {
            0.0
        };
        if let Some(weather) = weather.as_ref()
            && steps.is_some()
        {
            density = weather.atmosphere.fog_density;
        }
        if volume.density_factor != density {
            volume.density_factor = density;
        }
        diagnostics.volumetric = steps.is_some();
        diagnostics.visibility_m = weather.as_ref().map_or_else(
            || weather_visibility(content.weather),
            |s| s.atmosphere.visibility_m,
        );
        diagnostics.density = density;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_settings::FogQuality;
    #[test]
    fn dense_fog_obscures_nearby_track_without_an_edge_wall() {
        assert_eq!(ground_density(Vec3::new(1.0, 0.1, 0.5)), 0.0);
        assert_eq!(ground_density(Vec3::new(0.0, 0.1, 0.5)), 0.0);
        assert!(ground_density(Vec3::new(0.999, 0.1, 0.5)) < 0.0001);
        let height = (2.8 + 8.0) / 45.0;
        let optical_depth: f32 = (0..100)
            .map(|x| ground_density(Vec3::new(0.5 + x as f32 / 400.0, height, 0.5)) * 0.065 * 0.4)
            .sum();
        assert!(
            (-optical_depth).exp() < 0.1,
            "dense fog must extinguish contrast close to the train"
        );
    }
    #[test]
    fn missing_fog_quality_selects_auto_and_explicit_distance_is_preserved() {
        let settings: PlayerSettings = serde_json::from_str("{\"fog\":true}").unwrap();
        assert_eq!(settings.fog_quality, FogQuality::Auto);
        let explicit: PlayerSettings =
            serde_json::from_str("{\"fog_quality\":\"distance\"}").unwrap();
        assert_eq!(explicit.fog_quality, FogQuality::Distance);
        assert_eq!(FogQuality::Volumetric32.steps(), Some(32));
    }
    #[test]
    fn light_workaround_only_changes_punctual_attenuation() {
        let source = "var accumulated_color = exp(-ray_length_view * (absorption + scattering)) * ambient_color *\n        ambient_intensity;\nlet light_attenuation = exp(-density * bounding_radius * (absorption + scattering));\nlet light_color_per_step = (*light).color.rgb * phase * light_factors_per_step;\nlet light_to_frag = (*light).position_radius.xyz - P_world;\nlet light_attenuation = exp(-density * bounding_radius * (absorption + scattering));\nlet light_color_per_step = (*light).color_inverse_square_range.rgb * light_factors_per_step;\nreturn vec4(accumulated_color, 1.0 - background_alpha);";
        let corrected = corrected_shader(source).unwrap();
        assert_eq!(
            corrected
                .matches("bounding_radius * (absorption + scattering)")
                .count(),
            1
        );
        assert!(corrected.contains("length(light_to_frag) * (absorption + scattering)"));
        assert!(
            corrected.contains("color_inverse_square_range.rgb * henyey_greenstein(dot(L, V))")
        );
        assert!(corrected.contains("color.rgb * phase * light_factors_per_step"));
        assert!(corrected.contains("accumulated_color + ambient_scatter"));
        assert!(!corrected.contains("exp(-ray_length_view * (absorption + scattering))"));
        assert!(corrected_shader("different layout").is_none());
    }
}

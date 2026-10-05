//! Rain/snow change existing outdoor materials, without cloning textures.
use bevy::{asset::AssetId, prelude::*};
use std::collections::{HashMap, HashSet};
#[derive(Resource, Default)]
pub struct WetSurfaces {
    pub wetness: f32,
    pub snow_cover: f32,
    last_clock: Option<f64>,
    last_material_update: f64,
    last_applied_wetness: f32,
    bases: HashMap<AssetId<StandardMaterial>, (Color, f32, f32)>,
}
#[allow(clippy::type_complexity)]
pub fn update(
    time: Res<Time>,
    live: Res<crate::live::LiveDrive>,
    weather: Res<crate::player_launch::ActivePlayerContent>,
    mut state: ResMut<WetSurfaces>,
    outdoor: Query<
        (
            Option<&MeshMaterial3d<StandardMaterial>>,
            Option<&crate::surface_weather::SnowSurfaceSource>,
        ),
        Without<crate::cab_view::CabInteriorMarker>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain: ResMut<Assets<openrailsrs_bevy_scenery::OrTerrainMaterial>>,
    mut viewer_terrain: ResMut<Assets<openrailsrs_bevy_scenery::TerrainMaterial>>,
    mut scenery: ResMut<Assets<openrailsrs_bevy_scenery::OrSceneryMaterial>>,
    mut instances: Query<&mut crate::world_instancing::WorldInstanceAppearance>,
) {
    let clock = live.session.time_s();
    let previous = state.last_clock.replace(clock);
    let dt = (clock - previous.unwrap_or(clock)).clamp(0.0, 1.0) as f32;
    let rainy = matches!(
        weather.weather,
        crate::player_launch::PlayerWeather::Rain | crate::player_launch::PlayerWeather::Storm
    );
    let target = f32::from(rainy);
    let snowy = weather.weather == crate::player_launch::PlayerWeather::Snow;
    let snow_target = f32::from(snowy);
    if previous.is_none() {
        state.wetness = target;
        state.snow_cover = snow_target;
    }
    state.wetness +=
        (target - state.wetness) * (1.0 - (-dt / if rainy { 12.0 } else { 90.0 }).exp());
    state.snow_cover +=
        (snow_target - state.snow_cover) * (1.0 - (-dt / if snowy { 60.0 } else { 300.0 }).exp());
    if previous.is_some() && time.elapsed_secs_f64() - state.last_material_update < 0.2 {
        return;
    }
    state.last_material_update = time.elapsed_secs_f64();
    let wetness = state.wetness;
    let snow = state.snow_cover;
    for mut appearance in &mut instances {
        // Buildings and other native WORLD shapes retain their appearance.
        // Snow belongs to terrain and opted-in train exteriors, including when
        // WORLD uses GPU instancing rather than StandardMaterial.
        let next = Vec2::new(wetness, 0.0);
        if appearance.surface_weather != next {
            appearance.surface_weather = next;
        }
    }
    state.bases.retain(|id, _| materials.contains(*id));
    let mut updated = HashSet::new();
    for (standard, snow_source) in &outdoor {
        let Some(handle) = standard.map(|s| &s.0).or_else(|| snow_source.map(|s| &s.0)) else {
            continue;
        };
        if !updated.insert(handle.id()) {
            continue;
        }
        if state.bases.contains_key(&handle.id())
            && (wetness - state.last_applied_wetness).abs() < 1e-4
        {
            continue;
        }
        let Some(mut material) = materials.get_mut(handle) else {
            continue;
        };
        if material.unlit || !matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))
        {
            continue;
        }
        let (color, roughness, reflectance) = *state.bases.entry(handle.id()).or_insert((
            material.base_color,
            material.perceptual_roughness,
            material.reflectance,
        ));
        let mut color = color.to_linear();
        color.red *= 1.0 - 0.16 * wetness;
        color.green *= 1.0 - 0.16 * wetness;
        color.blue *= 1.0 - 0.16 * wetness;
        material.base_color = Color::LinearRgba(color);
        material.perceptual_roughness = roughness * (1.0 - 0.45 * wetness);
        material.reflectance = reflectance + (0.65 - reflectance).max(0.0) * wetness;
    }
    state.last_applied_wetness = wetness;
    let next_weather = Vec2::new(wetness, snow);
    let changed: Vec<_> = viewer_terrain
        .iter()
        .filter_map(|(id, m)| (m.surface_weather != next_weather).then_some(id))
        .collect();
    for id in changed {
        if let Some(mut material) = viewer_terrain.get_mut(id) {
            material.surface_weather = next_weather;
        }
    }
    let changed: Vec<_> = scenery
        .iter()
        .filter_map(|(id, m)| {
            (matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))
                && m.params.shader_kind != 4.0
                && ((m.params.wetness - wetness).abs() > 1e-4 || m.params.snow_cover.abs() > 1e-4))
                .then_some(id)
        })
        .collect();
    for id in changed {
        if let Some(mut material) = scenery.get_mut(id) {
            material.params.wetness = wetness;
            material.params.snow_cover = 0.0;
        }
    }
    let changed: Vec<_> = terrain
        .iter()
        .filter(|(_, m)| {
            (m.params._pad0 - wetness).abs() > 1e-4 || (m.params._pad1 - snow).abs() > 1e-4
        })
        .map(|(id, _)| id)
        .collect();
    for id in changed {
        if let Some(mut material) = terrain.get_mut(id) {
            material.params._pad0 = wetness;
            material.params._pad1 = snow;
        }
    }
}

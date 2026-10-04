//! Rain changes existing outdoor materials gradually, without cloning textures.
use bevy::{asset::AssetId, prelude::*};
use std::collections::{HashMap, HashSet};
#[derive(Resource, Default)]
pub struct WetSurfaces {
    pub wetness: f32,
    last_clock: Option<f64>,
    last_material_update: f64,
    last_applied_wetness: f32,
    bases: HashMap<AssetId<StandardMaterial>, (Color, f32, f32)>,
}
pub fn update(
    time: Res<Time>,
    live: Res<crate::live::LiveDrive>,
    weather: Res<crate::player_launch::ActivePlayerContent>,
    mut state: ResMut<WetSurfaces>,
    outdoor: Query<&MeshMaterial3d<StandardMaterial>, Without<crate::cab_view::CabInteriorMarker>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain: ResMut<Assets<openrailsrs_bevy_scenery::OrTerrainMaterial>>,
) {
    let clock = live.session.time_s();
    let previous = state.last_clock.replace(clock);
    let dt = (clock - previous.unwrap_or(clock)).clamp(0.0, 1.0) as f32;
    let rainy = weather.weather == crate::player_launch::PlayerWeather::Rain;
    let target = f32::from(rainy);
    if previous.is_none() {
        state.wetness = target;
    }
    state.wetness +=
        (target - state.wetness) * (1.0 - (-dt / if rainy { 12.0 } else { 90.0 }).exp());
    if previous.is_some() && time.elapsed_secs_f64() - state.last_material_update < 0.2 {
        return;
    }
    state.last_material_update = time.elapsed_secs_f64();
    let wetness = state.wetness;
    state.bases.retain(|id, _| materials.contains(*id));
    let mut updated = HashSet::new();
    for handle in &outdoor {
        if !updated.insert(handle.0.id()) {
            continue;
        }
        if state.bases.contains_key(&handle.0.id())
            && (wetness - state.last_applied_wetness).abs() < 1e-4
        {
            continue;
        }
        let Some(mut material) = materials.get_mut(&handle.0) else {
            continue;
        };
        if material.unlit || !matches!(material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_))
        {
            continue;
        }
        let (color, roughness, reflectance) = *state.bases.entry(handle.0.id()).or_insert((
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
    let changed: Vec<_> = terrain
        .iter()
        .filter(|(_, m)| (m.params._pad0 - wetness).abs() > 1e-4)
        .map(|(id, _)| id)
        .collect();
    for id in changed {
        if let Some(mut material) = terrain.get_mut(id) {
            material.params._pad0 = wetness;
        }
    }
}

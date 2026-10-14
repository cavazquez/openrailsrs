//! One allocation for cosmetic features. Visibility of the player's headlights
//! has priority; weather/vegetation and secondary lights share the remainder.
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder};
use bevy::prelude::*;
use serde::Serialize;
#[derive(Resource, Clone, Serialize, PartialEq)]
pub struct RenderBudget {
    pub train_lights: usize,
    pub passenger_lights: usize,
    pub station_lights: usize,
    pub particles: usize,
    pub vegetation_divisor: usize,
    pub upload_mib: usize,
    pub memory_pressure: bool,
}
impl Default for RenderBudget {
    fn default() -> Self {
        Self::allocate(40, 8192, 0, false)
    }
}
impl RenderBudget {
    pub fn allocate(slots: usize, particles: usize, quality: u8, pressure: bool) -> Self {
        let level = quality.max(u8::from(pressure)).min(3);
        let available = (slots.clamp(4, 64) / (1usize << level)).max(4);
        let train_lights = available.min(8);
        let passenger_lights = available.saturating_sub(train_lights).min(8);
        let station_lights = available
            .saturating_sub(train_lights + passenger_lights)
            .min(24);
        Self {
            train_lights,
            passenger_lights,
            station_lights,
            particles: (particles / (1usize << level)).clamp(128, 8192),
            vegetation_divisor: 1usize << level,
            upload_mib: if pressure { 2 } else { 8 },
            memory_pressure: pressure,
        }
    }
}
pub fn update(
    settings: Res<crate::player_settings::PlayerSettings>,
    quality: Res<crate::adaptive_graphics::AdaptiveGraphics>,
    memory: Res<crate::gpu_memory::GraphicsMemory>,
    mut budget: ResMut<RenderBudget>,
    residency: Res<crate::gpu_memory::ResidencyPolicy>,
    mut upload: ResMut<bevy::render::render_asset::RenderAssetBytesPerFrame>,
) {
    let next = RenderBudget::allocate(
        settings.light_budget,
        settings.weather_particle_budget,
        quality.level,
        residency.pressured || memory.pressure(),
    );
    let bytes = Some(next.upload_mib * 1024 * 1024);
    if upload.max_bytes != bytes {
        upload.max_bytes = bytes;
    }
    if *budget != next {
        *budget = next;
    }
}
pub fn shadows(
    budget: Res<RenderBudget>,
    settings: Res<crate::player_settings::PlayerSettings>,
    mut lights: Query<
        (&mut CascadeShadowConfig, &mut DirectionalLight),
        With<crate::route_lighting::RouteSunLight>,
    >,
) {
    if !budget.is_changed() && !settings.is_changed() {
        return;
    }
    let reach =
        (crate::scene::shadow_distance_m() / budget.vegetation_divisor.max(1) as f32).max(60.);
    for (mut cascade, mut light) in &mut lights {
        *cascade = CascadeShadowConfigBuilder {
            num_cascades: if budget.vegetation_divisor >= 4 { 2 } else { 3 },
            maximum_distance: reach,
            first_cascade_far_bound: reach.min(24.),
            overlap_proportion: 0.2,
            ..default()
        }
        .build();
        light.shadow_maps_enabled = settings.shadows;
        // Conservative biases suppress foliage acne without detaching wheel shadows.
        light.shadow_depth_bias = 0.02;
        light.shadow_normal_bias = 0.5;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_share_one_budget_and_preserve_priority_under_pressure() {
        for slots in 4..=64 {
            for level in 0..=3 {
                let b = RenderBudget::allocate(slots, 8192, level, false);
                assert!(b.train_lights + b.passenger_lights + b.station_lights <= slots);
                assert!(b.train_lights >= 4);
            }
        }
        let regular = RenderBudget::default();
        let pressure = RenderBudget::allocate(40, 8192, 0, true);
        assert!(pressure.particles < regular.particles);
        assert!(pressure.station_lights < regular.station_lights);
    }
}

//! Shared sky palette and distance fog (#123 / #39).

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

pub const SKY_COLOR_ZENITH: Color = Color::srgb(0.38, 0.62, 0.92);
pub const SKY_COLOR_HORIZON: Color = Color::srgb(0.72, 0.84, 0.96);
pub const NIGHT_ZENITH: Color = Color::srgb(0.04, 0.06, 0.14);
pub const NIGHT_HORIZON: Color = Color::srgb(0.08, 0.10, 0.18);

/// Horizon + zenith colours for day/night.
pub fn sky_palette(night: bool) -> (Color, Color) {
    if night {
        (NIGHT_HORIZON, NIGHT_ZENITH)
    } else {
        (SKY_COLOR_HORIZON, SKY_COLOR_ZENITH)
    }
}

/// Window clear colour for the current time of day.
pub fn sky_clear_color(night: bool) -> Color {
    sky_palette(night).0
}

/// Atmospheric fog keyed to an explicit visibility distance (metres).
pub fn distance_fog(visibility_m: f32, night: bool) -> DistanceFog {
    let visibility = visibility_m.clamp(200.0, 16_000.0);
    let (horizon, _) = sky_palette(night);

    if night {
        return DistanceFog {
            color: horizon.with_alpha(0.75),
            directional_light_color: Color::NONE,
            directional_light_exponent: 8.0,
            falloff: FogFalloff::from_visibility_contrast(visibility * 0.55, 0.02),
        };
    }

    let extinction = Color::srgba(0.62, 0.70, 0.80, 0.88);
    let inscattering = horizon.with_alpha(0.96);
    DistanceFog {
        color: horizon.with_alpha(0.94),
        directional_light_color: Color::srgba(1.0, 0.95, 0.86, 0.28),
        directional_light_exponent: 10.0,
        falloff: FogFalloff::from_visibility_colors(visibility, extinction, inscattering),
    }
}

/// Fog visibility derived from a tile-grid span (render3d lab policy).
pub fn fog_visibility_from_tile_span(side_m: f32, tile_count: usize) -> f32 {
    let tile_span = side_m * (tile_count as f32).sqrt().max(1.0);
    (tile_span * 4.5).clamp(2_000.0, 6_500.0)
}

/// Spawn an inverted sky sphere (interior faces) at the origin.
pub fn spawn_sky_dome(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    radius: f32,
    night: bool,
) {
    let (horizon, zenith) = sky_palette(night);
    let mesh = meshes.add(Sphere::new(radius.clamp(500.0, 150_000.0)));
    let material = materials.add(StandardMaterial {
        base_color: horizon,
        emissive: LinearRgba::from(zenith) * if night { 0.35 } else { 0.85 },
        perceptual_roughness: 1.0,
        metallic: 0.0,
        double_sided: true,
        unlit: true,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(Vec3::ZERO).with_scale(Vec3::splat(-1.0)),
        Name::new("sky-dome"),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_color_is_light_blue_by_day() {
        let c = sky_clear_color(false);
        assert!(c.to_srgba().blue > 0.9);
    }

    #[test]
    fn day_fog_uses_atmospheric_falloff() {
        let fog = distance_fog(2_000.0, false);
        assert!(matches!(fog.falloff, FogFalloff::Atmospheric { .. }));
    }

    #[test]
    fn tile_span_fog_scales() {
        let small = fog_visibility_from_tile_span(512.0, 1);
        let large = fog_visibility_from_tile_span(2048.0, 4);
        assert!(large > small);
    }
}

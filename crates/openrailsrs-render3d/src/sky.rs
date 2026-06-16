//! Cielo procedural y niebla atmosferica (paridad OR cielo nublado / horizonte suave).

use bevy::light::NotShadowCaster;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::SceneExtent;

const SKY_COLOR_ZENITH: Color = Color::srgb(0.38, 0.62, 0.92);
const SKY_COLOR_HORIZON: Color = Color::srgb(0.72, 0.84, 0.96);
const NIGHT_ZENITH: Color = Color::srgb(0.04, 0.06, 0.14);
const NIGHT_HORIZON: Color = Color::srgb(0.08, 0.10, 0.18);

/// Horizonte + zenit segun hora del dia.
pub fn sky_palette(night: bool) -> (Color, Color) {
    if night {
        (NIGHT_HORIZON, NIGHT_ZENITH)
    } else {
        (SKY_COLOR_HORIZON, SKY_COLOR_ZENITH)
    }
}

/// Color de fondo de ventana segun hora.
pub fn sky_clear_color(night: bool) -> Color {
    sky_palette(night).0
}

/// Niebla de camara: visibilidad ~3-6 km, tono del horizonte (dia) o exponencial suave (noche).
pub fn scene_distance_fog(extent: &SceneExtent, tile_count: usize, night: bool) -> DistanceFog {
    let tile_span = extent.side_m * (tile_count as f32).sqrt().max(1.0);
    let visibility = (tile_span * 4.5).clamp(2_000.0, 6_500.0);
    let (horizon, _zenith) = sky_palette(night);

    if night {
        return DistanceFog {
            color: horizon.with_alpha(0.75),
            directional_light_color: Color::NONE,
            directional_light_exponent: 8.0,
            falloff: FogFalloff::from_visibility_contrast(visibility * 0.55, 0.02),
        };
    }

    // Extincion ligeramente gris-azul; inscattering del horizonte (cielo OR nublado).
    let extinction = Color::srgba(0.62, 0.70, 0.80, 0.88);
    let inscattering = horizon.with_alpha(0.96);
    DistanceFog {
        color: horizon.with_alpha(0.94),
        directional_light_color: Color::srgba(1.0, 0.95, 0.86, 0.28),
        directional_light_exponent: 10.0,
        falloff: FogFalloff::from_visibility_colors(visibility, extinction, inscattering),
    }
}

/// Domo de cielo centrado en el origen del tile (escala negativa = cara interior).
pub fn spawn_scene_sky(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    extent: &SceneExtent,
    tile_count: usize,
    night: bool,
) {
    let min_radius = 8_000.0;
    let tile_span = extent.side_m * (tile_count as f32).sqrt().max(1.0);
    let radius = (tile_span * 12.0).clamp(min_radius, 150_000.0);

    let (horizon, zenith) = sky_palette(night);

    let mesh = meshes.add(Sphere::new(radius));
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
        NotShadowCaster,
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
    fn clear_color_is_dark_by_night() {
        let c = sky_clear_color(true);
        assert!(c.to_srgba().red < 0.15);
    }

    #[test]
    fn day_fog_uses_atmospheric_falloff() {
        let extent = SceneExtent { side_m: 2048.0 };
        let fog = scene_distance_fog(&extent, 1, false);
        assert!(matches!(fog.falloff, FogFalloff::Atmospheric { .. }));
    }

    #[test]
    fn fog_visibility_scales_with_tile_span() {
        let small = scene_distance_fog(&SceneExtent { side_m: 512.0 }, 1, false);
        let large = scene_distance_fog(&SceneExtent { side_m: 2048.0 }, 4, false);
        let vis = |f: &DistanceFog| match f.falloff {
            FogFalloff::Atmospheric { extinction, .. } => extinction.x,
            FogFalloff::Exponential { density } => density,
            _ => 0.0,
        };
        assert!(vis(&large) <= vis(&small) || vis(&large) > 0.0);
    }
}

//! Cielo procedural y niebla atmosferica (paridad OR cielo nublado / horizonte suave).
//!
//! Thin adapter over [`openrailsrs_bevy_scenery::atmosphere`] (#123).

use bevy::light::NotShadowCaster;
use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    distance_fog, fog_visibility_from_tile_span, sky_clear_color as shared_sky_clear_color,
    sky_palette,
};

use crate::SceneExtent;

/// Color de fondo de ventana segun hora.
pub fn sky_clear_color(night: bool) -> Color {
    shared_sky_clear_color(night)
}

/// Niebla de camara: visibilidad ~3-6 km, tono del horizonte (dia) o exponencial suave (noche).
pub fn scene_distance_fog(extent: &SceneExtent, tile_count: usize, night: bool) -> DistanceFog {
    distance_fog(
        fog_visibility_from_tile_span(extent.side_m, tile_count),
        night,
    )
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
    use bevy::pbr::FogFalloff;

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
        // Larger span → larger visibility → smaller extinction.
        assert!(vis(&large) < vis(&small));
    }
}

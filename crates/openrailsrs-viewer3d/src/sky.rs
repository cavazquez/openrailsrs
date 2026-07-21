//! Procedural sky dome and atmospheric distance fog (#8 / #39).

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::launch::view_radius_m;
use crate::track::TrackScene;
use crate::viewer_log;
use crate::world::RouteFocus;

const SKY_COLOR_ZENITH: Color = Color::srgb(0.38, 0.62, 0.92);
const SKY_COLOR_HORIZON: Color = Color::srgb(0.72, 0.84, 0.96);
const NIGHT_HORIZON: Color = Color::srgb(0.08, 0.10, 0.18);

/// Toggle atmospheric fog with `F`. Disabled by default (opt-in with `F`, #39).
#[derive(Resource, Clone, Debug)]
pub struct FogState {
    pub enabled: bool,
}

impl Default for FogState {
    fn default() -> Self {
        Self { enabled: false }
    }
}

impl FogState {
    pub fn hud_label(&self) -> &'static str {
        if self.enabled { "on" } else { "off" }
    }
}

/// Spawn an inverted sky sphere centred on the route.
pub fn spawn_sky_dome(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    _focus: Res<RouteFocus>,
) {
    // Tile-lab puede tener grafo vacío (bbox 0 → radio mínimo 500 m), pero la
    // cámara orbita a ~2.6 km: el domo debe envolverla siempre.
    let min_radius = if mode.is_tile_lab() { 20_000.0 } else { 500.0 };
    let radius = (scene.bounds.orbit_distance() * 3.0).clamp(min_radius, 150_000.0);
    let mesh = meshes.add(Sphere::new(radius));
    let material = materials.add(StandardMaterial {
        base_color: SKY_COLOR_HORIZON,
        emissive: LinearRgba::from(SKY_COLOR_ZENITH) * 0.85,
        perceptual_roughness: 1.0,
        metallic: 0.0,
        double_sided: true,
        unlit: true,
        cull_mode: None,
        // Domo is the backdrop; fog must not wash it out.
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

/// Horizon tint used as the window clear colour.
pub fn sky_clear_color() -> Color {
    SKY_COLOR_HORIZON
}

/// Atmospheric fog keyed to viewing distance (parity with `render3d::scene_distance_fog`).
///
/// Visibility matches [`view_radius_m`] so streamed content fades into haze instead of
/// hard-cutting before the fog horizon.
pub fn viewer_distance_fog(visibility_m: f32, night: bool) -> DistanceFog {
    let visibility = visibility_m.clamp(200.0, 16_000.0);
    if night {
        return DistanceFog {
            color: NIGHT_HORIZON.with_alpha(0.75),
            directional_light_color: Color::NONE,
            directional_light_exponent: 8.0,
            falloff: FogFalloff::from_visibility_contrast(visibility * 0.55, 0.02),
        };
    }

    let extinction = Color::srgba(0.62, 0.70, 0.80, 0.88);
    let inscattering = SKY_COLOR_HORIZON.with_alpha(0.96);
    DistanceFog {
        color: SKY_COLOR_HORIZON.with_alpha(0.94),
        directional_light_color: Color::srgba(1.0, 0.95, 0.86, 0.28),
        directional_light_exponent: 10.0,
        falloff: FogFalloff::from_visibility_colors(visibility, extinction, inscattering),
    }
}

/// Fog for the playable camera using the process viewing-distance policy (#39 / #30).
pub fn camera_distance_fog() -> DistanceFog {
    viewer_distance_fog(view_radius_m(), false)
}

/// Toggle fog with `F` — inserts/removes [`DistanceFog`] on the playable camera.
pub fn toggle_distance_fog(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<FogState>,
    mut cameras: Query<(Entity, Option<&mut DistanceFog>), With<Camera3d>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    // F1/F2 are camera modes; plain F is fog. Ignore if a function-key chord is held.
    if keys.pressed(KeyCode::F1) || keys.pressed(KeyCode::F2) {
        return;
    }
    state.enabled = !state.enabled;
    for (entity, fog) in &mut cameras {
        if state.enabled {
            if fog.is_none() {
                commands.entity(entity).insert(camera_distance_fog());
            }
        } else if fog.is_some() {
            commands.entity(entity).remove::<DistanceFog>();
        }
    }
    viewer_log!(
        "openrailsrs-viewer3d: fog {}",
        if state.enabled { "on" } else { "off" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_track::TrackGraph;

    use crate::track::TrackScene;

    #[test]
    fn clear_color_is_light_blue() {
        let c = sky_clear_color();
        assert!(c.to_srgba().blue > 0.9);
    }

    #[test]
    fn sky_radius_scales_with_route() {
        let scene = TrackScene::from_graph(TrackGraph::new());
        let radius = (scene.bounds.orbit_distance() * 3.0).clamp(500.0, 150_000.0);
        assert!(radius >= 500.0);
    }

    #[test]
    fn day_fog_uses_atmospheric_falloff() {
        let fog = viewer_distance_fog(2000.0, false);
        assert!(matches!(fog.falloff, FogFalloff::Atmospheric { .. }));
    }

    #[test]
    fn night_fog_uses_non_atmospheric_falloff() {
        let fog = viewer_distance_fog(2000.0, true);
        assert!(
            !matches!(fog.falloff, FogFalloff::Atmospheric { .. }),
            "night fog should use contrast/exponential path"
        );
    }

    #[test]
    fn fog_visibility_scales_with_viewing_distance() {
        let near = viewer_distance_fog(500.0, false);
        let far = viewer_distance_fog(4000.0, false);
        let dens = |f: &DistanceFog| match &f.falloff {
            FogFalloff::Atmospheric { extinction, .. } => extinction.x,
            other => panic!("expected atmospheric fog, got {other:?}"),
        };
        // Longer visibility → lower extinction density.
        assert!(dens(&far) < dens(&near));
    }
}

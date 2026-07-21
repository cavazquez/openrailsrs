//! Procedural sky dome and atmospheric distance fog (#8 / #39 / #123).

use bevy::pbr::DistanceFog;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    distance_fog, sky_clear_color as shared_sky_clear_color,
    spawn_sky_dome as shared_spawn_sky_dome,
};

use crate::launch::view_radius_m;
use crate::track::TrackScene;
use crate::viewer_log;
use crate::world::RouteFocus;

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
    shared_spawn_sky_dome(&mut commands, &mut meshes, &mut materials, radius, false);
}

/// Horizon tint used as the window clear colour.
pub fn sky_clear_color() -> Color {
    shared_sky_clear_color(false)
}

/// Atmospheric fog keyed to viewing distance (parity with `render3d::scene_distance_fog`).
pub fn viewer_distance_fog(visibility_m: f32, night: bool) -> DistanceFog {
    distance_fog(visibility_m, night)
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
    use bevy::pbr::FogFalloff;
    use openrailsrs_bevy_scenery::sky_palette;
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

    #[test]
    fn shared_palette_matches_clear_color() {
        let (horizon, _) = sky_palette(false);
        assert_eq!(horizon.to_srgba().blue, sky_clear_color().to_srgba().blue);
    }
}

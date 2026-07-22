//! Procedural sky dome and atmospheric distance fog (#8 / #39 / #123).

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    distance_fog, sky_clear_color as shared_sky_clear_color,
    spawn_sky_dome as shared_spawn_sky_dome,
};

use crate::launch::view_radius_m;
use crate::track::TrackScene;
use crate::viewer_log;
use crate::world::RouteFocus;

/// Atmospheric fog on the playable camera (#39). Enabled by default; toggle with `F`.
#[derive(Resource, Clone, Debug)]
pub struct FogState {
    pub enabled: bool,
}

impl Default for FogState {
    fn default() -> Self {
        Self { enabled: true }
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
///
/// Visibility tracks [`view_radius_m`] so scenery is not culled before the fog horizon.
pub fn camera_distance_fog() -> DistanceFog {
    // Slightly beyond the view window so tiles at the rim fade instead of popping.
    let visibility = (view_radius_m() * 1.15).max(view_radius_m());
    viewer_distance_fog(visibility, false)
}

/// Keep [`DistanceFog`] on the camera with zero density.
///
/// Bevy's mesh view bind group layout includes fog binding 13 only when the
/// component is present. Removing it while pipelines still carry
/// `MeshPipelineKey::DISTANCE_FOG` triggers a wgpu validation crash on toggle.
pub fn disabled_distance_fog() -> DistanceFog {
    DistanceFog {
        color: Color::srgba(0.0, 0.0, 0.0, 0.0),
        directional_light_color: Color::NONE,
        directional_light_exponent: 1.0,
        falloff: FogFalloff::Exponential { density: 0.0 },
    }
}

/// Apply [`FogState::enabled`] without adding/removing the fog component.
pub fn sync_camera_fog(fog: &mut DistanceFog, enabled: bool) {
    *fog = if enabled {
        camera_distance_fog()
    } else {
        disabled_distance_fog()
    };
}

/// Toggle fog with `F` — zeros falloff instead of removing [`DistanceFog`].
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
        match fog {
            Some(mut fog) => sync_camera_fog(&mut fog, state.enabled),
            None => {
                // Camera missing the component (e.g. after hot-reload) — always insert
                // so the DISTANCE_FOG view layout stays stable across toggles.
                let mut fog = camera_distance_fog();
                sync_camera_fog(&mut fog, state.enabled);
                commands.entity(entity).insert(fog);
            }
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

    #[test]
    fn fog_enabled_by_default() {
        assert!(FogState::default().enabled);
    }

    #[test]
    fn camera_fog_visibility_not_below_view_radius() {
        let fog = camera_distance_fog();
        let view = view_radius_m();
        // Atmospheric extinction decreases as visibility grows; ensure we use ≥ view radius.
        let at_view = viewer_distance_fog(view, false);
        let dens = |f: &DistanceFog| match &f.falloff {
            FogFalloff::Atmospheric { extinction, .. } => extinction.x,
            other => panic!("expected atmospheric fog, got {other:?}"),
        };
        assert!(
            dens(&fog) <= dens(&at_view) + 1e-6,
            "camera fog must not be denser than view-radius fog (would hide tiles early)"
        );
    }

    #[test]
    fn toggle_off_keeps_distance_fog_component_with_zero_density() {
        let mut fog = camera_distance_fog();
        sync_camera_fog(&mut fog, false);
        match fog.falloff {
            FogFalloff::Exponential { density } => assert_eq!(density, 0.0),
            other => panic!("disabled fog must stay Exponential(0), got {other:?}"),
        }
        sync_camera_fog(&mut fog, true);
        assert!(
            matches!(fog.falloff, FogFalloff::Atmospheric { .. }),
            "re-enable must restore atmospheric camera fog"
        );
    }
}

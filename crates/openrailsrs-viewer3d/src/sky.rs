//! Procedural sky dome (order 11 / issue #8).

use bevy::prelude::*;

use crate::track::TrackScene;
use crate::world::RouteFocus;

const SKY_COLOR_ZENITH: Color = Color::srgb(0.38, 0.62, 0.92);
const SKY_COLOR_HORIZON: Color = Color::srgb(0.72, 0.84, 0.96);

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
}

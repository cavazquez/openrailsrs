//! Near-field lamps placed on original compatible scenery installations.
use bevy::{camera::primitives::Aabb, prelude::*};
#[derive(Resource, Default)]
pub struct StationLighting {
    lights: Vec<Entity>,
    last_clock: Option<f64>,
    pub candidates: usize,
    pub active: usize,
    last_origin: Vec3,
    last_eye: Option<Vec3>,
    last_night: bool,
    last_parts: usize,
}
#[derive(Component)]
pub struct StationLamp;
impl StationLighting {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"candidates":self.candidates,"active":self.active,"light_budget":24,"volumetric_budget":4,"shadow_maps":false})
    }
}
pub fn compatible(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let native = name.rsplit('/').next().is_some_and(|n| {
        n.starts_with("doc_")
            && (n.contains("lamp") || n.contains("light"))
            && !n.contains("disc")
            && !n.contains("disk")
            && !n.contains("signal")
    });
    native
        || [
            "lamppost",
            "lamp_post",
            "platformlight",
            "platform_light",
            "platform_lamp",
            "streetlight",
            "street_light",
            "stationlamp",
        ]
        .iter()
        .any(|k| name.contains(k))
}
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    live: Res<crate::live::LiveDrive>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    mut state: ResMut<StationLighting>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<crate::mirrors::MirrorCamera>)>,
    parts: Query<(
        &crate::world::WorldSceneryLod,
        &GlobalTransform,
        &Aabb,
        &Mesh3d,
        &InheritedVisibility,
    )>,
    mut lamps: Query<(&mut Transform, &mut PointLight, &mut Visibility), With<StationLamp>>,
    quality: Option<Res<crate::adaptive_graphics::AdaptiveGraphics>>,
    budget: Option<Res<crate::render_budget::RenderBudget>>,
    meshes: Res<Assets<Mesh>>,
) {
    let clock = live.session.time_s();
    let night = sun.as_ref().is_some_and(|s| s.direction.y < 0.05);
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();
    state.lights.retain(|e| lamps.get(*e).is_ok());
    let part_count = parts.iter().len();
    if state.last_parts == part_count
        && state.last_clock.is_some_and(|t| (t - clock).abs() < 0.5)
        && state
            .last_eye
            .is_some_and(|p| p.distance_squared(eye) < 64.)
        && state.last_origin == origin.shift
        && state.last_night == night
    {
        return;
    }
    state.last_parts = part_count;
    state.last_clock = Some(clock);
    state.last_eye = Some(eye);
    state.last_origin = origin.shift;
    state.last_night = night;
    let mut points: Vec<Vec3> = Vec::new();
    for (part, tf, bounds, mesh, visible) in &parts {
        if !visible.get() || !compatible(&part.shape_path.to_string_lossy()) {
            continue;
        }
        let top = tf.transform_point(Vec3::from(bounds.center) + Vec3::Y * bounds.half_extents.y);
        if top.distance_squared(eye) > 180_f32.powi(2) {
            continue;
        }
        let mut positions = Vec::new();
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) = meshes
            .get(&mesh.0)
            .and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION))
        {
            for v in vertices {
                let point = tf.transform_point(Vec3::from_array(*v));
                if point.y >= top.y - 0.3
                    && point.distance_squared(eye) <= 180_f32.powi(2)
                    && positions
                        .iter()
                        .all(|p: &Vec3| p.xz().distance_squared(point.xz()) >= 1.)
                {
                    positions.push(point);
                    if positions.len() >= 64 {
                        break;
                    }
                }
            }
        }
        if positions.is_empty() {
            positions.push(top);
        }
        for top in positions {
            if let Some(p) = points
                .iter_mut()
                .find(|p| p.xz().distance_squared(top.xz()) < 1.)
            {
                if top.y > p.y {
                    *p = top;
                }
            } else {
                points.push(top);
            }
        }
    }
    state.candidates = points.len();
    points.sort_by(|a, b| a.distance_squared(eye).total_cmp(&b.distance_squared(eye)));
    let limit = quality
        .as_ref()
        .map_or(24, |q| if q.level >= 2 { 8 } else { 24 });
    points.truncate(
        budget
            .as_ref()
            .map_or(limit, |b| limit.min(b.station_lights)),
    );
    for i in state.lights.len()..points.len() {
        let id = commands
            .spawn((
                StationLamp,
                Name::new(format!("station-lamp-{i}")),
                PointLight {
                    color: Color::srgb(1., 0.88, 0.68),
                    intensity: 1800.,
                    range: 18.,
                    radius: 0.12,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::IDENTITY,
                Visibility::Hidden,
            ))
            .id();
        state.lights.push(id);
    }
    for (i, e) in state.lights.iter().enumerate() {
        if night && i < 4 && i < points.len() {
            commands.entity(*e).insert(bevy::light::VolumetricLight);
        } else {
            commands.entity(*e).remove::<bevy::light::VolumetricLight>();
        }
        if let Ok((mut tf, mut lamp, mut visible)) = lamps.get_mut(*e) {
            if let Some(point) = points.get(i) {
                tf.translation = *point - Vec3::Y * 0.1;
                lamp.intensity = 1800.;
                *visible = if night {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            } else {
                *visible = Visibility::Hidden;
            }
        }
    }
    state.active = if night { points.len() } else { 0 };
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_matching_does_not_turn_signals_windows_or_trees_into_station_lamps() {
        assert!(compatible("Chiltern/platformLight.s"));
        for n in ["signal.s", "WINDOW.s", "forest.s", "office.s"] {
            assert!(!compatible(n));
        }
    }
}

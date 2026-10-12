//! Declared coach window lights use the real train supply, with a render budget.
use crate::{
    live::LiveDrive, rolling_stock::ConsistCarIndex, rolling_stock_anim::TrainCarTrackOffset,
};
use bevy::prelude::*;
#[derive(Resource, Default)]
pub struct TrainPresentation {
    pub vehicles: Vec<openrailsrs_scenarios::presentation::VehiclePresentation>,
    spawned: std::collections::HashSet<Entity>,
}
#[derive(Component)]
pub struct PassengerLight {
    pub car: Entity,
    pub vehicle: usize,
    pub definition: openrailsrs_scenarios::presentation::PassengerLampDef,
}
pub fn prepare(mut state: ResMut<TrainPresentation>, live: Option<Res<LiveDrive>>) {
    *state = default();
    if let Some(live) = live
        && let Ok(s) = openrailsrs_scenarios::load_scenario(live.scenario_path())
    {
        state.vehicles = s.train.presentation;
    }
}
pub fn supplied(session: &openrailsrs_sim::LiveDriveSession, vehicle: usize) -> bool {
    vehicle < session.formation.coupled_count
        && session
            .state
            .power_supply
            .cars
            .iter()
            .any(|c| c.vehicle < session.formation.coupled_count && c.supply.train_supply)
}
pub fn update(
    mut commands: Commands,
    mut state: ResMut<TrainPresentation>,
    live: Res<LiveDrive>,
    settings: Res<crate::player_settings::PlayerSettings>,
    quality: Option<Res<crate::adaptive_graphics::AdaptiveGraphics>>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<PassengerLight>)>,
    cars: Query<
        (
            Entity,
            &ConsistCarIndex,
            &TrainCarTrackOffset,
            &GlobalTransform,
        ),
        Without<PassengerLight>,
    >,
    mut lamps: Query<(
        Entity,
        &PassengerLight,
        &mut Transform,
        &mut SpotLight,
        &mut Visibility,
    )>,
) {
    state.spawned.retain(|e| cars.get(*e).is_ok());
    for (entity, index, offset, _) in &cars {
        if offset.track_index != 0 || state.spawned.contains(&entity) {
            continue;
        }
        let Some(config) = state.vehicles.iter().find(|v| v.vehicle == index.0) else {
            continue;
        };
        for definition in &config.passenger_lamps {
            commands.spawn((
                PassengerLight {
                    car: entity,
                    vehicle: index.0,
                    definition: definition.clone(),
                },
                SpotLight {
                    intensity: 0.,
                    color: Color::srgb(1., 0.78, 0.5),
                    range: definition.range_m,
                    outer_angle: 1.25,
                    inner_angle: 0.75,
                    radius: 0.15,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::default(),
                Visibility::Hidden,
                Name::new("passenger-window-light"),
            ));
        }
        state.spawned.insert(entity);
    }
    let camera = camera
        .single()
        .map(|t| t.translation())
        .unwrap_or(Vec3::ZERO);
    let hour = (live.clock_time_s() / 3600.).rem_euclid(24.);
    let dark = sun
        .as_ref()
        .map_or(!(6. ..18.).contains(&hour), |s| s.direction.y < 0.02);
    let mut active = lamps
        .iter()
        .filter_map(|(e, l, _, _, _)| {
            cars.get(l.car)
                .ok()
                .map(|(_, _, _, tf)| (e, tf.translation().distance_squared(camera)))
        })
        .filter(|(_, d)| *d < 80f32.powi(2))
        .collect::<Vec<_>>();
    active.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.to_bits().cmp(&b.0.to_bits())));
    active.truncate(if quality.as_ref().is_some_and(|q| q.level >= 2) {
        4
    } else {
        12
    });
    for (entity, lamp, mut transform, mut light, mut visibility) in &mut lamps {
        let Ok((_, _, _, car)) = cars.get(lamp.car) else {
            commands.entity(entity).despawn();
            continue;
        };
        let enabled = dark
            && supplied(&live.session, lamp.vehicle)
            && active.iter().any(|(e, _)| *e == entity);
        let pos = car.transform_point(Vec3::from_array(lamp.definition.position_m));
        *transform = Transform::from_translation(pos).looking_to(
            car.rotation() * Vec3::from_array(lamp.definition.direction).normalize(),
            Vec3::Y,
        );
        light.intensity = if enabled { lamp.definition.lumens } else { 0. };
        light.range = lamp.definition.range_m;
        light.shadow_maps_enabled =
            enabled && settings.shadows && active.iter().take(4).any(|(e, _)| *e == entity);
        *visibility = if enabled {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn coach_projection_obeys_actual_supply_and_coupled_part() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/player_lab/scenario.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        live.session.step_realtime(2., |_| {});
        assert!(super::supplied(&live.session, 1));
        for c in &mut live.session.state.power_supply.cars {
            c.supply.train_supply = false;
        }
        assert!(!super::supplied(&live.session, 1));
        live.session.state.power_supply.cars[0].supply.train_supply = true;
        live.session.formation.coupled_count = 1;
        assert!(!super::supplied(&live.session, 1));
    }
}

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
        state.vehicles =
            presentation_for_world(&s, &live.session.service_id, &live.traffic.couplings);
    }
}
fn presentation_for_world(
    scenario: &openrailsrs_scenarios::ScenarioFile,
    player: &str,
    couplings: &[openrailsrs_sim::independent_coupling::CouplingReceipt],
) -> Vec<openrailsrs_scenarios::presentation::VehiclePresentation> {
    let source = |id: &str| {
        if id == "Jugador" {
            scenario.train.presentation.as_slice()
        } else {
            scenario
                .extra_trains
                .iter()
                .find(|e| e.id == id)
                .map_or(&[][..], |e| e.presentation.as_slice())
        }
    };
    let mut vehicles = source(player).to_vec();
    for coupling in couplings {
        let offset = coupling.before.formation.cars.len();
        for mut vehicle in source(&coupling.donor).iter().cloned() {
            vehicle.vehicle += offset;
            for connection in &mut vehicle.connections {
                connection.neighbor += offset;
            }
            if let Some(jacobs) = &mut vehicle.jacobs {
                jacobs.neighbor += offset;
            }
            vehicles.push(vehicle);
        }
    }
    vehicles
}

pub fn supplied(session: &openrailsrs_sim::LiveDriveSession, vehicle: usize) -> bool {
    vehicle < session.formation.coupled_count
        && session.state.power_supply.vehicle_supplied(vehicle)
}
pub fn update(
    mut commands: Commands,
    mut state: ResMut<TrainPresentation>,
    live: Res<LiveDrive>,
    settings: Res<crate::player_settings::PlayerSettings>,
    quality: Option<Res<crate::adaptive_graphics::AdaptiveGraphics>>,
    budget: Option<Res<crate::render_budget::RenderBudget>>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    camera: Query<
        &GlobalTransform,
        (
            (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
            Without<PassengerLight>,
        ),
    >,
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
    if let Some(budget) = budget {
        active.truncate(budget.passenger_lights);
    }
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

#[cfg(test)]
mod ownership_tests {
    #[test]
    fn coupled_coaches_keep_their_own_lamps_doors_connections_and_shared_bogie_indices() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("examples/traffic_passing/scenario_coupling.toml");
        let mut scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        let session =
            openrailsrs_sim::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)
                .unwrap();
        scenario.extra_trains[0].presentation = scenario.train.presentation.clone();
        scenario.extra_trains[0].presentation[0].jacobs =
            Some(openrailsrs_scenarios::presentation::JacobsDef {
                neighbor: 1,
                own_support_m: 10.,
                other_support_m: -10.,
                bogie_matrix: "JACOBS".into(),
            });
        let receipt = openrailsrs_sim::independent_coupling::CouplingReceipt {
            donor: "donor".into(),
            before: session.snapshot(),
            donor_before: session.snapshot(),
        };
        let merged = super::presentation_for_world(&scenario, "Jugador", &[receipt]);
        assert_eq!(
            merged.iter().map(|v| v.vehicle).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(merged[2].connections[0].neighbor, 3);
        assert_eq!(merged[2].jacobs.as_ref().unwrap().neighbor, 3);
        assert_eq!(
            merged[3].boarding_doors,
            scenario.train.presentation[1].boarding_doors
        );
        assert_eq!(
            merged[3].passenger_lamps,
            scenario.train.presentation[1].passenger_lamps
        );
        assert_eq!(
            super::presentation_for_world(&scenario, "donor", &[]),
            scenario.extra_trains[0].presentation
        );
    }
}

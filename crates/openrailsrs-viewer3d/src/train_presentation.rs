//! Declared coach window lights use the real train supply, with a render budget.
use crate::{
    live::LiveDrive, rolling_stock::ConsistCarIndex, rolling_stock_anim::TrainCarTrackOffset,
};
use bevy::prelude::*;
#[derive(Resource, Default)]
pub struct TrainPresentation {
    pub vehicles: Vec<openrailsrs_scenarios::presentation::VehiclePresentation>,
    spawned: std::collections::HashSet<Entity>,
    loaded_ids: Vec<Vec<String>>,
    loaded_services: Vec<String>,
    root_ids: std::collections::HashMap<Entity, String>,
    entity_ids: std::collections::HashMap<Entity, String>,
    sources: Vec<(
        String,
        openrailsrs_scenarios::presentation::VehiclePresentation,
    )>,
    current_ids: Vec<String>,
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
        let ids = live.session.vehicle_identities();
        state.loaded_ids = std::iter::once(ids.clone())
            .chain(
                live.traffic
                    .services
                    .iter()
                    .map(|t| t.session.vehicle_identities()),
            )
            .collect();
        state.loaded_services = std::iter::once(live.session.service_id.clone())
            .chain(live.traffic.services.iter().map(|s| s.id.clone()))
            .collect();
        let all_ids: Vec<_> = state.loaded_ids.iter().flatten().cloned().collect();
        let source = |service: &str| {
            if service == "Jugador" {
                s.train.presentation.as_slice()
            } else {
                s.extra_trains
                    .iter()
                    .find(|e| e.id == service)
                    .map_or(&[][..], |e| e.presentation.as_slice())
            }
        };
        for id in &all_ids {
            let Some((service, number)) = id.rsplit_once("/car/") else {
                continue;
            };
            let Ok(number) = number.parse::<usize>() else {
                continue;
            };
            if let Some(definition) = source(service).iter().find(|v| v.vehicle == number) {
                state.sources.push((id.clone(), definition.clone()));
            }
        }
        state.vehicles = definitions_for_ids(&state.sources, &ids);
        state.current_ids = ids;
    }
}
fn definitions_for_ids(
    sources: &[(
        String,
        openrailsrs_scenarios::presentation::VehiclePresentation,
    )],
    ids: &[String],
) -> Vec<openrailsrs_scenarios::presentation::VehiclePresentation> {
    sources
        .iter()
        .filter_map(|(id, source)| {
            let index = ids.iter().position(|v| v == id)?;
            let (service, _) = id.rsplit_once("/car/")?;
            let mut definition = source.clone();
            definition.vehicle = index;
            definition.connections.retain_mut(|c| {
                if let Some(i) = ids
                    .iter()
                    .position(|id| id == &format!("{service}/car/{}", c.neighbor))
                {
                    c.neighbor = i;
                    true
                } else {
                    false
                }
            });
            if let Some(j) = &mut definition.jacobs {
                if let Some(i) = ids
                    .iter()
                    .position(|id| id == &format!("{service}/car/{}", j.neighbor))
                {
                    j.neighbor = i;
                } else {
                    definition.jacobs = None;
                }
            }
            Some(definition)
        })
        .collect()
}

/// Vehicle entity identities are captured once, then indices and centre offsets
/// follow the physical formation rather than the order in which it was loaded.
#[allow(clippy::type_complexity)]
pub fn remap_portions(
    mut commands: Commands,
    mut state: ResMut<TrainPresentation>,
    live: Option<Res<LiveDrive>>,
    mut roots: Query<
        (
            Entity,
            Option<&crate::live::LiveTrainMarker>,
            Option<&mut crate::traffic::TrafficTrainMarker>,
            &mut Visibility,
        ),
        Or<(
            With<crate::live::LiveTrainMarker>,
            With<crate::traffic::TrafficTrainMarker>,
        )>,
    >,
    mut cars: Query<(
        Entity,
        &mut ConsistCarIndex,
        &mut TrainCarTrackOffset,
        Option<&mut crate::live::LiveTrainCar>,
        Option<&ChildOf>,
    )>,
) {
    let Some(live) = live else {
        return;
    };
    let mut root_for_service = std::collections::HashMap::new();
    for (entity, player, traffic, mut visibility) in &mut roots {
        if player.is_none() && traffic.is_none() {
            continue;
        }
        let id = if let Some(id) = state.root_ids.get(&entity) {
            id.clone()
        } else {
            let index = traffic.as_ref().map_or(0, |m| m.0);
            let Some(id) = state.loaded_services.get(index).cloned() else {
                continue;
            };
            state.root_ids.insert(entity, id.clone());
            id
        };
        if let Some(mut marker) = traffic {
            if let Some(index) = live.traffic.services.iter().position(|s| s.id == id) {
                marker.0 = index + 1;
                *visibility = Visibility::Inherited;
            } else {
                *visibility = Visibility::Hidden;
            }
        }
        root_for_service.insert(id, entity);
    }
    let mut location = std::collections::HashMap::new();
    for (track, session) in std::iter::once((0, &live.session)).chain(
        live.traffic
            .services
            .iter()
            .enumerate()
            .map(|(i, s)| (i + 1, &s.session)),
    ) {
        let parent = root_for_service.get(&session.service_id).copied();
        for (i, id) in session.vehicle_identities().into_iter().enumerate() {
            location.insert(
                id,
                (
                    track,
                    i,
                    session.formation.center_offset_m(i) as f32,
                    parent,
                ),
            );
        }
    }
    for (entity, mut index, mut offset, marker, parent) in &mut cars {
        let id = if let Some(id) = state.entity_ids.get(&entity) {
            id.clone()
        } else {
            let Some(id) = state
                .loaded_ids
                .get(offset.track_index)
                .and_then(|v| v.get(index.0))
                .cloned()
            else {
                continue;
            };
            state.entity_ids.insert(entity, id.clone());
            id
        };
        if let Some(&(track, i, center, target)) = location.get(&id) {
            index.0 = i;
            offset.offset_m = center;
            offset.track_index = track;
            if let Some(target) = target
                && parent.is_none_or(|p| p.parent() != target)
            {
                commands.entity(entity).insert(ChildOf(target));
            }
            if track == 0 {
                if let Some(mut marker) = marker {
                    marker.index = i;
                } else {
                    commands
                        .entity(entity)
                        .insert(crate::live::LiveTrainCar { index: i });
                }
            } else if marker.is_some() {
                commands
                    .entity(entity)
                    .remove::<crate::live::LiveTrainCar>();
            }
        }
    }
    let ids = live.session.vehicle_identities();
    if ids != state.current_ids {
        state.vehicles = definitions_for_ids(&state.sources, &ids);
        state.current_ids = ids;
    }
}
#[cfg(test)]
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
        let Ok((_, index, offset, car)) = cars.get(lamp.car) else {
            commands.entity(entity).despawn();
            continue;
        };
        let owner = if offset.track_index == 0 {
            Some(&live.session)
        } else {
            live.traffic
                .services
                .get(offset.track_index - 1)
                .map(|s| &s.session)
        };
        let enabled = dark
            && owner.is_some_and(|session| supplied(session, index.0))
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
            receiver: None,
            donor: "donor".into(),
            before: session.snapshot(),
            donor_before: session.snapshot(),
            end: Default::default(),
            kind: Default::default(),
            departure_s: 0.,
            departed: false,
            parked: true,
            donor_legs: vec![],
            donor_clock_start_s: 0.,
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

#[cfg(test)]
mod portion_entity_tests {
    use super::*;
    use openrailsrs_sim::{CarOperation, sections::CouplingEnd};
    fn app(file: &str) -> (App, Entity, Option<Entity>, Vec<Entity>, Vec<Entity>) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/portion_operations")
            .join(file);
        let live = LiveDrive::from_scenario_path(&path).unwrap();
        let count = live.session.formation.cars.len();
        let foreign = live
            .traffic
            .services
            .first()
            .map(|s| s.session.formation.cars.len());
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(live)
            .init_resource::<TrainPresentation>();
        app.add_systems(Startup, prepare)
            .add_systems(Update, remap_portions);
        let primary = app
            .world_mut()
            .spawn((
                crate::live::LiveTrainMarker,
                Visibility::Inherited,
                Transform::default(),
            ))
            .id();
        let other = foreign.map(|_| {
            app.world_mut()
                .spawn((
                    crate::traffic::TrafficTrainMarker(1),
                    Visibility::Inherited,
                    Transform::default(),
                ))
                .id()
        });
        let mut own = vec![];
        let mut donor = vec![];
        for (parent, track, n) in
            std::iter::once((primary, 0, count)).chain(other.zip(foreign).map(|(p, n)| (p, 1, n)))
        {
            for i in 0..n {
                let e = app
                    .world_mut()
                    .spawn((
                        ConsistCarIndex(i),
                        TrainCarTrackOffset {
                            offset_m: -(i as f32) * 20. - 10.,
                            track_index: track,
                            flipped: false,
                        },
                        ChildOf(parent),
                    ))
                    .id();
                if track == 0 {
                    own.push(e);
                } else {
                    donor.push(e);
                }
            }
        }
        app.update();
        (app, primary, other, own, donor)
    }
    #[test]
    fn car_entities_keep_original_identity_when_front_pickup_changes_indices() {
        let (mut app, primary, _, own, _) = app("scenario.toml");
        {
            let mut live = app.world_mut().resource_mut::<LiveDrive>();
            let s = &mut live.session;
            for i in 1..5 {
                s.operate_car(i, CarOperation::Handbrake).unwrap();
            }
            s.uncouple_after_named(2, "B").unwrap();
            s.uncouple_after_named(0, "C").unwrap();
            s.state.pos_on_edge_m = 100.;
            s.recouple_section("B", CouplingEnd::Front).unwrap();
        }
        app.update();
        assert_eq!(app.world().get::<ConsistCarIndex>(own[0]).unwrap().0, 2);
        assert_eq!(app.world().get::<ConsistCarIndex>(own[3]).unwrap().0, 0);
        assert_eq!(
            app.world()
                .get::<TrainCarTrackOffset>(own[3])
                .unwrap()
                .offset_m,
            -10.
        );
        assert_eq!(
            app.world().get::<ChildOf>(own[3]).unwrap().parent(),
            primary
        );
    }
    #[test]
    fn service_join_and_split_reparent_existing_entities_without_duplicating_models() {
        let (mut app, primary, foreign, _, donor) = app("scenario_services.toml");
        {
            let mut resource = app.world_mut().resource_mut::<LiveDrive>();
            let live = &mut *resource;
            live.traffic.couple_service(&mut live.session, 0).unwrap();
        }
        app.update();
        assert_eq!(
            app.world().get::<ChildOf>(donor[0]).unwrap().parent(),
            primary
        );
        assert_eq!(app.world().get::<ConsistCarIndex>(donor[0]).unwrap().0, 3);
        assert_eq!(
            app.world()
                .get::<TrainCarTrackOffset>(donor[0])
                .unwrap()
                .track_index,
            0
        );
        {
            let mut resource = app.world_mut().resource_mut::<LiveDrive>();
            let live = &mut *resource;
            live.traffic
                .split_service(&mut live.session, "Porción-Sur")
                .unwrap();
        }
        app.update();
        assert_eq!(
            app.world().get::<ChildOf>(donor[0]).unwrap().parent(),
            foreign.unwrap()
        );
        assert_eq!(app.world().get::<ConsistCarIndex>(donor[0]).unwrap().0, 0);
        assert_eq!(
            app.world()
                .get::<TrainCarTrackOffset>(donor[0])
                .unwrap()
                .track_index,
            1
        );
    }
}

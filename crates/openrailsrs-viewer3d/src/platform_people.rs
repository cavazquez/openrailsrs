//! Bounded passengers on declared door/platform corridors. Unknown geometry
//! gets no invented access route; the author's platform side stays authoritative.
use bevy::prelude::*;
use openrailsrs_scenarios::vehicle_systems::DoorSide;
#[derive(Component)]
pub(crate) struct Person {
    ordinal: usize,
}
/// Dynamic figures are never opaque world obstacles for passengers or plumes.
#[derive(Component)]
pub(crate) struct PassengerFigure;
#[derive(Resource, Default, serde::Serialize)]
pub struct PlatformPeople {
    pub visible: usize,
    pub represented_boarding: u32,
    pub represented_alighting: u32,
    pub rejected_paths: u64,
    pub stop: Option<usize>,
}
pub fn install(app: &mut App) {
    app.init_resource::<PlatformPeople>()
        .add_systems(OnEnter(crate::ViewerAppState::Playing), spawn)
        .add_systems(
            PostUpdate,
            update
                .after(bevy::transform::TransformSystems::Propagate)
                .run_if(in_state(crate::ViewerAppState::Playing)),
        )
        .add_systems(OnExit(crate::ViewerAppState::Playing), cleanup);
}
fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: ResMut<PlatformPeople>,
) {
    *state = default();
    let mesh = meshes.add(Capsule3d::new(0.17, 1.1));
    let head = meshes.add(Sphere::new(0.13));
    let skin = materials.add(StandardMaterial {
        base_color: Color::srgb(0.65, 0.43, 0.3),
        perceptual_roughness: 0.9,
        ..default()
    });
    let coats = [
        Color::srgb(0.18, 0.25, 0.45),
        Color::srgb(0.4, 0.16, 0.12),
        Color::srgb(0.28, 0.38, 0.24),
        Color::srgb(0.35, 0.3, 0.2),
    ]
    .map(|base_color| {
        materials.add(StandardMaterial {
            base_color,
            perceptual_roughness: 0.9,
            ..default()
        })
    });
    for ordinal in 0..64 {
        commands
            .spawn((
                Person { ordinal },
                PassengerFigure,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(coats[ordinal % 4].clone()),
                Transform::default(),
                Visibility::Hidden,
                bevy::camera::visibility::RenderLayers::layer(crate::cab_render::LAYER_WORLD),
                Name::new("platform-passenger"),
            ))
            .with_children(|person| {
                person.spawn((
                    PassengerFigure,
                    Mesh3d(head.clone()),
                    MeshMaterial3d(skin.clone()),
                    Transform::from_xyz(0., 0.84, 0.),
                    Visibility::Inherited,
                ));
            });
    }
}
#[allow(clippy::type_complexity)]
fn update(
    live: Option<Res<crate::live::LiveDrive>>,
    presentation: Res<crate::train_presentation::TrainPresentation>,
    mut state: ResMut<PlatformPeople>,
    obstacles: Res<crate::effect_obstacles::EffectObstacles>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    cars: Query<
        (
            &crate::rolling_stock::ConsistCarIndex,
            &crate::rolling_stock_anim::TrainCarTrackOffset,
            &GlobalTransform,
        ),
        Without<Person>,
    >,
    mut people: Query<(&Person, &mut Transform, &mut Visibility)>,
) {
    let _profile = crate::performance::profile_system("platform_people");
    state.visible = 0;
    state.represented_boarding = 0;
    state.represented_alighting = 0;
    let Some(live) = live else {
        return;
    };
    let session = &live.session;
    let stop = session
        .gameplay
        .stop_targets
        .get(session.gameplay.next_stop_idx);
    let enabled = stop.is_some_and(|s| (s.cum_dist_m - session.gameplay_chainage_m()).abs() < 65.)
        && session.velocity_mps() < 0.1;
    let boarding = matches!(
        session.gameplay.phase,
        openrailsrs_sim::ServicePhase::Boarding | openrailsrs_sim::ServicePhase::ReadyToDepart
    );
    let doors = presentation
        .vehicles
        .iter()
        .flat_map(|vehicle| {
            vehicle
                .boarding_doors
                .iter()
                .map(move |door| (vehicle.vehicle, door))
        })
        .filter_map(|(vehicle, door)| {
            let (_, offset, car) = cars
                .iter()
                .find(|(i, o, _)| i.0 == vehicle && o.track_index == 0)?;
            let platform_side = if offset.flipped {
                door.side.flipped()
            } else {
                door.side
            };
            if session
                .platform_door_side()
                .is_some_and(|s| s != DoorSide::Both && s != platform_side)
            {
                return None;
            }
            let side = if door.side == DoorSide::Left { 0 } else { 1 };
            let open = session
                .state
                .vehicle_systems
                .iter()
                .find(|s| s.definition.vehicle == vehicle)
                .is_some_and(|s| s.door_fraction[side] > 0.98);
            // A closed door cannot be assigned boarding slots. Otherwise an
            // unopened opposite side consumes half the group and passengers
            // disappear instead of using the enabled platform access.
            (!boarding || open).then_some((door, car, open))
        })
        .collect::<Vec<_>>();
    let progress = session.gameplay.boarding_progress();
    if let Some(stop) = stop {
        state.represented_boarding = (stop.passengers_on as f64 * progress).floor() as u32;
        state.represented_alighting =
            (stop.passengers_off.min(session.state.passengers) as f64 * progress).floor() as u32;
    }
    state.stop = enabled.then_some(session.gameplay.next_stop_idx);
    for (person, mut tf, mut visibility) in &mut people {
        *visibility = Visibility::Hidden;
        if !enabled || doors.is_empty() {
            continue;
        }
        let (door, car, open) = doors[person.ordinal % doors.len()];
        let side = door.position_m[0].signum();
        let local = Vec3::from_array(door.position_m);
        // Along-platform circulation stays outside the car envelope and track.
        let waiting = local
            + Vec3::new(
                side * 1.8,
                0.,
                (person.ordinal / doors.len()) as f32 * 0.9 - 12.,
            );
        let off = stop.map_or(0, |s| s.passengers_off.min(session.state.passengers)) as usize;
        let on = stop.map_or(0, |s| s.passengers_on) as usize;
        let total = off.saturating_add(on);
        let slots = total.min(64);
        if person.ordinal >= slots {
            continue;
        }
        let alighting_slots = if total == 0 {
            0
        } else {
            (slots * off).div_ceil(total)
        };
        // Keep each prefix proportional to the stop's transferred quantities.
        // Assigning every alighting figure first makes all of them leave while
        // the service counter has only transferred a small fraction of them.
        let alighting = alighting_slot(person.ordinal, slots, alighting_slots);
        let (motion, active) = session.gameplay.passenger_motion(person.ordinal, slots);
        let motion = motion as f32;
        if boarding && (!open || (alighting && motion == 0.) || (!alighting && motion == 1.)) {
            continue;
        }
        let point = if boarding && open && active {
            if alighting {
                local.lerp(waiting, motion)
            } else {
                waiting.lerp(local, motion)
            }
        } else {
            waiting
        };
        let position = car.transform_point(point) + Vec3::Y * 0.8;
        let absolute =
            position + crate::floating_origin::horizontal_shift(origin.shift - obstacles.origin);
        let frame = crate::floating_origin::horizontal_shift(origin.shift - obstacles.origin);
        let access = car.transform_point(local) + Vec3::Y * 0.8 + frame;
        let queue = car.transform_point(waiting) + Vec3::Y * 0.8 + frame;
        // Test the complete authorised doorway corridor at torso height, so a
        // passenger cannot step through an opaque wall between safe endpoints.
        if !safe_corridor(access, queue, &obstacles.solids)
            || !safe_corridor(
                absolute - Vec3::Y * 0.5,
                absolute + Vec3::Y * 0.5,
                &obstacles.solids,
            )
        {
            state.rejected_paths += 1;
            continue;
        }
        tf.translation = position;
        tf.rotation = car.rotation();
        *visibility = Visibility::Inherited;
        state.visible += 1;
    }
}
fn alighting_slot(slot: usize, slots: usize, alighting: usize) -> bool {
    slots > 0 && (slot + 1) * alighting / slots > slot * alighting / slots
}
fn safe_corridor(from: Vec3, to: Vec3, solids: &[crate::effect_obstacles::Solid]) -> bool {
    from.is_finite()
        && to.is_finite()
        && !solids
            .iter()
            .filter(|s| s.owner.is_none())
            .any(|s| s.hits(from, to))
}
fn cleanup(mut commands: Commands, people: Query<Entity, With<Person>>) {
    for e in &people {
        commands.entity(e).despawn();
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn doorway_corridor_rejects_a_wall_between_safe_endpoints() {
        use crate::effect_obstacles::Solid;
        use bevy::prelude::*;
        let wall = Solid {
            min: Vec3::new(1., 0., -1.),
            max: Vec3::new(1.2, 3., 1.),
            owner: None,
        };
        assert!(!super::safe_corridor(
            Vec3::new(0., 1., 0.),
            Vec3::new(2., 1., 0.),
            &[wall]
        ));
        assert!(super::safe_corridor(
            Vec3::new(0., 1., 2.),
            Vec3::new(2., 1., 2.),
            &[wall]
        ));
        let train = Solid {
            owner: Some(Entity::PLACEHOLDER),
            ..wall
        };
        assert!(super::safe_corridor(
            Vec3::new(0., 1., 0.),
            Vec3::new(2., 1., 0.),
            &[train]
        ));
    }
}
#[cfg(test)]
#[test]
fn mixed_transfer_prefixes_follow_the_boarding_and_alighting_counts() {
    for (slots, alighting) in [(42, 12), (64, 31), (30, 0), (12, 12)] {
        for completed in 0..=slots {
            let count = (0..completed)
                .filter(|slot| alighting_slot(*slot, slots, alighting))
                .count();
            assert_eq!(count, completed * alighting / slots);
        }
    }
    assert!(!alighting_slot(0, 0, 0));
    assert_eq!(
        (0..16).filter(|slot| alighting_slot(*slot, 42, 12)).count(),
        4
    );
}

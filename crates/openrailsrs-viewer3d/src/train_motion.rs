//! Body presentation consumes physical vertical suspension when configured.
//! Cosmetic motion remains optional; running gear keeps its track pose.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::rolling_stock_anim::{
    RollingStockPartKind, TrainBogieAnim, TrainCarTrackOffset, TrainExteriorAnimPart,
    TrainKeyedAnim, TrainWheelAnim,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionLevel {
    Off,
    #[default]
    Soft,
    Normal,
    Strong,
}
impl MotionLevel {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Soft,
            Self::Soft => Self::Normal,
            Self::Normal => Self::Strong,
            Self::Strong => Self::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Apagado",
            Self::Soft => "Suave",
            Self::Normal => "Normal",
            Self::Strong => "Marcado",
        }
    }
    pub(crate) fn scale(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Soft => 0.35,
            Self::Normal => 0.65,
            Self::Strong => 1.0,
        }
    }
}

#[derive(Default)]
struct Spring {
    position: f32,
    velocity: f32,
}
impl Spring {
    fn advance(&mut self, target: f32, dt: f32, omega: f32) {
        if dt <= 0.0 {
            return;
        }
        let displacement = self.position - target;
        let rate = self.velocity + omega * displacement;
        let decay = (-omega * dt).exp();
        self.position = target + (displacement + rate * dt) * decay;
        self.velocity = (self.velocity - omega * rate * dt) * decay;
    }
}
#[derive(Default)]
struct CarMotion {
    physical_pose: Transform,
    clock: Option<f64>,
    heading: Option<f32>,
    speed: f32,
    distance: f64,
    vehicle: usize,
    roll: Spring,
    pitch: Spring,
    bounce: Spring,
    pose: Transform,
}

#[derive(Resource, Default)]
pub struct TrainMotion {
    cars: HashMap<Entity, CarMotion>,
    running_gear: HashMap<Entity, Transform>,
    pub level: MotionLevel,
}

/// Procedural running gear shares suspension compensation with authored parts.
#[derive(Component)]
pub struct FallbackRunningGear;

pub fn restore_running_gear(
    motion: Res<TrainMotion>,
    mut parts: Query<(Entity, &mut Transform), With<TrainExteriorAnimPart>>,
) {
    // Cached baseline, rather than inverse matrix round trips, prevents drift
    // when wheel animation deliberately skips unchanged poses during a pause.
    for (entity, mut transform) in &mut parts {
        if let Some(baseline) = motion.running_gear.get(&entity) {
            transform.set_if_neq(*baseline);
        }
    }
}
impl TrainMotion {
    pub fn rigid_transform(&self, entity: Entity, transform: GlobalTransform) -> GlobalTransform {
        self.cars.get(&entity).map_or(transform, |c| {
            GlobalTransform::from(Transform::from_matrix(
                transform.to_matrix() * c.pose.to_matrix().inverse() * c.physical_pose.to_matrix(),
            ))
        })
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"level":self.level,"cars":self.cars.len(),
            "max_roll_deg":self.cars.values().map(|c|c.roll.position.abs().to_degrees()).fold(0.0_f32,f32::max),
            "max_pitch_deg":self.cars.values().map(|c|c.pitch.position.abs().to_degrees()).fold(0.0_f32,f32::max),
            "max_vertical_m":self.cars.values().map(|c|c.bounce.position.abs()).fold(0.0_f32,f32::max),
            "physical_max_body_offset_m":self.cars.values().map(|c|c.physical_pose.translation.length()).fold(0.0_f32,f32::max),
            "physical_max_body_angle_deg":self.cars.values().map(|c|c.physical_pose.rotation.angle_between(Quat::IDENTITY).to_degrees()).fold(0.0_f32,f32::max),
            "running_gear_parts":self.running_gear.len(),
            "wheel_bogie_pose_compensated":true})
    }
}

/// Spatial joints at 18.3 m plus low-amplitude roughness; never advance on render frames.
pub(crate) fn track_response(distance: f64, speed: f32, vehicle: usize) -> Vec3 {
    let running = (speed.abs() / 25.0).clamp(0.0, 1.0);
    let phase = (distance * std::f64::consts::TAU / 18.3) as f32;
    let joint = ((distance + 9.15).rem_euclid(18.3) - 9.15) as f32;
    let impact = (-(joint / 0.30).powi(2)).exp();
    let stiffness = 0.8 + (vehicle % 5) as f32 * 0.06;
    Vec3::new(
        (phase * 0.73).sin() * 0.001,
        phase.sin() * 0.002,
        ((phase * 1.1).sin() * 0.012 + (phase * 0.57).cos() * 0.006 + impact * 0.025) * stiffness,
    ) * running
}
impl TrainMotion {
    pub fn relative_response(&self, entity: Entity, gain: f32, head: bool) -> Transform {
        let Some(c) = self.cars.get(&entity) else {
            return Transform::IDENTITY;
        };
        let r = track_response(c.distance, c.speed, c.vehicle) * self.level.scale() * gain;
        let scale = if head { 0.55 } else { 0.16 };
        suspension_pose(r.y * scale, r.x * scale, r.z * scale)
    }
}

fn suspension_pose(roll: f32, pitch: f32, bounce: f32) -> Transform {
    // Native vehicle +Y is up, +Z is longitudinal. Rotate around the sprung
    // body centre, not a distant route/camera origin.
    let pivot = Vec3::Y * 1.4;
    let rotation = Quat::from_rotation_z(roll) * Quat::from_rotation_x(pitch);
    Transform::from_translation(pivot - rotation * pivot + Vec3::Y * bounce).with_rotation(rotation)
}

/// Physical displacements are measured from the track rest frame. Model
/// coordinates reverse with the stock; authored mesh flips reverse roll/pitch.
fn vertical_pose(
    session: &openrailsrs_sim::LiveDriveSession,
    vehicle: usize,
    flipped: bool,
) -> Option<Transform> {
    let model = session.physics.bogies.get(vehicle)?;
    let params = model.vertical_suspension.as_ref()?;
    let state = session.state.bogies.get(vehicle)?.vertical.as_ref()?;
    if !state.enabled(params) {
        return None;
    }
    let z = state
        .supports
        .each_ref()
        .map(|support| support.wheels.each_ref().map(|w| w.body_z_m));
    let front = (z[0][0] + z[0][1]) / 2.;
    let rear = (z[1][0] + z[1][1]) / 2.;
    let left = (z[0][0] + z[1][0]) / 2.;
    let right = (z[0][1] + z[1][1]) / 2.;
    let direction = if flipped { -1. } else { 1. };
    let roll = ((right - left).atan2(model.gauge_m) * direction) as f32;
    let pitch = ((rear - front).atan2(model.support_offsets_m[0] - model.support_offsets_m[1])
        * direction) as f32;
    Some(suspension_pose(roll, pitch, ((front + rear) / 2.) as f32))
}

fn has_vertical_physics(session: &openrailsrs_sim::LiveDriveSession) -> bool {
    session.physics.bogies.iter().enumerate().any(|(i, model)| {
        model
            .vertical_suspension
            .as_ref()
            .is_some_and(|parameters| {
                session
                    .state
                    .bogies
                    .get(i)
                    .and_then(|bogie| bogie.vertical.as_ref())
                    .is_some_and(|state| state.enabled(parameters))
            })
    })
}

#[allow(clippy::type_complexity)]
pub fn update(
    live: Res<crate::live::LiveDrive>,
    settings: Res<crate::player_settings::PlayerSettings>,
    presentation: Option<Res<crate::train_presentation::TrainPresentation>>,
    mut motion: ResMut<TrainMotion>,
    parents: Query<&Transform, (Without<TrainCarTrackOffset>, Without<TrainExteriorAnimPart>)>,
    mut cars: Query<
        (Entity, &TrainCarTrackOffset, &ChildOf, &mut Transform),
        Without<TrainExteriorAnimPart>,
    >,
    mut running_gear: Query<
        (Entity, &ChildOf, &mut Transform, Option<&TrainKeyedAnim>),
        (
            With<TrainExteriorAnimPart>,
            Or<(
                With<TrainWheelAnim>,
                With<TrainBogieAnim>,
                With<TrainKeyedAnim>,
                With<FallbackRunningGear>,
            )>,
            Without<TrainCarTrackOffset>,
        ),
    >,
) {
    let level = std::env::var("OPENRAILSRS_TRAIN_MOTION")
        .ok()
        .and_then(|v| match v.as_str() {
            "off" => Some(MotionLevel::Off),
            "soft" => Some(MotionLevel::Soft),
            "normal" => Some(MotionLevel::Normal),
            "strong" => Some(MotionLevel::Strong),
            _ => None,
        })
        .unwrap_or(settings.train_motion);
    if level != motion.level {
        motion.cars.clear();
        motion.level = level;
    }
    let scale = level.scale();
    if scale == 0.0
        && !has_vertical_physics(&live.session)
        && live
            .traffic
            .services
            .iter()
            .all(|t| !has_vertical_physics(&t.session))
        && presentation
            .as_ref()
            .is_none_or(|p| p.vehicles.iter().all(|v| v.suspension.is_none()))
        && live
            .session
            .state
            .vehicle_systems
            .iter()
            .all(|s| s.definition.crosswind.is_none() && s.definition.tilt.is_none())
        && live.traffic.services.iter().all(|t| {
            t.session
                .state
                .vehicle_systems
                .iter()
                .all(|s| s.definition.crosswind.is_none() && s.definition.tilt.is_none())
        })
    {
        motion.cars.clear();
        motion.running_gear.clear();
        return;
    }
    let mut active = Vec::new();
    for (entity, offset, parent, mut local) in &mut cars {
        let Some(session) = (if offset.track_index == 0 {
            Some(&live.session)
        } else {
            live.traffic
                .services
                .get(offset.track_index - 1)
                .map(|s| &s.session)
        }) else {
            continue;
        };
        let Ok(parent) = parents.get(parent.parent()) else {
            continue;
        };
        let world = *parent * *local;
        let direction = world.rotation * Vec3::NEG_Z;
        let heading = direction.x.atan2(direction.z);
        let clock = session.time_s();
        let index = session
            .formation
            .cars
            .iter()
            .enumerate()
            .min_by(|(a, _), (b, _)| {
                (session.formation.center_offset_m(*a) - f64::from(offset.offset_m))
                    .abs()
                    .total_cmp(
                        &(session.formation.center_offset_m(*b) - f64::from(offset.offset_m)).abs(),
                    )
            })
            .map_or(0, |(i, _)| i);
        let speed = if index >= session.formation.coupled_count {
            0.0
        } else {
            session
                .state
                .vehicles
                .get(index)
                .map_or(session.state.velocity_mps, |v| v.velocity_mps) as f32
        };
        let speed = speed
            * if session.driver_direction <= 0.25 {
                -1.0
            } else {
                1.0
            };
        let base_mass = session.formation.cars[index].mass_kg;
        let passenger_mass = if index < session.formation.coupled_count {
            session.state.extra_mass_kg / session.formation.coupled_count.max(1) as f64
        } else {
            0.
        };
        let mass = openrailsrs_sim::consumable_mass::vehicle_mass_kg(
            &session.physics,
            &session.state,
            index,
            base_mass,
        ) + passenger_mass;
        let suspension = presentation
            .as_ref()
            .filter(|_| offset.track_index == 0)
            .and_then(|p| p.vehicles.iter().find(|v| v.vehicle == index))
            .and_then(|v| v.suspension.as_ref());
        let vertical = vertical_pose(session, index, offset.flipped);
        let depression = if vertical.is_some() {
            0.
        } else {
            suspension.map_or(0., |s| -s.depression(mass) as f32)
        };
        let omega = suspension.map_or(7., |s| {
            (s.natural_frequency(mass) * s.damping_ratio).clamp(1., 30.) as f32
        });
        let car = motion.cars.entry(entity).or_default();
        if car.clock.is_none() {
            car.bounce.position = depression;
            car.pose = suspension_pose(0., 0., depression);
        }
        if car
            .clock
            .is_some_and(|previous| clock < previous || clock - previous > 1.0)
        {
            *car = CarMotion::default();
            car.bounce.position = depression;
            car.pose = suspension_pose(0., 0., depression);
        }
        let dt = (clock - car.clock.unwrap_or(clock)).max(0.0) as f32;
        if !live.paused && dt > 0.0 && (scale > 0.0 || suspension.is_some()) {
            let acceleration = ((speed - car.speed) / dt).clamp(-2.0, 2.0);
            let angle = heading - car.heading.unwrap_or(heading);
            let change = (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let lateral = (speed * change / dt).clamp(-1.8, 1.8);
            let distance = session.presentation_chainage_for_car(
                f64::from(offset.offset_m),
                f64::from(offset.offset_m),
                0.0,
            );
            let response = if vertical.is_some() {
                Vec3::ZERO
            } else {
                track_response(distance, speed, index)
            };
            car.distance = distance;
            car.vehicle = index;
            let roll = (-lateral * 0.014 + response.y) * scale;
            let pitch = (-acceleration * 0.006 + response.x) * scale;
            let load_response = suspension.map_or(1., |s| {
                (s.empty_mass_kg / mass.max(s.empty_mass_kg)).sqrt() as f32
            });
            let bounce = depression + response.z * scale * load_response;
            car.roll.advance(roll, dt, 4.0);
            car.pitch.advance(pitch, dt, 4.5);
            car.bounce.advance(bounce, dt, omega);
            car.pose = suspension_pose(car.roll.position, car.pitch.position, car.bounce.position);
        }
        car.clock = Some(clock);
        car.heading = Some(heading);
        car.speed = speed;
        let physical_roll = session
            .state
            .vehicle_systems
            .iter()
            .find(|c| c.definition.vehicle == index)
            .map_or(0., |c| c.crosswind.angle_rad - c.tilt.angle_rad)
            as f32
            * if offset.flipped { -1. } else { 1. };
        car.physical_pose =
            suspension_pose(physical_roll, 0., 0.) * vertical.unwrap_or(Transform::IDENTITY);
        // Rebuild from spring coordinates every frame; never compound a physical
        // angle onto the previous render pose during pause or a camera switch.
        car.pose = car.physical_pose
            * suspension_pose(car.roll.position, car.pitch.position, car.bounce.position);
        *local = *local * car.pose;
        active.push(entity);
    }
    // Part animation wrote these transforms earlier in this frame. Cancel
    // suspension only, preserving each wheel angle and bogie track yaw.
    let mut active_parts = Vec::new();
    for (entity, parent, mut transform, keyed) in &mut running_gear {
        if keyed.is_some_and(|k| {
            !matches!(
                k.kind,
                RollingStockPartKind::Pantograph | RollingStockPartKind::RunningGear
            )
        }) {
            continue;
        }
        motion.running_gear.insert(entity, *transform);
        active_parts.push(entity);
        if let Some(car) = motion.cars.get(&parent.parent()) {
            let inverse = car.pose.to_matrix().inverse();
            *transform = Transform::from_matrix(inverse * transform.to_matrix());
        }
    }
    motion.cars.retain(|entity, _| active.contains(entity));
    motion
        .running_gear
        .retain(|entity, _| active_parts.contains(entity));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn driving_physical_suspension_reaches_ecs_mesh_with_cosmetic_motion_off() {
        use bevy::ecs::system::RunSystemOnce;
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/physics_lab/scenario_vertical.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        assert_eq!(live.session.velocity_mps(), 0.);
        live.session.driver_direction = 1.;
        live.session.driver_throttle = 0.8;
        live.session.driver_brake = 0.;
        for _ in 0..400 {
            live.session.step_realtime(0.05, |_| {});
        }
        assert!(live.session.state.odometer_m > 1.);
        let physical = vertical_pose(&live.session, 0, false).unwrap();
        assert!(physical.translation.length() > 1e-6);
        live.paused = true;
        let offset = live.session.formation.center_offset_m(0) as f32;
        let mut app = App::new();
        app.insert_resource(live)
            .insert_resource(crate::player_settings::PlayerSettings {
                train_motion: MotionLevel::Off,
                ..default()
            })
            .init_resource::<TrainMotion>();
        let root = app.world_mut().spawn(Transform::IDENTITY).id();
        let car = app
            .world_mut()
            .spawn((
                Transform::IDENTITY,
                ChildOf(root),
                TrainCarTrackOffset {
                    offset_m: offset,
                    track_index: 0,
                    flipped: false,
                },
            ))
            .id();
        let gear_baseline = Transform::from_xyz(0.4, 0.5, -3.)
            .with_rotation(Quat::from_rotation_y(0.7))
            .with_scale(Vec3::new(20., 3.6, 3.));
        let wheel = app
            .world_mut()
            .spawn((
                gear_baseline,
                ChildOf(car),
                TrainExteriorAnimPart,
                FallbackRunningGear,
            ))
            .id();
        app.world_mut().run_system_once(update).unwrap();
        assert_eq!(*app.world().get::<Transform>(car).unwrap(), physical);
        assert_eq!(app.world().resource::<TrainMotion>().cars.len(), 1);
        let wheel_world =
            physical.to_matrix() * app.world().get::<Transform>(wheel).unwrap().to_matrix();
        for (actual, expected) in wheel_world
            .to_cols_array()
            .iter()
            .zip(gear_baseline.to_matrix().to_cols_array())
        {
            assert!((actual - expected).abs() < 1e-4);
        }
        app.world_mut()
            .run_system_once(restore_running_gear)
            .unwrap();
        app.world_mut()
            .resource_mut::<crate::live::LiveDrive>()
            .session
            .set_vertical_irregularities_enabled(false);
        *app.world_mut().get_mut::<Transform>(car).unwrap() = Transform::IDENTITY;
        app.world_mut().run_system_once(update).unwrap();
        assert!(app.world().resource::<TrainMotion>().cars.is_empty());
        assert_eq!(
            *app.world().get::<Transform>(car).unwrap(),
            Transform::IDENTITY
        );
        assert_eq!(*app.world().get::<Transform>(wheel).unwrap(), gear_baseline);
    }

    #[test]
    fn physical_quarter_car_motion_reaches_the_mesh_and_respects_disable_and_flip() {
        use openrailsrs_train::bogie_geometry::VerticalSuspension;
        let path = crate::test_harness::smoke_scenario_path();
        let mut live = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        let parameters = VerticalSuspension {
            enabled: true,
            sprung_mass_fraction: 0.9,
            spring_n_m: 1e6,
            damper_ns_m: 1e5,
            contact_spring_n_m: 1e7,
            contact_damper_ns_m: 1e5,
            travel_m: 0.1,
            bump_stop_n_m: 1e7,
            sections: vec![],
        };
        let mut state = openrailsrs_sim::track_irregularity::VerticalVehicleState::new(&parameters);
        for support in &mut state.supports {
            support.wheels[0].body_z_m = 0.;
            support.wheels[1].body_z_m = 0.02;
        }
        live.session.physics.bogies[0].vertical_suspension = Some(parameters);
        live.session.state.bogies[0].vertical = Some(state);
        let first = vertical_pose(&live.session, 0, false).unwrap();
        let flipped = vertical_pose(&live.session, 0, true).unwrap();
        assert!(
            (first.rotation.angle_between(Quat::IDENTITY) - 0.02_f32.atan2(1.435)).abs() < 1e-5
        );
        assert!(first.rotation.angle_between(flipped.rotation.inverse()) < 1e-5);
        assert_eq!(first, vertical_pose(&live.session, 0, false).unwrap());
        live.session.set_vertical_irregularities_enabled(false);
        assert!(vertical_pose(&live.session, 0, false).is_none());
        live.session.set_vertical_irregularities_enabled(true);
        assert_eq!(first, vertical_pose(&live.session, 0, false).unwrap());
    }

    #[test]
    fn damped_suspension_is_stable_across_frame_rates_and_pause() {
        let mut one = Spring::default();
        let mut many = Spring::default();
        one.advance(0.02, 2.0, 4.0);
        for _ in 0..120 {
            many.advance(0.02, 2.0 / 120.0, 4.0);
        }
        assert!((one.position - many.position).abs() < 1e-6);
        assert!((one.velocity - many.velocity).abs() < 1e-6);
        let paused = many.position;
        many.advance(-1.0, 0.0, 4.0);
        assert_eq!(many.position, paused);
        many.advance(0.0, 1000.0, 4.0);
        assert!(many.position.is_finite() && many.position.abs() < 1e-6);
    }
    #[test]
    fn parked_tail_does_not_inherit_the_departing_locomotives_suspension() {
        use bevy::ecs::system::RunSystemOnce;
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/yard_training/scenario.toml");
        let mut live = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        for i in [1, 2] {
            live.session
                .operate_car(i, openrailsrs_sim::operations::CarOperation::Handbrake)
                .unwrap();
        }
        live.session.uncouple_after(0).unwrap();
        let offset = live.session.formation.center_offset_m(1) as f32;
        let mut app = App::new();
        app.insert_resource(live)
            .insert_resource(crate::player_settings::PlayerSettings::default())
            .init_resource::<TrainMotion>();
        let root = app.world_mut().spawn(Transform::IDENTITY).id();
        let car = app
            .world_mut()
            .spawn((
                Transform::IDENTITY,
                ChildOf(root),
                TrainCarTrackOffset {
                    offset_m: offset,
                    track_index: 0,
                    flipped: false,
                },
            ))
            .id();
        app.world_mut().run_system_once(update).unwrap();
        for tick in 1..=200 {
            {
                let mut live = app.world_mut().resource_mut::<crate::live::LiveDrive>();
                live.session.state.velocity_mps = 25.0;
                live.session.state.time.0 = f64::from(tick) * 0.05;
            }
            *app.world_mut().get_mut::<Transform>(car).unwrap() = Transform::IDENTITY;
            app.world_mut().run_system_once(update).unwrap();
            assert_eq!(
                *app.world().get::<Transform>(car).unwrap(),
                Transform::IDENTITY
            );
        }
    }

    #[test]
    fn body_motion_keeps_running_gear_in_its_original_world_pose() {
        let rail =
            Transform::from_xyz(100.0, 20.0, -30.0).with_rotation(Quat::from_rotation_y(0.8));
        let body = suspension_pose(0.025, -0.01, 0.03);
        let wheel = Transform::from_xyz(1.0, 0.4, -3.0).with_rotation(Quat::from_rotation_x(1.2));
        let compensated = body.to_matrix().inverse() * wheel.to_matrix();
        let actual = rail.to_matrix() * body.to_matrix() * compensated;
        let expected = rail.to_matrix() * wheel.to_matrix();
        for (a, b) in actual.to_cols_array().iter().zip(expected.to_cols_array()) {
            assert!((a - b).abs() < 1e-4);
        }
    }
    #[test]
    fn paused_animation_skips_do_not_accumulate_suspension_and_off_restores_wheels() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let live =
            crate::live::LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path())
                .unwrap();
        let clock = live.session.time_s();
        app.insert_resource(live)
            .insert_resource(crate::player_settings::PlayerSettings::default())
            .init_resource::<TrainMotion>();
        let root = app.world_mut().spawn(Transform::IDENTITY).id();
        let car = app
            .world_mut()
            .spawn((
                Transform::IDENTITY,
                ChildOf(root),
                TrainCarTrackOffset {
                    offset_m: 0.0,
                    track_index: 0,
                    flipped: false,
                },
            ))
            .id();
        let baseline =
            Transform::from_xyz(0.4, 0.5, -2.0).with_rotation(Quat::from_rotation_x(1.2));
        let wheel = app
            .world_mut()
            .spawn((
                baseline,
                ChildOf(car),
                TrainExteriorAnimPart,
                TrainWheelAnim {
                    matrix_idx: 0,
                    radius_m: 0.46,
                    angle_rad: 1.2,
                    steam_driver: false,
                },
            ))
            .id();
        let pose = suspension_pose(0.02, -0.01, 0.02);
        app.world_mut().resource_mut::<TrainMotion>().cars.insert(
            car,
            CarMotion {
                clock: Some(clock),
                pose,
                roll: Spring {
                    position: 0.02,
                    ..default()
                },
                pitch: Spring {
                    position: -0.01,
                    ..default()
                },
                bounce: Spring {
                    position: 0.02,
                    ..default()
                },
                ..default()
            },
        );
        app.world_mut()
            .resource_mut::<crate::live::LiveDrive>()
            .paused = true;
        let expected = pose.to_matrix().inverse() * baseline.to_matrix();
        for _ in 0..120 {
            app.world_mut()
                .run_system_once(restore_running_gear)
                .unwrap();
            *app.world_mut().get_mut::<Transform>(car).unwrap() = Transform::IDENTITY; // track rest pose written each frame
            app.world_mut().run_system_once(update).unwrap();
            let actual = app.world().get::<Transform>(wheel).unwrap().to_matrix();
            for (a, b) in actual.to_cols_array().iter().zip(expected.to_cols_array()) {
                assert!((a - b).abs() < 1e-5);
            }
        }
        app.world_mut()
            .resource_mut::<crate::player_settings::PlayerSettings>()
            .train_motion = MotionLevel::Off;
        app.world_mut()
            .run_system_once(restore_running_gear)
            .unwrap();
        *app.world_mut().get_mut::<Transform>(car).unwrap() = Transform::IDENTITY;
        app.world_mut().run_system_once(update).unwrap();
        assert_eq!(*app.world().get::<Transform>(wheel).unwrap(), baseline);
        assert_eq!(
            *app.world().get::<Transform>(car).unwrap(),
            Transform::IDENTITY
        );
        assert!(
            app.world()
                .resource::<TrainMotion>()
                .running_gear
                .is_empty()
        );
    }

    #[test]
    fn plan30_spatial_joints_are_vehicle_specific_bounded_and_stop_without_speed() {
        for i in 0..20000 {
            let distance = i as f64 * 0.01;
            let r = track_response(distance, 45., 0);
            assert!(r.x.abs() <= 0.0011 && r.y.abs() <= 0.0021 && r.z.abs() < 0.05);
            assert_eq!(track_response(distance, 0., 0), Vec3::ZERO);
        }
        assert_ne!(track_response(0., 25., 0), track_response(0., 25., 3));
        let e = Entity::PLACEHOLDER;
        let mut motion = TrainMotion {
            level: MotionLevel::Soft,
            ..default()
        };
        motion.cars.insert(
            e,
            CarMotion {
                distance: 0.,
                speed: 25.,
                ..default()
            },
        );
        let head = motion.relative_response(e, 1., true);
        let desk = motion.relative_response(e, 1., false);
        assert!(head.translation.y.abs() > desk.translation.y.abs());
        assert_eq!(motion.relative_response(e, 0., true), Transform::IDENTITY);
        motion.level = MotionLevel::Off;
        assert_eq!(motion.relative_response(e, 1., true), Transform::IDENTITY);
    }
}

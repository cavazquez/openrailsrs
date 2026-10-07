//! Presentation suspension, independent of railway forces. Track pose is the
//! rest frame; wheel/bogie children undo the body pose to remain on the rails.
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
    fn scale(self) -> f32 {
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
    clock: Option<f64>,
    heading: Option<f32>,
    speed: f32,
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
                transform.to_matrix() * c.pose.to_matrix().inverse(),
            ))
        })
    }
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"level":self.level,"cars":self.cars.len(),
            "max_roll_deg":self.cars.values().map(|c|c.roll.position.abs().to_degrees()).fold(0.0_f32,f32::max),
            "max_pitch_deg":self.cars.values().map(|c|c.pitch.position.abs().to_degrees()).fold(0.0_f32,f32::max),
            "max_vertical_m":self.cars.values().map(|c|c.bounce.position.abs()).fold(0.0_f32,f32::max),
            "wheel_bogie_pose_compensated":true})
    }
}

fn suspension_pose(roll: f32, pitch: f32, bounce: f32) -> Transform {
    // Native vehicle +Y is up, +Z is longitudinal. Rotate around the sprung
    // body centre, not a distant route/camera origin.
    let pivot = Vec3::Y * 1.4;
    let rotation = Quat::from_rotation_z(roll) * Quat::from_rotation_x(pitch);
    Transform::from_translation(pivot - rotation * pivot + Vec3::Y * bounce).with_rotation(rotation)
}

#[allow(clippy::type_complexity)]
pub fn update(
    live: Res<crate::live::LiveDrive>,
    settings: Res<crate::player_settings::PlayerSettings>,
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
    if scale == 0.0 {
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
        let speed = session.state.velocity_mps as f32;
        let car = motion.cars.entry(entity).or_default();
        if car.clock.is_some_and(|previous| clock < previous) {
            *car = CarMotion::default();
        }
        let dt = (clock - car.clock.unwrap_or(clock)).max(0.0) as f32;
        if !live.paused && dt > 0.0 && scale > 0.0 {
            let acceleration = ((speed - car.speed) / dt).clamp(-2.0, 2.0);
            let angle = heading - car.heading.unwrap_or(heading);
            let change = (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let lateral = (speed * change / dt).clamp(-1.8, 1.8);
            let distance = (session.state.odometer_m + f64::from(offset.offset_m)).abs();
            let running = (speed.abs() / 25.0).clamp(0.0, 1.0);
            let track_phase = (distance * std::f64::consts::TAU / 18.3) as f32;
            let roll = (-lateral * 0.014 + track_phase.sin() * 0.002 * running) * scale;
            let pitch =
                (-acceleration * 0.006 + (track_phase * 0.73).sin() * 0.001 * running) * scale;
            let bounce = ((track_phase * 1.1).sin() * 0.018 + (track_phase * 0.57).cos() * 0.008)
                * running
                * scale;
            car.roll.advance(roll, dt, 4.0);
            car.pitch.advance(pitch, dt, 4.5);
            car.bounce.advance(bounce, dt, 7.0);
            car.pose = suspension_pose(car.roll.position, car.pitch.position, car.bounce.position);
        }
        car.clock = Some(clock);
        car.heading = Some(heading);
        car.speed = speed;
        *local = *local * car.pose;
        active.push(entity);
    }
    // Part animation wrote these transforms earlier in this frame. Cancel
    // suspension only, preserving each wheel angle and bogie track yaw.
    let mut active_parts = Vec::new();
    for (entity, parent, mut transform, keyed) in &mut running_gear {
        if keyed.is_some_and(|k| k.kind != RollingStockPartKind::Pantograph) {
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
}

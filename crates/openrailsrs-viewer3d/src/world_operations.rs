//! WORLD clocks and fuel pickups are driven by the simulation, never wall time.
use crate::{
    floating_origin::FloatingOrigin,
    live::{LiveDrive, LiveTrainCar},
    rolling_stock_anim::TrainCarTrackOffset,
    world::{RouteFocus, WorldScene},
};
use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::{ShapeAnimBinding, ShapeAnimState};
use openrailsrs_formats::ShapeFile;
use openrailsrs_sim::refill::RefillStation;

#[derive(Component, Clone, Debug)]
pub struct FuelPickupAnimation {
    pub id: String,
    pub operating_frames: f32,
    pub opening_time_s: f64,
}
impl FuelPickupAnimation {
    pub fn new(shape: &ShapeFile, station: &RefillStation) -> Self {
        let count = shape
            .animations
            .first()
            .map_or(0., |a| a.frame_count as f32);
        let operating_frames = if shape
            .matrices
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("ANIMATED_PARTS"))
        {
            count
        } else {
            count.min(1.)
        };
        Self {
            id: station.id.clone(),
            operating_frames,
            opening_time_s: if count > 0. {
                station.opening_time_s * f64::from(operating_frames / count)
            } else {
                0.
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefillTarget {
    pub station: RefillStation,
    pub vehicle: usize,
    pub distance_m: f64,
    pub width_m: f64,
}
#[derive(Resource, Default)]
pub struct RefillTargets(pub Vec<RefillTarget>);

pub fn update_pickup_targets(
    mut live: Option<ResMut<LiveDrive>>,
    world: Res<WorldScene>,
    focus: Res<RouteFocus>,
    origin: Res<FloatingOrigin>,
    cars: Query<(&LiveTrainCar, &TrainCarTrackOffset, &GlobalTransform)>,
    animations: Query<&FuelPickupAnimation>,
    mut targets: ResMut<RefillTargets>,
    mut pickup_cache: Local<Vec<(Vec3, RefillStation)>>,
) {
    targets.0.clear();
    let Some(live) = &mut live else {
        return;
    };
    let session = &mut live.session;
    if world.is_changed() || focus.is_changed() {
        pickup_cache.clear();
        pickup_cache.extend(world.items.iter().filter_map(|object| {
            object
                .pickup
                .as_ref()
                .map(|station| (object.render_position(&focus), station.clone()))
        }));
    }
    for (car, path, pose) in &cars {
        if path.track_index != 0 || car.index >= session.formation.coupled_count {
            continue;
        }
        let Some(intakes) = session.intake_points.get(car.index) else {
            continue;
        };
        for intake in intakes {
            // Car roots retain the authored frame: MSTS +Z is Bevy -Z.
            let intake_pos = pose.transform_point(Vec3::new(0., 0., -intake.offset_m as f32));
            for (position, profile) in pickup_cache.iter() {
                if profile.pickup_type != intake.pickup_type {
                    continue;
                }
                let distance_m =
                    f64::from(intake_pos.distance(
                        *position - crate::floating_origin::horizontal_shift(origin.shift),
                    ));
                if distance_m > 2.5 + intake.width_m / 2. {
                    continue;
                }
                let mut station = profile.clone();
                station.opening_time_s = animations
                    .iter()
                    .find(|a| a.id == station.id)
                    .map_or(0., |a| a.opening_time_s);
                targets.0.push(RefillTarget {
                    station,
                    vehicle: car.index,
                    distance_m,
                    width_m: intake.width_m,
                });
            }
        }
    }
    targets
        .0
        .sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    if session.refilling.as_ref().is_some_and(|op| {
        !op.returning
            && !targets
                .0
                .iter()
                .any(|t| t.station.id == op.station.id && t.vehicle == op.vehicle)
    }) {
        session.cancel_refill();
    }
}

pub fn update_operational_animations(
    live: Option<Res<LiveDrive>>,
    mut pickups: Query<(&FuelPickupAnimation, &mut ShapeAnimState)>,
    mut clocks: Query<&mut ShapeAnimBinding>,
) {
    let Some(live) = live else {
        return;
    };
    for (pickup, mut pose) in &mut pickups {
        let opening = live
            .session
            .refilling
            .as_ref()
            .filter(|op| op.station.id == pickup.id)
            .map_or(0., |op| op.opening as f32);
        pose.key = opening * pickup.operating_frames;
    }
    let clock = live.clock_time_s();
    for mut binding in &mut clocks {
        if binding.clock_time_s.is_some_and(|old| old != clock) {
            binding.clock_time_s = Some(clock);
        }
    }
}

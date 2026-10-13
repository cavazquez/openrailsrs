//! Opt-in dev-tools actions for isolated, reproducible graphical acceptance.
use bevy::prelude::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Cab {
        vehicle: usize,
    },
    Doors {
        vehicle: usize,
        side: openrailsrs_scenarios::vehicle_systems::DoorSide,
        open: bool,
    },
    Collector {
        vehicle: usize,
        index: usize,
    },
    Walk {
        delta: [f32; 3],
    },
    View {
        mode: String,
    },
    Pause {
        value: bool,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TimedAction {
    at_s: f64,
    operation: Action,
}
#[derive(Resource, Default)]
pub struct EquipmentCapture {
    actions: Vec<TimedAction>,
    start: Option<std::time::Instant>,
    next: usize,
    pub results: Vec<serde_json::Value>,
}
pub fn apply(
    mut live: ResMut<crate::live::LiveDrive>,
    mut state: ResMut<EquipmentCapture>,
    mut follow: ResMut<crate::camera::CameraFollowMode>,
    mut passenger: ResMut<crate::camera::PassengerCamState>,
    presentation: Res<crate::train_presentation::TrainPresentation>,
    catalog: Res<crate::camera::PassengerSeatCatalog>,
    progress: Option<Res<crate::world::WorldSpawnProgress>>,
    pipelines: Option<Res<crate::performance::ScenePipelineStatus>>,
) {
    if !crate::capture::capture_enabled()
        || progress.is_some()
        || pipelines.as_ref().is_some_and(|p| !p.ready())
    {
        return;
    }
    if state.start.is_none() {
        let Some(raw) = std::env::var("OPENRAILSRS_CAPTURE_ACTIONS")
            .ok()
            .filter(|s| s.len() <= 16384)
        else {
            return;
        };
        match serde_json::from_str::<Vec<TimedAction>>(&raw) {
            Ok(actions)
                if actions.len() <= 64
                    && actions
                        .iter()
                        .all(|a| a.at_s.is_finite() && (0. ..=600.).contains(&a.at_s))
                    && actions.windows(2).all(|a| a[0].at_s <= a[1].at_s) =>
            {
                state.actions = actions
            }
            _ => {
                crate::viewer_log!("equipment capture: invalid action sequence");
                return;
            }
        }
        state.start = Some(std::time::Instant::now());
    }
    let age = state.start.unwrap().elapsed().as_secs_f64();
    while state.actions.get(state.next).is_some_and(|a| a.at_s <= age) {
        let index = state.next;
        let result = match &state.actions[index].operation {
            Action::Cab { vehicle } => live.session.select_cab(*vehicle),
            Action::Doors {
                vehicle,
                side,
                open,
            } => live.session.command_doors(Some(*vehicle), *side, *open),
            Action::Collector { vehicle, index } => live.session.toggle_collector(*vehicle, *index),
            Action::Walk { delta } => {
                let boxes = presentation
                    .vehicles
                    .iter()
                    .find(|v| v.vehicle == passenger.consist_car)
                    .map_or(&[][..], |v| v.walk_boxes_m.as_slice());
                passenger.head_msts = crate::passenger_walk::constrained_move(
                    passenger.head_msts,
                    Vec3::from_array(*delta),
                    boxes,
                );
                Ok(())
            }
            Action::View { mode } => {
                if mode == "passenger"
                    && let Some((index, views)) = catalog.seat_cars().first()
                    && let Some(view) = views.first()
                {
                    passenger.apply_viewpoint(view, *index);
                }
                *follow = match mode.as_str() {
                    "driver" => crate::camera::CameraFollowMode::DriverCam,
                    "passenger" => crate::camera::CameraFollowMode::PassengerCam,
                    "orbit" => crate::camera::CameraFollowMode::ChaseCam,
                    _ => {
                        state
                            .results
                            .push(serde_json::json!({"index":index,"error":"unknown view"}));
                        state.next += 1;
                        continue;
                    }
                };
                Ok(())
            }
            Action::Pause { value } => {
                live.paused = *value;
                Ok(())
            }
        };
        state
            .results
            .push(serde_json::json!({"index":index,"result":result}));
        state.next += 1;
    }
}

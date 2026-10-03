//! Semaphore arms are driven by signal aspects, never by the WORLD loop clock.
//! Matches OR 1.6.1 Signals.cs and AnimatedPart.SetFrameCycle.

use bevy::prelude::*;
use openrailsrs_bevy_scenery::shapes::ShapeAnimState;
use openrailsrs_formats::{AnimController, ShapeFile, SigCfgFile};
use openrailsrs_track::SignalAspect;

use crate::world::SignalPatch;

#[derive(Component, Clone, Debug)]
pub struct SignalSemaphore {
    signal_id: String,
    tr_item_id: u32,
    targets: [f32; 3],
    duration_s: f32,
    max_frame: f32,
    position: f32,
    initialized: bool,
    fallback_aspect: Option<SignalAspect>,
}

impl SignalSemaphore {
    pub fn for_part(
        shape: &ShapeFile,
        cfg: &SigCfgFile,
        file_name: &str,
        patch: &SignalPatch,
        matrix_idx: usize,
    ) -> Option<Self> {
        let definition = cfg.signal_shape(file_name)?;
        let hierarchy = &shape
            .lod_controls
            .first()?
            .distance_levels
            .first()?
            .hierarchy;
        for unit in &patch.units {
            let Some(sub) = definition
                .sub_objs
                .iter()
                .find(|sub| sub.index == unit.sub_obj)
            else {
                continue;
            };
            let Some(head_matrix) = shape
                .matrices
                .iter()
                .position(|matrix| matrix.name.eq_ignore_ascii_case(&sub.matrix_name))
            else {
                continue;
            };
            let mut ancestor = matrix_idx as i32;
            let mut matches = false;
            for _ in 0..shape.matrices.len() {
                if ancestor < 0 {
                    break;
                }
                if ancestor as usize == head_matrix {
                    matches = true;
                    break;
                }
                ancestor = hierarchy.get(ancestor as usize).copied().unwrap_or(-1);
            }
            if !matches {
                continue;
            }
            let Some(signal_type) = sub
                .signal_type_name
                .as_deref()
                .and_then(|name| cfg.signal_type(name))
            else {
                continue;
            };
            let Some(duration_s) = signal_type.semaphore_animation_time_s else {
                continue;
            };
            let Some(controller) = shape
                .animations
                .first()
                .and_then(|anim| anim.nodes.get(head_matrix))
                .and_then(|node| node.controllers.first())
            else {
                continue;
            };
            let (key_count, max_frame) = match controller {
                AnimController::LinearPos { keys } => (keys.len(), keys.last()?.0),
                AnimController::TcbRot { keys } | AnimController::SlerpRot { keys } => {
                    (keys.len(), keys.last()?.0)
                }
            };
            let max_position = signal_type
                .draw_states
                .iter()
                .filter_map(|state| state.semaphore_pos)
                .fold(0.0_f32, f32::max);
            let reindex = key_count == 2 && max_position == 2.0;
            let targets = std::array::from_fn(|aspect| {
                let position = signal_type
                    .draw_state_for_aspect(aspect as u8)
                    .and_then(|state| state.semaphore_pos)
                    .unwrap_or(0.0);
                if reindex && (position == 1.0 || position == 2.0) {
                    position - 1.0
                } else {
                    position
                }
            });
            return Some(Self {
                signal_id: format!("sig{}", unit.tr_item_id),
                tr_item_id: unit.tr_item_id,
                targets,
                duration_s,
                max_frame,
                position: 0.0,
                initialized: false,
                fallback_aspect: None,
            });
        }
        None
    }

    fn advance(&mut self, aspect: SignalAspect, dt: f32) -> f32 {
        let code = match aspect {
            SignalAspect::Stop => 0,
            SignalAspect::Caution => 1,
            SignalAspect::Clear => 2,
        };
        let target = self.targets[code];
        if !self.initialized || self.duration_s <= 0.0 {
            self.position = target;
            self.initialized = true;
        } else {
            let step = dt.max(0.0) / self.duration_s;
            self.position += (target - self.position).clamp(-step, step);
        }
        (self.max_frame - (self.position - self.max_frame).abs()).clamp(0.0, self.max_frame)
    }
}

pub fn update_signal_semaphores(
    time: Res<Time>,
    scene: Res<crate::track::TrackScene>,
    assets: Res<crate::shapes::RouteAssets>,
    live: Option<Res<crate::live::LiveDrive>>,
    mut signals: Query<(&mut SignalSemaphore, &mut ShapeAnimState)>,
) {
    let dt = live.as_ref().map_or(time.delta_secs(), |drive| {
        if drive.paused {
            0.0
        } else {
            time.delta_secs() * drive.session.speed_mul as f32
        }
    });
    for (mut signal, mut state) in &mut signals {
        let fallback = signal.fallback_aspect.unwrap_or_else(|| {
            let aspect = crate::signal_lamps::aspect_for_tr_item(&assets, signal.tr_item_id);
            signal.fallback_aspect = Some(aspect);
            aspect
        });
        let aspect = crate::signal_lamps::runtime_aspect(
            &scene,
            live.as_deref(),
            &signal.signal_id,
            fallback,
        );
        state.key = signal.advance(aspect, dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn semaphore() -> SignalSemaphore {
        SignalSemaphore {
            signal_id: "sig1".into(),
            tr_item_id: 1,
            targets: [0.0, 0.0, 1.0],
            duration_s: 0.5,
            max_frame: 1.0,
            position: 0.0,
            initialized: false,
            fallback_aspect: None,
        }
    }

    #[test]
    fn authored_signal_config_controls_arm_and_reindexes_two_key_msts_shapes() {
        let cfg = SigCfgFile::from_text(
            r#"
            SignalTypes ( 1 SignalType ( "Home"
                SignalFlags ( SEMAPHORE ) SemaphoreInfo ( 0.5 )
                SignalDrawStates ( 2
                    SignalDrawState ( 0 "Red" SemaphorePos ( 1 ) )
                    SignalDrawState ( 1 "Green" SemaphorePos ( 2 ) ) )
                SignalAspects ( 2 SignalAspect ( STOP "Red" ) SignalAspect ( CLEAR_1 "Green" ) )
            ) )
            SignalShapes ( 1 SignalShape ( "arm.s" "Test arm"
                SignalSubObjs ( 1 SignalSubObj ( 0 "ARM" SigSubType ( SIGNAL_HEAD ) SigSubSType ( "Home" ) ) )
            ) )
        "#,
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-formats/tests/fixtures/minimal.s");
        let mut shape = ShapeFile::from_path(path).unwrap();
        shape.matrices[0].name = "ARM".into();
        shape.animations = vec![openrailsrs_formats::Animation {
            frame_count: 1,
            frame_rate: 30,
            nodes: vec![openrailsrs_formats::AnimNode {
                name: "ARM".into(),
                controllers: vec![AnimController::SlerpRot {
                    keys: vec![(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [0.0, 0.0, 0.5, 0.866])],
                }],
            }],
        }];
        let patch = SignalPatch {
            uid: 1,
            signal_sub_obj: 1,
            units: vec![
                openrailsrs_formats::SignalUnitRef {
                    sub_obj: 999,
                    tr_item_id: 100,
                },
                openrailsrs_formats::SignalUnitRef {
                    sub_obj: 0,
                    tr_item_id: 42,
                },
            ],
        };
        let mut arm = SignalSemaphore::for_part(&shape, &cfg, "arm.s", &patch, 0).unwrap();
        assert_eq!(arm.signal_id, "sig42");
        assert_eq!(arm.duration_s, 0.5);
        assert_eq!(arm.targets, [0.0, 0.0, 1.0]);
        assert_eq!(arm.advance(SignalAspect::Clear, 0.0), 1.0);
        assert_eq!(arm.advance(SignalAspect::Clear, 2.0), 1.0);
    }

    #[test]
    fn unchanged_clear_aspect_stays_at_last_frame_without_looping() {
        let mut arm = semaphore();
        assert_eq!(arm.advance(SignalAspect::Clear, 0.0), 1.0);
        for _ in 0..600 {
            assert_eq!(arm.advance(SignalAspect::Clear, 1.0 / 60.0), 1.0);
        }
    }

    #[test]
    fn aspect_change_moves_once_clamps_and_honours_pause() {
        let mut arm = semaphore();
        assert_eq!(arm.advance(SignalAspect::Stop, 0.0), 0.0);
        assert_eq!(arm.advance(SignalAspect::Clear, 0.125), 0.25);
        assert_eq!(arm.advance(SignalAspect::Clear, 0.0), 0.25);
        assert_eq!(arm.advance(SignalAspect::Clear, 1.0), 1.0);
        assert_eq!(arm.advance(SignalAspect::Stop, 0.25), 0.5);
        assert_eq!(arm.advance(SignalAspect::Stop, 1.0), 0.0);
    }
}

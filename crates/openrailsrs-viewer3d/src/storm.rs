//! World-anchored, bounded lightning geometry and distance-delayed thunder.
//! Storm events use the simulation clock: pause freezes an event, time scaling
//! advances it. The optional wall clock only controls sun/date, not event playback.
use crate::{
    floating_origin::FloatingOrigin,
    live::LiveDrive,
    native_audio::NativeAudio,
    player_launch::{ActivePlayerContent, PlayerWeather},
    player_settings::PlayerSettings,
    precipitation::rain_rng01,
};
use bevy::{
    asset::RenderAssetUsages,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

const BOLT_LIFETIME_S: f64 = 0.32;
const SOUND_SPEED_MPS: f64 = 343.0;
#[derive(Component)]
pub struct LightningBolt;
struct PendingThunder {
    at: f64,
    position: Vec3,
    seed: u32,
}
#[derive(Resource, Default)]
pub struct StormState {
    pub flash: f32,
    pub strikes: u32,
    pub thunders: u32,
    last_clock: Option<f64>,
    next_strike: f64,
    bolt: Option<(Entity, f64)>,
    pending: Vec<PendingThunder>,
    material: Option<Handle<StandardMaterial>>,
}

pub fn flash_at(age: f64) -> f32 {
    if !(0.0..BOLT_LIFETIME_S).contains(&age) {
        return 0.0;
    }
    // A return stroke and a weaker second pulse; no sustained white screen.
    ((-age / 0.032).exp() + 0.65 * (-((age - 0.095) / 0.02).powi(2)).exp()) as f32
}
fn thunder_delay(distance: f32) -> f64 {
    f64::from(distance.max(0.0)) / SOUND_SPEED_MPS
}

fn bolt_segments(seed: u32) -> Vec<(Vec3, Vec3, f32)> {
    let mut segments = vec![];
    let mut previous = Vec3::ZERO;
    for i in 1..=28 {
        let point = Vec3::new(
            (rain_rng01(seed, i) - 0.5) * 85.0,
            i as f32 * 28.0,
            (rain_rng01(seed.wrapping_add(7), i) - 0.5) * 60.0,
        );
        segments.push((previous, point, 0.9));
        if i % 5 == 0 {
            let mut branch = point;
            let side = if rain_rng01(seed, i + 90) > 0.5 {
                1.0
            } else {
                -1.0
            };
            for j in 0..4 {
                let end = branch
                    + Vec3::new(
                        side * (20.0 + rain_rng01(seed, i + j + 130) * 25.0),
                        -20.0 - j as f32 * 3.0,
                        12.0,
                    );
                segments.push((branch, end, 0.45));
                branch = end;
            }
        }
        previous = point;
    }
    segments
}
pub fn lightning_mesh(seed: u32) -> Mesh {
    let mut positions = vec![];
    let mut indices = vec![];
    for (a, b, width) in bolt_segments(seed) {
        // Crossed ribbons live in the world. They neither follow nor rotate
        // with the camera; both sides render without casting camera shadows.
        for axis in [Vec3::X, Vec3::Z] {
            let offset = axis * width;
            let base = positions.len() as u32;
            positions
                .extend([a - offset, a + offset, b - offset, b + offset].map(|v| v.to_array()));
            indices.extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; count])
    .with_inserted_indices(Indices::U32(indices))
}

#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    live: Option<Res<LiveDrive>>,
    content: Res<ActivePlayerContent>,
    settings: Res<PlayerSettings>,
    origin: Res<FloatingOrigin>,
    focus: Res<crate::world::RouteFocus>,
    terrain: Option<Res<crate::terrain::TerrainElevation>>,
    audio: Res<NativeAudio>,
    mut state: ResMut<StormState>,
    camera: Query<&Transform, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(live) = live else { return };
    let clock = live.session.time_s();
    let previous = state.last_clock.replace(clock);
    let active = content.weather == PlayerWeather::Storm;
    if !active || previous.is_some_and(|last| clock < last) {
        if let Some((entity, _)) = state.bolt.take() {
            commands.entity(entity).despawn();
        }
        state.flash = 0.0;
        state.pending.clear();
        state.next_strike = clock + 4.0;
        return;
    }
    if previous.is_none() {
        state.next_strike = clock + 4.0;
    }
    if let Some((entity, at)) = state.bolt {
        state.flash = if settings.lightning {
            flash_at(clock - at)
        } else {
            0.0
        };
        if clock - at >= BOLT_LIFETIME_S || !settings.lightning {
            commands.entity(entity).despawn();
            state.bolt = None;
        }
    } else {
        state.flash = 0.0;
    }
    let Ok(camera) = camera.single() else { return };
    let listener = camera.translation + origin.shift;
    // Remove completed sounds before dispatch; a paused frame cannot retrigger.
    let mut index = 0;
    while index < state.pending.len() {
        if state.pending[index].at > clock {
            index += 1;
            continue;
        }
        let event = state.pending.remove(index);
        if settings.audio_enabled
            && settings.audio_volume > 0.0
            && let Some(engine) = audio.engine.as_ref()
        {
            engine.thunder(openrailsrs_audio::thunder::ThunderEvent {
                seed: event.seed,
                distance_m: listener.distance(event.position),
            });
            state.thunders += 1;
        }
    }
    if live.paused || clock < state.next_strike {
        return;
    }
    state.strikes = state.strikes.wrapping_add(1);
    let seed = state.strikes;
    state.next_strike = clock + 25.0 + f64::from(rain_rng01(seed, 94)) * 30.0;
    // Most events occur in the forward hemisphere; moving the camera afterwards
    // does not move the strike. The ground point lies below the listener.
    let forward = Vec3::new(camera.forward().x, 0., camera.forward().z).normalize_or_zero();
    let side = Vec3::new(-forward.z, 0., forward.x);
    let distance = 650.0 + rain_rng01(seed, 72) * 900.0;
    let mut position =
        listener + forward * distance + side * ((rain_rng01(seed, 73) - 0.5) * 900.0);
    position.y = terrain
        .as_ref()
        .and_then(|t| t.sample_world_y(position.x + focus.center.x, position.z + focus.center.z))
        .map_or(listener.y - 3.0, |height| height - focus.height_origin);
    let delay = thunder_delay(listener.distance(position));
    // An elevated free camera can make sound travel longer than the strike
    // interval. Keep the most recent events and bound the pending queue too.
    if state.pending.len() >= 2 {
        state.pending.remove(0);
    }
    state.pending.push(PendingThunder {
        at: clock + delay,
        position,
        seed,
    });
    if settings.lightning {
        let material = if let Some(handle) = state.material.clone() {
            handle
        } else {
            let handle = materials.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::new(150., 180., 230., 1.),
                // Bevy's unlit branch bypasses emissive entirely. A black PBR
                // base with emission renders the stroke bright in day AND night.
                emissive_exposure_weight: 0.0,
                reflectance: 0.0,
                cull_mode: None,
                fog_enabled: false,
                ..default()
            });
            state.material = Some(handle.clone());
            handle
        };
        let entity = commands
            .spawn((
                LightningBolt,
                Mesh3d(meshes.add(lightning_mesh(seed))),
                MeshMaterial3d(material),
                Transform::from_translation(position - origin.shift),
                NotShadowCaster,
                NotShadowReceiver,
                Name::new("storm-lightning"),
            ))
            .id();
        state.bolt = Some((entity, clock));
        state.flash = flash_at(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn return_strokes_are_brief_and_sound_arrives_after_light() {
        assert!(flash_at(0.0) > 0.9);
        assert!(flash_at(0.095) > flash_at(0.065));
        assert_eq!(flash_at(0.32), 0.0);
        assert_eq!(flash_at(-1.0), 0.0);
        assert!((thunder_delay(1000.0) - 2.91545).abs() < 0.0001);
        assert_eq!(thunder_delay(0.), 0.);
    }
    #[test]
    fn branches_are_deterministic_bounded_world_geometry() {
        let a = bolt_segments(1);
        assert_eq!(a, bolt_segments(1));
        assert_ne!(a, bolt_segments(2));
        assert!(a.len() < 64);
        assert!(
            a.iter()
                .all(|(a, b, w)| a.is_finite() && b.is_finite() && *w > 0.)
        );
        assert!(lightning_mesh(1).count_vertices() < 512);
    }
    #[test]
    fn pause_camera_motion_and_switching_weather_do_not_repeat_or_move_strikes() {
        let scenario = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/scenario.toml");
        let mut live = LiveDrive::from_scenario_path(&scenario).unwrap();
        live.paused = false;
        let clock = live.session.time_s();
        let mut app = App::new();
        app.insert_resource(live)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<NativeAudio>()
            .init_resource::<PlayerSettings>()
            .init_resource::<FloatingOrigin>()
            .insert_resource(crate::world::RouteFocus {
                center: Vec3::ZERO,
                height_origin: 0.,
            })
            .insert_resource(ActivePlayerContent {
                weather: PlayerWeather::Storm,
                ..default()
            })
            .insert_resource(StormState {
                last_clock: Some(clock),
                next_strike: clock,
                pending: vec![
                    PendingThunder {
                        at: clock + 60.,
                        position: Vec3::ZERO,
                        seed: 90,
                    },
                    PendingThunder {
                        at: clock + 90.,
                        position: Vec3::ZERO,
                        seed: 91,
                    },
                ],
                ..default()
            })
            .add_systems(Update, update);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), Transform::from_xyz(0., 3., 0.)))
            .id();
        app.update();
        let state = app.world().resource::<StormState>();
        let entity = state.bolt.unwrap().0;
        assert_eq!(state.strikes, 1);
        assert_eq!(state.pending.len(), 2);
        assert_eq!(state.pending[0].seed, 91);
        assert_eq!(state.pending[1].seed, 1);
        let position = app.world().get::<Transform>(entity).unwrap().translation;
        app.world_mut().resource_mut::<LiveDrive>().paused = true;
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation += Vec3::X * 50.;
        app.update();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation,
            position
        );
        assert_eq!(app.world().resource::<StormState>().strikes, 1);
        app.world_mut().resource_mut::<PlayerSettings>().lightning = false;
        app.update();
        assert!(app.world().get_entity(entity).is_err());
        assert_eq!(app.world().resource::<StormState>().flash, 0.);
        app.world_mut()
            .resource_mut::<ActivePlayerContent>()
            .weather = PlayerWeather::Clear;
        app.update();
        assert!(app.world().resource::<StormState>().pending.is_empty());
    }
}

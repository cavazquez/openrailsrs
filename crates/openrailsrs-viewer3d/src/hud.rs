//! On-screen HUD strip (mirrors the 2D viewer layout).

use bevy::prelude::*;

use crate::camera::{CameraFollowMode, CameraFollowTarget, CameraMode};
use crate::live::LiveDrive;
use crate::precipitation::PrecipitationState;
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::train::{ReplayState, pose_at_time};
use crate::viewer_log;

/// Window / route title shown in the HUD (set from `main` at launch).
#[derive(Resource, Clone, Default)]
pub struct HudTitle(pub String);

/// Smoothed render FPS (exponential moving average).
#[derive(Resource, Clone, Copy, Default)]
pub struct HudFps {
    pub smoothed: f32,
    pub frame_ms: f32,
}

impl HudFps {
    pub fn tick(&mut self, delta_secs: f32) {
        if delta_secs <= 0.0 {
            return;
        }
        let instant_fps = 1.0 / delta_secs;
        self.frame_ms = delta_secs * 1000.0;
        self.smoothed = if self.smoothed < 1.0 {
            instant_fps
        } else {
            self.smoothed * 0.92 + instant_fps * 0.08
        };
    }
}

/// Cached HUD strings to avoid per-frame allocations.
#[derive(Resource, Default)]
pub struct HudCache {
    pub row1: String,
    pub row2: String,
    pub fps_suffix: String,
}

/// Write the FPS suffix into `buf`, returning a reference to the written content.
pub fn write_fps_suffix(buf: &mut String, fps: &HudFps) {
    buf.clear();
    if fps.smoothed < 1.0 {
        return;
    }
    let warn = if fps.smoothed < 30.0 { " !" } else { "" };
    use std::fmt::Write;
    let _ = write!(
        buf,
        "  fps:{:.0} ({:.1}ms){warn}",
        fps.smoothed, fps.frame_ms
    );
}

/// Minimum HUD strip height; panel grows with visible rows (coords, replay, etc.).
pub const HUD_HEIGHT_PX: f32 = 72.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_HINT: f32 = 11.0;

const COL_HUD_BG: Color = Color::srgba(0.02, 0.06, 0.10, 0.88);
const COL_HUD_TEXT: Color = Color::srgb(0.8, 0.8, 0.8);
const COL_HUD_MUTED: Color = Color::srgb(0.27, 0.33, 0.4);
const COL_HUD_ACCENT: Color = Color::srgb(1.0, 0.67, 0.2);
const COL_HUD_TIME: Color = Color::srgb(0.4, 0.87, 1.0);
const COL_HUD_SPD: Color = Color::srgb(1.0, 0.8, 0.27);
const COL_PROGRESS_TRACK: Color = Color::srgb(0.13, 0.2, 0.27);
const COL_PROGRESS_FILL: Color = Color::srgb(0.27, 0.73, 1.0);

/// Pure HUD strings for one frame (unit-tested).
#[derive(Clone, Debug, PartialEq)]
pub struct HudContent {
    pub row1: String,
    pub row2: String,
    pub progress: f32,
    pub trains: String,
    pub controls: String,
    pub status_is_paused: bool,
    /// Show row2 even without an active replay (camera coordinates).
    pub show_row2: bool,
}

pub fn format_world_pos(pos: Vec3) -> String {
    format!("pos {:.0},{:.0},{:.0}", pos.x, pos.y, pos.z)
}

pub fn format_coords_line(camera_pos: Vec3, orbit_focus: Option<Vec3>) -> String {
    let mut line = format_world_pos(camera_pos);
    if let Some(focus) = orbit_focus {
        line.push_str(&format!(
            "    focus {:.0},{:.0},{:.0}",
            focus.x, focus.y, focus.z
        ));
    }
    line
}

pub fn camera_mode_label(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::Orbit => "orbit",
        CameraMode::Fly => "fly",
    }
}

pub fn camera_status_label(mode: CameraMode, follow: CameraFollowMode) -> String {
    if follow == CameraFollowMode::DriverCam {
        "CABINA".into()
    } else {
        format!("cam:{}", camera_mode_label(mode))
    }
}

pub fn follow_display_label(
    follow: CameraFollowMode,
    target: &CameraFollowTarget,
    replay: &ReplayState,
) -> String {
    if follow == CameraFollowMode::Off {
        "off".to_string()
    } else {
        let name = replay
            .tracks
            .get(target.track_index)
            .map(|t| t.label.as_str())
            .unwrap_or("?");
        format!("{}→{}", follow.hud_label(), name)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_hud_content(
    title: &str,
    replay: &ReplayState,
    scene: &TrackScene,
    camera_mode: CameraMode,
    follow: CameraFollowMode,
    follow_target: &CameraFollowTarget,
    precipitation: &PrecipitationState,
    terrain: Option<&TerrainElevation>,
    camera_pos: Vec3,
    orbit_focus: Option<Vec3>,
    world_offset: Vec3,
    focus: &crate::world::RouteFocus,
) -> HudContent {
    let coords = format_coords_line(camera_pos, orbit_focus);
    let rain_hint = "P:rain";
    let controls = if replay.is_active() {
        let train_hint = if replay.tracks.len() > 1 {
            "  [/]:train"
        } else {
            ""
        };
        format!(
            "Space:pause  R:reset  +/-:spd  T:cam  V:driver{train_hint}  {rain_hint}  G:goto  F2:fly  Esc:quit"
        )
    } else {
        format!(
            "Orbit: LMB=rotate  RMB↑↓=zoom  RMB←→=spin  Shift+drag/WASD=pan  wheel=zoom  G:goto  {rain_hint}  F2:fly  Esc:quit"
        )
    };

    let rain_label = precipitation.hud_label();

    if !replay.is_active() {
        return HudContent {
            row1: format!(
                "{title}    cam:{}  rain:{rain_label}",
                camera_mode_label(camera_mode)
            ),
            row2: coords,
            progress: 0.0,
            trains: String::new(),
            controls,
            status_is_paused: false,
            show_row2: true,
        };
    }

    build_hud_replay(
        title,
        replay,
        scene,
        camera_mode,
        follow,
        follow_target,
        rain_label,
        coords,
        controls,
        terrain,
        world_offset,
        focus,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_hud_replay(
    title: &str,
    replay: &ReplayState,
    scene: &TrackScene,
    camera_mode: CameraMode,
    follow: CameraFollowMode,
    follow_target: &CameraFollowTarget,
    rain_label: &str,
    coords: String,
    controls: String,
    terrain: Option<&TerrainElevation>,
    world_offset: Vec3,
    focus: &crate::world::RouteFocus,
) -> HudContent {
    let status = if replay.paused { "PAUSED" } else { "PLAY" };
    let follow_label = if follow == CameraFollowMode::DriverCam {
        "CABINA".to_string()
    } else {
        follow_display_label(follow, follow_target, replay)
    };
    let cam = camera_status_label(camera_mode, follow);

    let mut vel_kmh = 0.0_f64;
    if let Some(track) = replay.tracks.get(follow_target.track_index) {
        if let Some((_, _, v)) = pose_at_time(
            &scene.graph,
            &track.rows,
            replay.t_sim,
            terrain,
            scene,
            world_offset,
            focus,
        ) {
            vel_kmh = v * 3.6;
        }
    } else if let Some(track) = replay.tracks.first() {
        if let Some((_, _, v)) = pose_at_time(
            &scene.graph,
            &track.rows,
            replay.t_sim,
            terrain,
            scene,
            world_offset,
            focus,
        ) {
            vel_kmh = v * 3.6;
        }
    }

    let progress = if replay.max_t > 0.0 {
        (replay.t_sim / replay.max_t).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let progress_pct = (progress * 100.0).round() as u32;

    let row1 =
        format!("{title}    {status}    cam:{cam}  follow:{follow_label}  rain:{rain_label}");
    let row2 = format!(
        "{coords}    t={:.1}s  {:.0} km/h  spd={:.0}x  {progress_pct}%",
        replay.t_sim, vel_kmh, replay.speed
    );

    let mut train_parts = Vec::new();
    for track in &replay.tracks {
        if let Some((_, _, vel)) = pose_at_time(
            &scene.graph,
            &track.rows,
            replay.t_sim,
            terrain,
            scene,
            world_offset,
            focus,
        ) {
            train_parts.push(format!("{} {:.0} km/h", track.label, vel * 3.6));
        }
    }
    let trains = train_parts.join("   ");

    HudContent {
        row1,
        row2,
        progress,
        trains,
        controls,
        status_is_paused: replay.paused,
        show_row2: true,
    }
}

#[derive(Component)]
pub(crate) struct HudRoot;

#[derive(Component)]
pub(crate) struct HudRow1;

#[derive(Component)]
pub(crate) struct HudRow2;

#[derive(Component)]
pub(crate) struct HudProgressBar;

#[derive(Component)]
pub(crate) struct HudProgressFill;

#[derive(Component)]
pub(crate) struct HudTrains;

#[derive(Component)]
pub(crate) struct HudControls;

pub(crate) fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Auto,
                min_height: Val::Px(HUD_HEIGHT_PX),
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(Val::Px(6.0), Val::Px(8.0), Val::Px(5.0), Val::Px(6.0)),
                row_gap: Val::Px(3.0),
                border: UiRect::top(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(COL_HUD_BG),
            BorderColor::all(COL_HUD_ACCENT),
            ZIndex(100),
        ))
        .with_children(|panel| {
            panel.spawn((
                HudRow1,
                Text::new(""),
                TextFont {
                    font_size: FONT_SIZE,
                    ..default()
                },
                TextColor(COL_HUD_TEXT),
            ));
            panel.spawn((
                HudRow2,
                Visibility::Hidden,
                Text::new(""),
                TextFont {
                    font_size: FONT_SIZE,
                    ..default()
                },
                TextColor(COL_HUD_TIME),
            ));
            panel
                .spawn((
                    HudProgressBar,
                    Visibility::Hidden,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(8.0),
                        margin: UiRect::vertical(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(COL_PROGRESS_TRACK),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        HudProgressFill,
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(COL_PROGRESS_FILL),
                    ));
                });
            panel.spawn((
                HudTrains,
                Visibility::Hidden,
                Text::new(""),
                TextFont {
                    font_size: FONT_SIZE,
                    ..default()
                },
                TextColor(COL_HUD_SPD),
            ));
            panel.spawn((
                HudControls,
                Text::new(""),
                TextFont {
                    font_size: FONT_SIZE_HINT,
                    ..default()
                },
                TextColor(COL_HUD_MUTED),
            ));
        });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn build_hud_content_live(
    title: &str,
    live: &LiveDrive,
    camera_mode: CameraMode,
    follow: CameraFollowMode,
    precipitation: &PrecipitationState,
    camera_pos: Vec3,
    orbit_focus: Option<Vec3>,
) -> HudContent {
    let coords = format_coords_line(camera_pos, orbit_focus);
    let rain_label = precipitation.hud_label();
    let status = if live.session.arrived {
        "ARRIVED"
    } else if live.paused {
        "PAUSED"
    } else {
        "LIVE"
    };
    let follow_label = if follow == CameraFollowMode::DriverCam {
        "CABINA".to_string()
    } else if follow == CameraFollowMode::Off {
        "off".to_string()
    } else {
        format!("{}→live", follow.hud_label())
    };
    let cam_label = camera_status_label(camera_mode, follow);
    let vel_kmh = live.session.velocity_mps() * 3.6;
    let limit_kmh = live.session.effective_speed_limit_mps() * 3.6;
    let gp = &live.session.gameplay;
    let dist_line = live
        .session
        .distance_to_next_stop_m()
        .map(|d| format!("  dist:{d:.0}m"))
        .unwrap_or_default();
    let game_line = if live.session.arrived {
        format!("dest:{}  penalty:{:.0}", gp.destination, gp.accrued_penalty)
    } else if let Some(next) = live.session.next_stop_label() {
        let overspeed = if gp.overspeed_active {
            " OVERSPEED"
        } else {
            ""
        };
        format!(
            "next:{next}{dist_line}  penalty:{:.0}{overspeed}",
            gp.accrued_penalty
        )
    } else {
        let overspeed = if gp.overspeed_active {
            " OVERSPEED"
        } else {
            ""
        };
        format!(
            "dest:{}  penalty:{:.0}{overspeed}",
            gp.destination, gp.accrued_penalty
        )
    };
    let controls =
        "↑/↓:thr/brk  I/K:pan  WASD:pan  LMB:rotate  RMB↑↓:zoom  wheel:zoom  ,/.:zoom  Space:emerg  H:horn  C:cab  T:cam  V:driver  P:pause  R:reset  +/-:sim  G:goto  F2:fly  Esc:quit"
            .to_string();
    HudContent {
        row1: format!(
            "{title}    {status}    {cam_label}  follow:{follow_label}  rain:{rain_label}  {game_line}",
        ),
        row2: format!(
            "{coords}    t={:.1}s  {:.0} km/h  lim {:.0} km/h  thr={:.0}%  br={:.0}%  sim={:.1}x",
            live.session.time_s(),
            vel_kmh,
            limit_kmh,
            live.session.driver_throttle * 100.0,
            live.session.driver_brake * 100.0,
            live.session.speed_mul,
        ),
        progress: live.session.route_progress() as f32,
        trains: format!("live {:.0} km/h", vel_kmh),
        controls,
        status_is_paused: live.session.arrived || live.paused,
        show_row2: true,
    }
}

pub(crate) fn tick_hud_fps(time: Res<Time>, mut fps: ResMut<HudFps>) {
    fps.tick(time.delta_secs());
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update_hud(
    title: Res<HudTitle>,
    fps: Res<HudFps>,
    replay: Res<ReplayState>,
    live: Option<Res<LiveDrive>>,
    scene: Res<TrackScene>,
    offset: Res<crate::world::RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    camera_mode: Res<CameraMode>,
    follow: Res<CameraFollowMode>,
    follow_target: Res<CameraFollowTarget>,
    precipitation: Res<PrecipitationState>,
    terrain: Option<Res<TerrainElevation>>,
    camera: Query<(&Transform, Option<&crate::camera::OrbitState>), With<Camera3d>>,
    mut cache: Local<HudCache>,
    mut hud: Query<
        (
            &mut Visibility,
            Option<&mut Text>,
            Option<&mut Node>,
            Option<&HudRow1>,
            Option<&HudRow2>,
            Option<&HudProgressBar>,
            Option<&HudTrains>,
            Option<&HudControls>,
            Option<&HudProgressFill>,
        ),
        Or<(
            With<HudRow1>,
            With<HudRow2>,
            With<HudProgressBar>,
            With<HudTrains>,
            With<HudControls>,
            With<HudProgressFill>,
        )>,
    >,
) {
    let (camera_pos, orbit_focus) = camera
        .iter()
        .next()
        .map(|(transform, orbit)| {
            let focus = orbit.map(|o| o.focus);
            (transform.translation, focus)
        })
        .unwrap_or((Vec3::ZERO, None));
    let orbit_focus = if *camera_mode == CameraMode::Orbit {
        orbit_focus
    } else {
        None
    };

    let content = if let Some(live) = live.as_deref() {
        build_hud_content_live(
            &title.0,
            live,
            *camera_mode,
            *follow,
            &precipitation,
            camera_pos,
            orbit_focus,
        )
    } else {
        build_hud_content(
            &title.0,
            &replay,
            &scene,
            *camera_mode,
            *follow,
            &follow_target,
            &precipitation,
            terrain.as_deref(),
            camera_pos,
            orbit_focus,
            offset.delta,
            &focus,
        )
    };
    write_fps_suffix(&mut cache.row1, &fps);
    let active = replay.is_active() || live.is_some();

    for (mut vis, mut text, mut node, row1, row2, bar, trains, controls, fill) in &mut hud {
        if row1.is_some() {
            if let Some(text) = text.as_mut() {
                text.0.clear();
                text.0.push_str(&content.row1);
                text.0.push_str(&cache.fps_suffix);
            }
        } else if row2.is_some() {
            if let Some(text) = text.as_mut() {
                text.0.clear();
                text.0.push_str(&content.row2);
            }
            *vis = if active || content.show_row2 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        } else if bar.is_some() {
            *vis = if active || live.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        } else if trains.is_some() {
            if let Some(text) = text.as_mut() {
                text.0.clear();
                text.0.push_str(&content.trains);
            }
            *vis = if active && !content.trains.is_empty() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        } else if controls.is_some() {
            if let Some(text) = text.as_mut() {
                text.0.clear();
                text.0.push_str(&content.controls);
            }
        } else if fill.is_some() {
            if let Some(node) = node.as_mut() {
                node.width = Val::Percent(content.progress * 100.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::train::{CsvRow, TrainTrack};
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

    fn line_graph() -> TrackGraph {
        let mut g = TrackGraph::new();
        g.insert_node(Node {
            id: NodeId("a".into()),
            kind: NodeKind::Plain,
            x_m: 0.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_node(Node {
            id: NodeId("b".into()),
            kind: NodeKind::Plain,
            x_m: 100.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_edge(Edge {
            id: EdgeId("e1".into()),
            from: NodeId("a".into()),
            to: NodeId("b".into()),
            length_m: 100.0,
            speed_limit_mps: 30.0,
            grade_percent: 0.0,
        })
        .unwrap();
        g
    }

    fn sample_replay() -> ReplayState {
        ReplayState::new(
            "smoke".into(),
            vec![TrainTrack {
                label: "primary".into(),
                color: Color::WHITE,
                rows: vec![
                    CsvRow {
                        time_s: 0.0,
                        velocity_mps: 10.0,
                        edge_id: "e1".into(),
                        pos_on_edge_m: 0.0,
                    },
                    CsvRow {
                        time_s: 10.0,
                        velocity_mps: 10.0,
                        edge_id: "e1".into(),
                        pos_on_edge_m: 100.0,
                    },
                ],
            }],
        )
    }

    #[test]
    fn static_route_hud_shows_title_and_controls() {
        let scene = TrackScene::from_graph(line_graph());
        let replay = ReplayState::default();
        let content = build_hud_content(
            "test/route",
            &replay,
            &scene,
            CameraMode::Orbit,
            CameraFollowMode::Off,
            &CameraFollowTarget::default(),
            &PrecipitationState {
                enabled: true,
                ..Default::default()
            },
            None,
            Vec3::new(120.0, 35.0, 8.0),
            Some(Vec3::new(5000.0, 0.0, 25.0)),
            Vec3::ZERO,
            &crate::world::RouteFocus {
                center: Vec3::ZERO,
                height_origin: 0.0,
            },
        );
        assert!(content.row1.contains("test/route"));
        assert!(content.row1.contains("cam:orbit"));
        assert!(content.row1.contains("rain:on"));
        assert!(content.controls.contains("P:rain"));
        assert!(content.row2.contains("pos 120,35,8"));
        assert!(content.row2.contains("focus 5000,0,25"));
        assert!(content.show_row2);
        assert!(content.controls.contains("Esc:quit"));
    }

    #[test]
    fn format_coords_omits_focus_in_fly_mode() {
        let line = format_coords_line(Vec3::new(1.0, 2.0, 3.0), None);
        assert_eq!(line, "pos 1,2,3");
    }

    #[test]
    fn replay_hud_includes_time_speed_and_trains() {
        let scene = TrackScene::from_graph(line_graph());
        let mut replay = sample_replay();
        replay.t_sim = 5.0;
        replay.speed = 2.0;
        let content = build_hud_content(
            "smoke",
            &replay,
            &scene,
            CameraMode::Orbit,
            CameraFollowMode::OrbitFollow,
            &CameraFollowTarget::default(),
            &PrecipitationState {
                enabled: true,
                ..Default::default()
            },
            None,
            Vec3::ZERO,
            None,
            Vec3::ZERO,
            &crate::world::RouteFocus {
                center: Vec3::ZERO,
                height_origin: 0.0,
            },
        );
        assert!(content.row1.contains("PLAY"));
        assert!(content.row1.contains("follow:orbit→primary"));
        assert!(content.row1.contains("rain:on"));
        assert!(content.controls.contains("P:rain"));
        assert!(content.row2.contains("pos 0,0,0"));
        assert!(content.row2.contains("t=5.0s"));
        assert!(content.row2.contains("spd=2x"));
        assert!(content.trains.contains("primary"));
        assert!(content.progress > 0.4 && content.progress < 0.6);
    }

    #[test]
    fn fps_suffix_shows_smoothed_rate_and_warns_below_30() {
        let mut fps = HudFps {
            smoothed: 58.0,
            frame_ms: 17.2,
        };
        let mut buf = String::new();
        write_fps_suffix(&mut buf, &fps);
        assert!(buf.contains("fps:58"));
        assert!(buf.contains("17.2ms"));
        fps.smoothed = 22.0;
        write_fps_suffix(&mut buf, &fps);
        assert!(buf.ends_with('!'));
    }

    #[test]
    fn paused_replay_marks_status() {
        let scene = TrackScene::from_graph(line_graph());
        let mut replay = sample_replay();
        replay.paused = true;
        let content = build_hud_content(
            "smoke",
            &replay,
            &scene,
            CameraMode::Fly,
            CameraFollowMode::Off,
            &CameraFollowTarget::default(),
            &PrecipitationState {
                enabled: false,
                ..Default::default()
            },
            None,
            Vec3::ZERO,
            None,
            Vec3::ZERO,
            &crate::world::RouteFocus {
                center: Vec3::ZERO,
                height_origin: 0.0,
            },
        );
        assert!(content.row1.contains("PAUSED"));
        assert!(content.row1.contains("rain:off"));
        assert!(content.row1.contains("cam:fly"));
        assert!(content.status_is_paused);
    }
}

use std::time::Instant;

#[derive(Resource, Default)]
pub struct ProfileLog {
    last: Option<Instant>,
    frames: u32,
}

pub(crate) fn log_profile(fps: Res<HudFps>, mut state: ResMut<ProfileLog>) {
    state.frames += 1;
    let now = Instant::now();
    if let Some(last) = state.last {
        if now.duration_since(last).as_secs() >= 3 {
            viewer_log!(
                "[profile] fps={:.1} frame={:.1}ms over {} frames",
                fps.smoothed,
                fps.frame_ms,
                state.frames,
            );
            state.frames = 0;
            state.last = Some(now);
        }
    } else {
        state.last = Some(now);
    }
}

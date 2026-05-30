//! Live gameplay visuals: stop markers, toasts, arrival summary, driver vignette.

use bevy::prelude::*;
use openrailsrs_sim::path_data::PathData;

use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;
use crate::terrain::TerrainElevation;
use crate::track::TrackScene;
use crate::train::position_on_graph;
use crate::world::{RouteFocus, RouteWorldOffset};

const COL_STOP_NEXT: Color = Color::srgb(0.2, 0.85, 1.0);
const COL_STOP_FUTURE: Color = Color::srgb(0.35, 0.55, 0.75);
const COL_STOP_PASSED: Color = Color::srgb(0.35, 0.38, 0.42);
const COL_DEST: Color = Color::srgb(0.25, 0.95, 0.45);
const COL_TOAST_BG: Color = Color::srgba(0.02, 0.06, 0.12, 0.88);
const COL_TOAST_TEXT: Color = Color::srgb(1.0, 0.85, 0.35);
const COL_ARRIVAL_BG: Color = Color::srgba(0.02, 0.05, 0.1, 0.92);
const COL_ARRIVAL_ACCENT: Color = Color::srgb(0.3, 0.9, 0.5);
const COL_VIGNETTE: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

/// Short-lived HUD toast (stop passed, overspeed, etc.).
#[derive(Resource, Default)]
pub struct GameplayToast {
    pub message: String,
    pub ttl_s: f32,
}

/// Materials for stop marker states (created once at spawn).
#[derive(Resource)]
pub struct GameplayMarkerMaterials {
    pub passed: Handle<StandardMaterial>,
    pub next: Handle<StandardMaterial>,
    pub future: Handle<StandardMaterial>,
}

impl GameplayToast {
    pub fn show(&mut self, message: impl Into<String>, ttl_s: f32) {
        self.message = message.into();
        self.ttl_s = ttl_s;
    }
}

#[derive(Resource, Default)]
pub(crate) struct StopPassTracker {
    next_stop_idx: usize,
}

#[derive(Component)]
pub(crate) struct GameplayMarkerRoot;

#[derive(Component)]
pub(crate) struct GameplayStopMarker {
    stop_index: usize,
}

#[derive(Component)]
pub(crate) struct GameplayDestMarker;

#[derive(Component)]
pub(crate) struct GameplayToastRoot;

#[derive(Component)]
pub(crate) struct GameplayToastText;

#[derive(Component)]
pub(crate) struct ArrivalOverlayRoot;

#[derive(Component)]
pub(crate) struct ArrivalOverlayBody;

#[derive(Component)]
pub(crate) struct StopBillboard {
    pub(crate) world: Vec3,
}

#[derive(Component)]
pub(crate) struct StopBillboardText;

#[derive(Component)]
pub(crate) struct DriverVignetteRoot;

pub(crate) fn spawn_gameplay_ui(mut commands: Commands) {
    commands
        .spawn((
            GameplayToastRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(72.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-180.0)),
                width: Val::Px(360.0),
                padding: UiRect::all(Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            BackgroundColor(COL_TOAST_BG),
            ZIndex(150),
        ))
        .with_children(|p| {
            p.spawn((
                GameplayToastText,
                Text::new(""),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(COL_TOAST_TEXT),
            ));
        });

    commands
        .spawn((
            ArrivalOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            ZIndex(200),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(420.0),
                        padding: UiRect::all(Val::Px(20.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(COL_ARRIVAL_BG),
                    BorderColor::all(COL_ARRIVAL_ACCENT),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("DESTINO ALCANZADO"),
                        TextFont {
                            font_size: 22.0,
                            ..default()
                        },
                        TextColor(COL_ARRIVAL_ACCENT),
                    ));
                    panel.spawn((
                        ArrivalOverlayBody,
                        Text::new(""),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.85, 0.88, 0.92)),
                    ));
                });
        });

    // Driver vignette: four edge panels (top/bottom/left/right).
    commands
        .spawn((
            DriverVignetteRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Visibility::Hidden,
            ZIndex(90),
        ))
        .with_children(|v| {
            for (top, bottom, left, right, w, h) in [
                (
                    Val::Px(0.0),
                    Val::Auto,
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Percent(100.0),
                    Val::Px(72.0),
                ),
                (
                    Val::Auto,
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Percent(100.0),
                    Val::Px(96.0),
                ),
                (
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Auto,
                    Val::Px(96.0),
                    Val::Percent(100.0),
                ),
                (
                    Val::Px(0.0),
                    Val::Px(0.0),
                    Val::Auto,
                    Val::Px(0.0),
                    Val::Px(96.0),
                    Val::Percent(100.0),
                ),
            ] {
                v.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top,
                        bottom,
                        left,
                        right,
                        width: w,
                        height: h,
                        ..default()
                    },
                    BackgroundColor(COL_VIGNETTE),
                ));
            }
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_gameplay_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    terrain: Option<Res<TerrainElevation>>,
    live: Res<LiveDrive>,
) {
    let session = &live.session;
    if session.gameplay.stop_targets.is_empty() && session.path_data.total_length_m() <= 0.0 {
        return;
    }

    let terrain_ref = terrain.as_deref();
    let size = scene.bounds.edge_radius().max(2.0) * 1.8;
    let sphere = meshes.add(Sphere::new(size));
    let pole = meshes.add(Cylinder::new(size * 0.12, size * 3.0));

    let next_mat = materials.add(StandardMaterial {
        base_color: COL_STOP_NEXT,
        emissive: LinearRgba::from(COL_STOP_NEXT) * 0.6,
        ..default()
    });
    let future_mat = materials.add(StandardMaterial {
        base_color: COL_STOP_FUTURE,
        emissive: LinearRgba::from(COL_STOP_FUTURE) * 0.35,
        ..default()
    });
    let passed_mat = materials.add(StandardMaterial {
        base_color: COL_STOP_PASSED,
        emissive: LinearRgba::from(COL_STOP_PASSED) * 0.2,
        ..default()
    });
    commands.insert_resource(GameplayMarkerMaterials {
        passed: passed_mat.clone(),
        next: next_mat.clone(),
        future: future_mat.clone(),
    });
    let dest_mat = materials.add(StandardMaterial {
        base_color: COL_DEST,
        emissive: LinearRgba::from(COL_DEST) * 0.55,
        ..default()
    });
    let pole_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.48, 0.52),
        perceptual_roughness: 0.85,
        ..default()
    });

    let path_edges = &session.state.path_edges;
    let path_data = &session.path_data;

    commands.spawn((GameplayMarkerRoot, Name::new("gameplay:markers")));

    for (idx, stop) in session.gameplay.stop_targets.iter().enumerate() {
        let Some((edge_id, pos_m)) =
            PathData::position_at_odometer(path_edges, &path_data.edges, stop.cum_dist_m)
        else {
            continue;
        };
        let Some((world, _)) = position_on_graph(
            &scene.graph,
            &edge_id,
            pos_m,
            terrain_ref,
            &scene,
            offset.delta,
            &focus,
        ) else {
            continue;
        };
        let mat = if idx == 0 {
            next_mat.clone()
        } else {
            future_mat.clone()
        };
        let y = world.y + size * 2.2;
        commands.spawn((
            GameplayStopMarker { stop_index: idx },
            Mesh3d(sphere.clone()),
            MeshMaterial3d(mat),
            Transform::from_translation(Vec3::new(world.x, y, world.z)),
            Name::new(format!("gameplay:stop:{idx}:{}", stop.name)),
        ));
        commands
            .spawn((
                StopBillboard {
                    world: Vec3::new(world.x, y + size * 1.2, world.z),
                },
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                    ..default()
                },
                Visibility::Hidden,
                BackgroundColor(Color::srgba(0.02, 0.05, 0.1, 0.75)),
                ZIndex(140),
            ))
            .with_children(|label| {
                label.spawn((
                    StopBillboardText,
                    Text::new(stop.name.clone()),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(COL_STOP_NEXT),
                ));
            });
        commands.spawn((
            Mesh3d(pole.clone()),
            MeshMaterial3d(pole_mat.clone()),
            Transform::from_translation(Vec3::new(world.x, world.y + size * 1.5, world.z)),
        ));
    }

    let dest_odom = path_data.total_length_m();
    if dest_odom > 0.0 {
        if let Some((edge_id, pos_m)) =
            PathData::position_at_odometer(path_edges, &path_data.edges, dest_odom)
        {
            if let Some((world, _)) = position_on_graph(
                &scene.graph,
                &edge_id,
                pos_m,
                terrain_ref,
                &scene,
                offset.delta,
                &focus,
            ) {
                let y = world.y + size * 2.8;
                commands.spawn((
                    GameplayDestMarker,
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(dest_mat),
                    Transform::from_translation(Vec3::new(world.x, y, world.z)),
                    Name::new("gameplay:dest"),
                ));
            }
        }
    }
}

pub(crate) fn update_gameplay_markers(
    live: Res<LiveDrive>,
    mats: Res<GameplayMarkerMaterials>,
    mut markers: Query<(&GameplayStopMarker, &mut MeshMaterial3d<StandardMaterial>)>,
    mut tracker: Local<StopPassTracker>,
    mut toast: ResMut<GameplayToast>,
) {
    let gp = &live.session.gameplay;
    if gp.next_stop_idx > tracker.next_stop_idx {
        if let Some((name, delay)) = gp.passed_stops.last() {
            let msg = if *delay > 0.5 {
                format!("Parada {name}: +{delay:.0}s tarde")
            } else {
                format!("Parada {name}: a tiempo")
            };
            toast.show(msg, 4.0);
        }
        tracker.next_stop_idx = gp.next_stop_idx;
    }

    for (marker, mut mat_handle) in &mut markers {
        let mat = if marker.stop_index < gp.next_stop_idx {
            &mats.passed
        } else if marker.stop_index == gp.next_stop_idx {
            &mats.next
        } else {
            &mats.future
        };
        if mat_handle.0 != *mat {
            *mat_handle = MeshMaterial3d(mat.clone());
        }
    }
}

pub(crate) fn update_gameplay_toast(
    time: Res<Time>,
    mut toast: ResMut<GameplayToast>,
    mut root: Query<&mut Visibility, With<GameplayToastRoot>>,
    mut text: Query<&mut Text, With<GameplayToastText>>,
) {
    if toast.ttl_s > 0.0 {
        toast.ttl_s -= time.delta_secs();
    }
    let visible = toast.ttl_s > 0.0 && !toast.message.is_empty();
    for mut vis in &mut root {
        *vis = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if visible {
        for mut t in &mut text {
            if t.0 != toast.message {
                t.0.clear();
                t.0.push_str(&toast.message);
            }
        }
    }
}

pub(crate) fn update_arrival_overlay(
    live: Res<LiveDrive>,
    mut root: Query<&mut Visibility, With<ArrivalOverlayRoot>>,
    mut body: Query<&mut Text, With<ArrivalOverlayBody>>,
) {
    let show = live.session.arrived;
    for mut vis in &mut root {
        *vis = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if !show {
        return;
    }
    let gp = &live.session.gameplay;
    let mut lines = format!(
        "Destino: {}\nTiempo: {:.0}s\nPenalización total: {:.0}\n",
        gp.destination,
        live.session.time_s(),
        gp.accrued_penalty,
    );
    if gp.passed_stops.is_empty() {
        lines.push_str("\nSin paradas programadas.");
    } else {
        lines.push_str("\nParadas:\n");
        for (name, delay) in &gp.passed_stops {
            if *delay > 0.5 {
                lines.push_str(&format!("  • {name}: +{delay:.0}s\n"));
            } else {
                lines.push_str(&format!("  • {name}: OK\n"));
            }
        }
    }
    for mut text in &mut body {
        if text.0 != lines {
            text.0 = lines.clone();
        }
    }
}

/// Map a viewport position to UI coordinates when inside the window.
pub(crate) fn stop_billboard_ui_from_viewport(
    viewport: Vec2,
    window_width: f32,
    window_height: f32,
    scale_factor: f32,
) -> Option<(f32, f32)> {
    if viewport.x < 0.0
        || viewport.y < 0.0
        || viewport.x > window_width
        || viewport.y > window_height
    {
        return None;
    }
    Some((viewport.x / scale_factor, viewport.y / scale_factor))
}

/// Screen position (UI px) for a stop label, if on-screen in front of the camera.
pub(crate) fn stop_billboard_screen_pos(
    window_width: f32,
    window_height: f32,
    scale_factor: f32,
    world_pos: Vec3,
    cam: &Camera,
    cam_global: &GlobalTransform,
) -> Option<(f32, f32)> {
    let viewport = cam.world_to_viewport(cam_global, world_pos).ok()?;
    stop_billboard_ui_from_viewport(viewport, window_width, window_height, scale_factor)
}

pub(crate) fn update_stop_billboards(
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(&StopBillboard, &mut Node, &mut Visibility), With<StopBillboard>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((cam, cam_tf)) = camera.single() else {
        return;
    };
    let scale = window.resolution.scale_factor();
    let w = window.width();
    let h = window.height();
    for (billboard, mut node, mut vis) in &mut labels {
        let Some((left, top)) =
            stop_billboard_screen_pos(w, h, scale, billboard.world, cam, cam_tf)
        else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        node.left = Val::Px(left);
        node.top = Val::Px(top);
    }
}

pub(crate) fn update_driver_vignette(
    follow: Res<CameraFollowMode>,
    mut root: Query<&mut Visibility, With<DriverVignetteRoot>>,
) {
    // Cab panel provides its own HUD; edge vignettes blocked too much of the windscreen.
    let _ = follow;
    for mut vis in &mut root {
        *vis = Visibility::Hidden;
    }
}

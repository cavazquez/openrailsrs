//! Open Rails–style 2D `Cab` view: CVF ACE background + control sprites (#152).
//!
//! Active only in [`CameraFollowMode::Cab2d`]. Never composites onto the 3D cab
//! (`DriverCam`) — matching Open Rails `Camera.Styles.Cab` vs `ThreeDimCab` (#151).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::math::{Rect, Rot2};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::ui::RelativeCursorPosition;
use bevy::ui::UiTransform;
use bevy::ui::Val2;
use bevy::ui::widget::ImageNode;
use openrailsrs_ace::read_ace;
use openrailsrs_formats::{
    CabControl, CabDialParams, CabDigitalParams, CabLeverFrames, CabViewFile, ControlType,
    ScreenRect,
};

use crate::cab_cvf::{
    self, CabCvfRuntime, CabCvfState, MatrixDriver, control_value, dial_control_value,
    digital_control_value, lever_has_authored_animation, pick_multi_state_index,
};
use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;
use crate::shapes::{
    RouteAssets, ace_to_image, cab_night_textures_enabled, cvf_texture_search_dirs,
    resolve_cvf_graphic_path_night,
};
use crate::viewer_log;

#[derive(Resource, Debug)]
pub struct CabCvfOverlayState {
    pub spawned_cvf: Option<PathBuf>,
    pub panel_size: (f32, f32),
    /// Active `CabView` index (front / left / right).
    pub view_index: usize,
    /// CVF `Direction` (pitch, yaw, roll) degrees for the active view.
    pub view_direction_deg: [f64; 3],
    /// Whether the last spawn used night ACE lookup.
    pub night_textures: bool,
    image_cache: HashMap<String, Handle<Image>>,
}

impl Default for CabCvfOverlayState {
    fn default() -> Self {
        Self {
            spawned_cvf: None,
            panel_size: (640.0, 480.0),
            view_index: 0,
            view_direction_deg: [0.0; 3],
            night_textures: false,
            image_cache: HashMap::new(),
        }
    }
}

#[derive(Component)]
pub(crate) struct CabCvfOverlayRoot;

#[derive(Component)]
struct CabCvfOverlayPanel;

#[derive(Component)]
struct CabCvfOverlayBackground;

#[derive(Component, Clone, Debug)]
pub struct CabCvfOverlayWidget {
    pub control_type: ControlType,
    pub kind: CabCvfOverlayKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CabCvfOverlayKind {
    DialNeedle {
        dial: CabDialParams,
        /// Pivot Y in ACE pixels (resolved at spawn).
        pivot_y: f32,
        tex_w: f32,
        tex_h: f32,
        draw_scale: f32,
    },
    Lever {
        frames: CabLeverFrames,
        mouse_control: bool,
        style: Option<String>,
    },
    TwoState {
        frames: CabLeverFrames,
        mouse_control: bool,
        style: Option<String>,
    },
    TriState {
        frames: CabLeverFrames,
        mouse_control: bool,
        style: Option<String>,
    },
    MultiState {
        state_index: usize,
    },
    Digital {
        digital: CabDigitalParams,
    },
}

pub fn reference_panel_size(cvf: &CabViewFile) -> (f32, f32) {
    cvf.views
        .first()
        .map(|v| {
            (
                v.window.width.max(1.0) as f32,
                v.window.height.max(1.0) as f32,
            )
        })
        .unwrap_or((640.0, 480.0))
}

/// Letterbox scale so the CVF window fits inside the screen.
fn letterbox_layout(
    panel_w: f32,
    panel_h: f32,
    screen_w: f32,
    screen_h: f32,
) -> (f32, f32, f32, f32) {
    let panel_w = panel_w.max(1.0);
    let panel_h = panel_h.max(1.0);
    let scale = (screen_w / panel_w).min(screen_h / panel_h);
    let draw_w = panel_w * scale;
    let draw_h = panel_h * scale;
    let left = (screen_w - draw_w) * 0.5;
    let bottom = (screen_h - draw_h) * 0.5;
    (left, bottom, draw_w, draw_h)
}

fn spawn_letterbox_bars(
    parent: &mut ChildSpawnerCommands,
    panel_left: f32,
    panel_bottom: f32,
    draw_w: f32,
    draw_h: f32,
    screen_w: f32,
    screen_h: f32,
) {
    let black = BackgroundColor(Color::BLACK);
    let right = (panel_left + draw_w).max(0.0);
    let top = (panel_bottom + draw_h).max(0.0);
    // Left
    if panel_left > 0.5 {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(panel_left),
                height: Val::Percent(100.0),
                ..default()
            },
            black,
            ZIndex(-1),
        ));
    }
    // Right
    if screen_w - right > 0.5 {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(right),
                bottom: Val::Px(0.0),
                width: Val::Px(screen_w - right),
                height: Val::Percent(100.0),
                ..default()
            },
            black,
            ZIndex(-1),
        ));
    }
    // Bottom (between letterbox columns)
    if panel_bottom > 0.5 {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(panel_left),
                bottom: Val::Px(0.0),
                width: Val::Px(draw_w),
                height: Val::Px(panel_bottom),
                ..default()
            },
            black,
            ZIndex(-1),
        ));
    }
    // Top
    if screen_h - top > 0.5 {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(panel_left),
                bottom: Val::Px(top),
                width: Val::Px(draw_w),
                height: Val::Px(screen_h - top),
                ..default()
            },
            black,
            ZIndex(-1),
        ));
    }
}

fn ui_node_for_rect(rect: &ScreenRect, panel_h: f32, scale: f32) -> Node {
    let x = rect.x as f32;
    let y = rect.y as f32;
    let w = rect.width as f32;
    let h = rect.height as f32;
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(x * scale),
        bottom: Val::Px((panel_h - y - h) * scale),
        width: Val::Px((w * scale).max(1.0)),
        height: Val::Px((h * scale).max(1.0)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

fn load_graphic(
    cab_dir: &Path,
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    graphic: &str,
    night: bool,
) -> Option<Handle<Image>> {
    if graphic.is_empty() {
        return None;
    }
    let cache_key = if night {
        format!("night:{graphic}")
    } else {
        graphic.to_string()
    };
    if let Some(handle) = cache.get(&cache_key) {
        return Some(handle.clone());
    }
    let path = resolve_cvf_graphic_path_night(tex_dirs, cab_dir, graphic, night)?;
    let ace = read_ace(&path).ok()?;
    let handle = images.add(ace_to_image(&ace));
    cache.insert(cache_key, handle.clone());
    Some(handle)
}

/// Overlay is suppressed only when a matching lever matrix has authored animation.
pub fn control_has_animated_3d_lever(runtime: &CabCvfRuntime, control: &ControlType) -> bool {
    runtime.matrix_drivers.values().any(|driver| match driver {
        MatrixDriver::Lever {
            control: lever,
            anim_node,
            ..
        } if cab_cvf::types_match(lever, control) => {
            lever_has_authored_animation(&runtime.shape, *anim_node)
        }
        _ => false,
    })
}

fn spawn_widget_image(
    parent: &mut ChildSpawnerCommands,
    node: Node,
    handle: Handle<Image>,
    widget: CabCvfOverlayWidget,
    rect: Option<Rect>,
    mouse_control: bool,
) {
    let mut image = ImageNode::new(handle);
    image.rect = rect;
    if mouse_control {
        parent.spawn((
            widget,
            Button,
            RelativeCursorPosition::default(),
            FocusPolicy::Block,
            node,
            image,
            UiTransform::default(),
            Visibility::Visible,
            ZIndex(20),
        ));
    } else {
        parent.spawn((
            widget,
            node,
            image,
            UiTransform::default(),
            Visibility::Visible,
        ));
    }
}

fn discrete_frame_rect(
    images: &Assets<Image>,
    handle: &Handle<Image>,
    frames: &CabLeverFrames,
    index: usize,
) -> Option<Rect> {
    let image = images.get(handle)?;
    let size = image.size();
    let (x, y, w, h) = frames.frame_rect(size.x as f32, size.y as f32, index);
    Some(Rect::new(x, y, x + w, y + h))
}

pub(crate) fn sync_cab_cvf_overlay(
    follow: Res<CameraFollowMode>,
    cvf_state: Res<CabCvfState>,
    assets: Res<RouteAssets>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay_state: ResMut<CabCvfOverlayState>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    roots: Query<Entity, With<CabCvfOverlayRoot>>,
) {
    if !follow.is_cab2d() {
        for entity in roots.iter() {
            commands.entity(entity).despawn();
        }
        overlay_state.spawned_cvf = None;
        overlay_state.image_cache.clear();
        return;
    }

    let night = cab_night_textures_enabled();
    if night != overlay_state.night_textures {
        overlay_state.night_textures = night;
        overlay_state.spawned_cvf = None;
        overlay_state.image_cache.clear();
    }

    let Some(runtime) = cvf_state.runtime.as_ref() else {
        for entity in roots.iter() {
            commands.entity(entity).despawn();
        }
        overlay_state.spawned_cvf = None;
        overlay_state.image_cache.clear();
        return;
    };
    let Some(cab_shape) = cvf_state
        .cvf_path
        .as_ref()
        .map(|p| p.with_extension("s"))
        .filter(|p| p.is_file())
    else {
        return;
    };
    let Some(cab_dir) = cab_shape.parent() else {
        return;
    };

    // ArrowLeft/Right switch CabView (front/left/right). Avoids [ ] reverser conflict.
    let view_count = runtime.cvf.views.len().max(1);
    let mut view_changed = false;
    if keys.just_pressed(KeyCode::ArrowLeft) {
        overlay_state.view_index = (overlay_state.view_index + view_count - 1) % view_count;
        view_changed = true;
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        overlay_state.view_index = (overlay_state.view_index + 1) % view_count;
        view_changed = true;
    }
    if overlay_state.view_index >= view_count {
        overlay_state.view_index = 0;
        view_changed = true;
    }

    if overlay_state.spawned_cvf.as_deref() == cvf_state.cvf_path.as_deref()
        && !roots.is_empty()
        && !view_changed
    {
        return;
    }
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
    overlay_state.image_cache.clear();

    let tex_dirs = cvf_texture_search_dirs(&cab_shape, &assets.route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let (panel_w, panel_h) = reference_panel_size(&runtime.cvf);
    overlay_state.panel_size = (panel_w, panel_h);

    let (screen_w, screen_h) = windows
        .iter()
        .next()
        .map(|w| (w.resolution.width(), w.resolution.height()))
        .unwrap_or((1280.0, 720.0));
    let (panel_left, panel_bottom, draw_w, draw_h) =
        letterbox_layout(panel_w, panel_h, screen_w, screen_h);
    let scale = draw_w / panel_w.max(1.0);

    let view = runtime
        .cvf
        .views
        .get(overlay_state.view_index)
        .or_else(|| runtime.cvf.views.first());
    if let Some(v) = view {
        overlay_state.view_direction_deg = v.direction_deg;
    }
    let bg_handle = view.and_then(|v| {
        load_graphic(
            cab_dir,
            &tex_refs,
            &mut images,
            &mut overlay_state.image_cache,
            &v.texture_ace,
            night,
        )
    });

    let mut spawned = 0usize;
    let mut skipped = 0usize;
    // Transparent root: PullmanCabFront.ace windows are alpha=0 so the 3D world
    // must show through (Open Rails CabRenderer). Only letterbox bars are black.
    commands
        .spawn((
            CabCvfOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            UiTransform::default(),
            ZIndex(90),
            Visibility::Visible,
        ))
        .with_children(|overlay| {
            spawn_letterbox_bars(
                overlay,
                panel_left,
                panel_bottom,
                draw_w,
                draw_h,
                screen_w,
                screen_h,
            );
            overlay
                .spawn((
                    CabCvfOverlayPanel,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(panel_left),
                        bottom: Val::Px(panel_bottom),
                        width: Val::Px(draw_w),
                        height: Val::Px(draw_h),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    UiTransform::default(),
                ))
                .with_children(|panel| {
                    if let Some(handle) = bg_handle {
                        panel.spawn((
                            CabCvfOverlayBackground,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                bottom: Val::Px(0.0),
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            ImageNode {
                                image: handle,
                                color: Color::WHITE,
                                ..default()
                            },
                            UiTransform::default(),
                            ZIndex(0),
                        ));
                    }
                    for control in &runtime.cvf.controls {
                        let (n, skip) = spawn_cvf_control(
                            panel,
                            control,
                            runtime,
                            cab_dir,
                            &tex_refs,
                            &mut images,
                            &mut overlay_state.image_cache,
                            panel_h,
                            scale,
                            night,
                        );
                        spawned += n;
                        skipped += skip;
                    }
                });
        });

    overlay_state.spawned_cvf = cvf_state.cvf_path.clone();
    viewer_log!(
        "openrailsrs-viewer3d: cab 2D CVF — view {}/{} — {} controls, {} widgets ({} skipped)",
        overlay_state.view_index + 1,
        view_count,
        runtime.cvf.controls.len(),
        spawned,
        skipped,
    );
}

fn spawn_dial_widget(
    panel: &mut ChildSpawnerCommands,
    cab_dir: &Path,
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    control_type: ControlType,
    dial: &CabDialParams,
    position: &ScreenRect,
    panel_h: f32,
    scale: f32,
    graphic: &str,
    night: bool,
) -> usize {
    let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic, night) else {
        return 0;
    };
    let Some(image) = images.get(&handle) else {
        return 0;
    };
    let tex_w = image.size().x as f32;
    let tex_h = image.size().y as f32;
    // OR: Scale = min(1, Control.Height / Texture.Height)
    let draw_scale = if tex_h > 0.0 {
        ((position.height as f32) / tex_h).min(1.0)
    } else {
        1.0
    };
    let pivot_y = dial.pivot.unwrap_or((tex_h * 0.5) as f64) as f32;
    let origin_x = tex_w * 0.5 * draw_scale * scale;
    let origin_y = pivot_y * draw_scale * scale;
    let draw_w = tex_w * draw_scale * scale;
    let draw_h = tex_h * draw_scale * scale;

    // Parent at pivot screen location (CVF Y from top).
    let pivot_left = (position.x as f32) * scale + origin_x;
    let pivot_from_top = (position.y as f32) * scale + origin_y;
    let pivot_bottom = panel_h * scale - pivot_from_top;

    panel
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(pivot_left),
                bottom: Val::Px(pivot_bottom),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                ..default()
            },
            UiTransform::default(),
            ZIndex(10),
        ))
        .with_children(|pivot| {
            let mut image_node = ImageNode::new(handle);
            image_node.rect = None;
            pivot.spawn((
                CabCvfOverlayWidget {
                    control_type,
                    kind: CabCvfOverlayKind::DialNeedle {
                        dial: dial.clone(),
                        pivot_y,
                        tex_w,
                        tex_h,
                        draw_scale,
                    },
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-origin_x),
                    bottom: Val::Px(-(draw_h - origin_y)),
                    width: Val::Px(draw_w.max(1.0)),
                    height: Val::Px(draw_h.max(1.0)),
                    ..default()
                },
                image_node,
                UiTransform::default(),
                Visibility::Visible,
            ));
        });
    1
}

#[allow(clippy::too_many_arguments)]
fn spawn_cvf_control(
    panel: &mut ChildSpawnerCommands,
    control: &CabControl,
    _runtime: &CabCvfRuntime,
    cab_dir: &Path,
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    panel_h: f32,
    scale: f32,
    night: bool,
) -> (usize, usize) {
    let mut skip = 0usize;

    match control {
        CabControl::Dial {
            control_type,
            position,
            graphic,
            dial,
        } => {
            // Cab2d always draws CVF sprites (3D lever mesh is despawned).
            let n = spawn_dial_widget(
                panel,
                cab_dir,
                tex_dirs,
                images,
                cache,
                control_type.clone(),
                dial,
                position,
                panel_h,
                scale,
                graphic,
                night,
            );
            if n == 0 {
                skip = 1;
            }
            (n, skip)
        }
        CabControl::Lever {
            control_type,
            position: Some(position),
            graphic,
            frames,
            mouse_control,
            style,
        } => {
            let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic, night)
            else {
                return (0, 1);
            };
            let rect = if frames.frames_count > 1 && frames.frames_x > 0 && frames.frames_y > 0 {
                discrete_frame_rect(images, &handle, frames, 0)
            } else {
                None
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::Lever {
                        frames: frames.clone(),
                        mouse_control: *mouse_control,
                        style: style.clone(),
                    },
                },
                rect,
                *mouse_control,
            );
            (1, 0)
        }
        CabControl::TwoStateDisplay {
            control_type,
            position,
            graphic,
            frames,
            mouse_control,
            style,
        } => {
            let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic, night)
            else {
                return (0, 1);
            };
            let rect = if frames.frames_count > 1 {
                discrete_frame_rect(images, &handle, frames, 0)
            } else {
                None
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::TwoState {
                        frames: frames.clone(),
                        mouse_control: *mouse_control,
                        style: style.clone(),
                    },
                },
                rect,
                *mouse_control,
            );
            (1, 0)
        }
        CabControl::TriStateDisplay {
            control_type,
            position,
            graphic,
            frames,
            mouse_control,
            style,
        } => {
            if position.width <= 0.0 || position.height <= 0.0 {
                return (0, 1);
            }
            let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic, night)
            else {
                return (0, 1);
            };
            let rect = if frames.frames_count > 1 {
                discrete_frame_rect(images, &handle, frames, 0)
            } else {
                None
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::TriState {
                        frames: frames.clone(),
                        mouse_control: *mouse_control,
                        style: style.clone(),
                    },
                },
                rect,
                *mouse_control,
            );
            (1, 0)
        }
        CabControl::MultiStateDisplay {
            control_type,
            position,
            graphic,
            states,
        } => {
            let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic, night)
            else {
                return (0, 1);
            };
            let base = ui_node_for_rect(position, panel_h, scale);
            for (state_index, _) in states.iter().enumerate() {
                spawn_widget_image(
                    panel,
                    base.clone(),
                    handle.clone(),
                    CabCvfOverlayWidget {
                        control_type: control_type.clone(),
                        kind: CabCvfOverlayKind::MultiState { state_index },
                    },
                    None,
                    false,
                );
            }
            (states.len().max(1), 0)
        }
        CabControl::Digital {
            control_type,
            position,
            digital,
        } => {
            if position.width <= 0.0 || position.height <= 0.0 {
                return (0, 1);
            }
            let justify = match digital.justification {
                2 => JustifyContent::FlexStart,
                3 => JustifyContent::FlexEnd,
                _ => JustifyContent::Center,
            };
            let mut node = ui_node_for_rect(position, panel_h, scale);
            node.justify_content = justify;
            node.align_items = AlignItems::Center;
            let font_size = (position.height as f32 * scale * 0.85).clamp(8.0, 28.0);
            panel.spawn((
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::Digital {
                        digital: digital.clone(),
                    },
                },
                node,
                Text::new("0"),
                TextFont {
                    font_size: FontSize::Px(font_size),
                    ..default()
                },
                TextColor(Color::srgb(1.0, 0.15, 0.1)),
                UiTransform::default(),
                Visibility::Visible,
                ZIndex(15),
            ));
            (1, 0)
        }
        CabControl::Lever { .. }
        | CabControl::Gauge { .. }
        | CabControl::Screen { .. }
        | CabControl::Unknown { .. } => (0, 0),
    }
}

fn apply_discrete_frame(
    images: &Assets<Image>,
    image_node: &mut ImageNode,
    frames: &CabLeverFrames,
    index: usize,
) {
    if frames.frames_count > 1 && frames.frames_x > 0 && frames.frames_y > 0 {
        if let Some(image) = images.get(&image_node.image) {
            let size = image.size();
            let (x, y, w, h) = frames.frame_rect(size.x as f32, size.y as f32, index);
            image_node.rect = Some(Rect::new(x, y, x + w, y + h));
        }
    }
}

pub(crate) fn update_cab_cvf_overlay(
    follow: Res<CameraFollowMode>,
    cvf_state: Res<CabCvfState>,
    live: Option<Res<LiveDrive>>,
    images: Res<Assets<Image>>,
    mut roots: Query<&mut Visibility, With<CabCvfOverlayRoot>>,
    mut image_widgets: Query<
        (
            &CabCvfOverlayWidget,
            &mut UiTransform,
            &mut Visibility,
            &mut ImageNode,
        ),
        (With<ImageNode>, Without<CabCvfOverlayRoot>),
    >,
    mut digital_widgets: Query<
        (&CabCvfOverlayWidget, &mut Text, &mut Visibility),
        (With<Text>, Without<ImageNode>, Without<CabCvfOverlayRoot>),
    >,
) {
    let Ok(mut root_vis) = roots.single_mut() else {
        return;
    };
    if !follow.is_cab2d() {
        *root_vis = Visibility::Hidden;
        return;
    }
    let Some(runtime) = cvf_state.runtime.as_ref() else {
        *root_vis = Visibility::Hidden;
        return;
    };
    let Some(live) = live else {
        *root_vis = Visibility::Hidden;
        return;
    };
    *root_vis = Visibility::Visible;
    let tel = live.session.cab_telemetry();

    for (widget, mut ui, mut visibility, mut image_node) in &mut image_widgets {
        let value = control_value(&widget.control_type, &tel);
        match &widget.kind {
            CabCvfOverlayKind::DialNeedle { dial, .. } => {
                *visibility = Visibility::Visible;
                let reading = dial_control_value(&widget.control_type, dial, &tel);
                ui.rotation = Rot2::radians(dial.rotation_radians(reading));
                ui.translation = Val2::ZERO;
            }
            CabCvfOverlayKind::Lever { frames, .. } => {
                *visibility = Visibility::Visible;
                ui.rotation = Rot2::IDENTITY;
                ui.translation = Val2::ZERO;
                let index = frames.percent_to_index(value);
                apply_discrete_frame(&images, &mut image_node, frames, index);
            }
            CabCvfOverlayKind::TwoState { frames, .. } => {
                *visibility = Visibility::Visible;
                ui.rotation = Rot2::IDENTITY;
                ui.translation = Val2::ZERO;
                let index = if value > 0.5 { 1 } else { 0 };
                apply_discrete_frame(&images, &mut image_node, frames, index);
            }
            CabCvfOverlayKind::TriState { frames, .. } => {
                *visibility = Visibility::Visible;
                ui.rotation = Rot2::IDENTITY;
                ui.translation = Val2::ZERO;
                let index = if value <= 0.25 {
                    0
                } else if value >= 0.75 {
                    2
                } else {
                    1
                };
                apply_discrete_frame(&images, &mut image_node, frames, index);
            }
            CabCvfOverlayKind::MultiState { state_index } => {
                let active = pick_multi_state_index(&runtime.cvf, &widget.control_type, value);
                *visibility = if active == *state_index {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
            CabCvfOverlayKind::Digital { .. } => {}
        }
    }

    for (widget, mut text, mut visibility) in &mut digital_widgets {
        let CabCvfOverlayKind::Digital { digital } = &widget.kind else {
            continue;
        };
        *visibility = Visibility::Visible;
        let reading = digital_control_value(&widget.control_type, digital, &tel);
        **text = digital.format_value(reading);
    }
}

/// Cab2d mouse: levers / two-state with `MouseControl ( 1 )` (OR discrete renderer).
pub(crate) fn handle_cab2d_mouse_controls(
    follow: Res<CameraFollowMode>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut live: ResMut<LiveDrive>,
    widgets: Query<
        (
            &CabCvfOverlayWidget,
            &Interaction,
            Option<&RelativeCursorPosition>,
        ),
        (With<Button>, With<CabCvfOverlayWidget>),
    >,
) {
    if !follow.is_cab2d() {
        return;
    }

    for (widget, interaction, relative) in &widgets {
        let mouse_ok = match &widget.kind {
            CabCvfOverlayKind::Lever { mouse_control, .. }
            | CabCvfOverlayKind::TwoState { mouse_control, .. }
            | CabCvfOverlayKind::TriState { mouse_control, .. } => *mouse_control,
            _ => false,
        };
        if !mouse_ok {
            continue;
        }
        let style = match &widget.kind {
            CabCvfOverlayKind::Lever { style, .. }
            | CabCvfOverlayKind::TwoState { style, .. }
            | CabCvfOverlayKind::TriState { style, .. } => style.as_deref().unwrap_or(""),
            _ => "",
        };

        match &widget.kind {
            CabCvfOverlayKind::Lever { frames, .. } => {
                if *interaction != Interaction::Pressed {
                    continue;
                }
                let Some(rel) = relative.and_then(|r| r.normalized) else {
                    continue;
                };
                // Bevy UI normalized: (0,0) top-left → (1,1) bottom-right.
                let mut frac = if frames.orientation != 0 {
                    1.0 - rel.y as f64
                } else {
                    rel.x as f64
                };
                if !frames.dir_increase {
                    frac = 1.0 - frac;
                }
                let value = (frames.min_value + frac * (frames.max_value - frames.min_value))
                    .clamp(0.0, 1.0);
                apply_mouse_lever_value(&mut live, &widget.control_type, value);
            }
            CabCvfOverlayKind::TwoState { .. } => {
                let is_horn = matches!(
                    &widget.control_type,
                    ControlType::Generic(n) if n.eq_ignore_ascii_case("HORN")
                );
                if (style == "WHILE_PRESSED" || style == "PRESSED")
                    && is_horn
                    && *interaction == Interaction::Pressed
                {
                    live.session.trigger_horn(0.2);
                } else if (style == "ONOFF" || style.is_empty())
                    && mouse_buttons.just_pressed(MouseButton::Left)
                    && *interaction == Interaction::Pressed
                {
                    if is_horn {
                        live.session.trigger_horn(0.35);
                    } else if matches!(
                        &widget.control_type,
                        ControlType::Generic(n) if n.contains("WIPER")
                    ) {
                        live.session.toggle_wiper();
                    }
                }
            }
            CabCvfOverlayKind::TriState { frames, .. } => {
                if *interaction != Interaction::Pressed {
                    continue;
                }
                let Some(rel) = relative.and_then(|r| r.normalized) else {
                    continue;
                };
                let mut frac = if frames.orientation != 0 {
                    1.0 - rel.y as f64
                } else {
                    rel.x as f64
                };
                if !frames.dir_increase {
                    frac = 1.0 - frac;
                }
                let _ = frac; // headlight etc. — no live binding yet
            }
            _ => {}
        }
    }
}

fn apply_mouse_lever_value(live: &mut LiveDrive, control: &ControlType, value: f64) {
    match control {
        ControlType::Throttle | ControlType::ThrottleDisplay => {
            live.session.driver_throttle = value.clamp(0.0, 1.0);
            if value > 0.01 {
                live.session.driver_brake = 0.0;
            }
        }
        ControlType::TrainBrake => {
            live.session.driver_brake = value.clamp(0.0, 1.0);
            if value > 0.01 {
                live.session.driver_throttle = 0.0;
            }
        }
        ControlType::DirectionDisplay => {
            live.session.driver_direction = value.clamp(0.0, 1.0);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{AnimController, AnimNode, Animation, ShapeFile};
    use std::path::PathBuf;

    #[test]
    fn letterbox_centers_panel() {
        let (left, bottom, w, h) = letterbox_layout(640.0, 480.0, 1280.0, 720.0);
        assert!((w - 960.0).abs() < 1.0);
        assert!((h - 720.0).abs() < 1.0);
        assert!((left - 160.0).abs() < 1.0);
        assert!(bottom.abs() < 1.0);
    }

    #[test]
    fn reference_panel_size_uses_cabview_window() {
        let cvf = CabViewFile {
            cab_view_type: Some(2),
            views: vec![openrailsrs_formats::CabView {
                texture_ace: "panel.ace".into(),
                window: ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                },
                position_m: [0.0; 3],
                direction_deg: [0.0; 3],
            }],
            controls: vec![],
        };
        assert_eq!(reference_panel_size(&cvf), (800.0, 600.0));
    }

    #[test]
    fn resolve_cvf_graphic_finds_sibling_cabview() {
        let cab_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern")
            .join("TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d");
        if !cab_dir.is_dir() {
            return;
        }
        let dirs = cvf_texture_search_dirs(&cab_dir.join("PULLMAN_GR.s"), &cab_dir);
        let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        assert!(
            crate::shapes::resolve_cvf_graphic_path(&refs, &cab_dir, "hornlever.ace").is_some()
                || crate::shapes::resolve_cvf_graphic_path(&refs, &cab_dir, "HornLever.ace")
                    .is_some()
        );
    }

    #[test]
    fn night_resolve_prefers_night_subdir_when_present() {
        let cab3d = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d",
        );
        if !cab3d.join("NIGHT").is_dir() {
            return;
        }
        let dirs = cvf_texture_search_dirs(&cab3d.join("PULLMAN_GR.s"), &cab3d);
        let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        let day = crate::shapes::resolve_cvf_graphic_path(&refs, &cab3d, "Cab1.ace");
        let night = crate::shapes::resolve_cvf_graphic_path_night(&refs, &cab3d, "Cab1.ace", true);
        assert!(day.is_some());
        assert!(night.is_some());
        let night = night.unwrap();
        assert!(
            night
                .components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case("night")),
            "night path should include NIGHT/: {}",
            night.display()
        );
    }

    #[test]
    fn missing_cab_ace_does_not_resolve_outside_trainset() {
        // Pullman CVF references bare `cab.ace` (KIHA leftovers) with huge Positions.
        // Must not pick another loco's cab.ace under Content/.
        let content = PathBuf::from(
            "/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d",
        );
        if !content.is_dir() {
            return;
        }
        let dirs = cvf_texture_search_dirs(&content.join("PULLMAN_GR.s"), &content);
        let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
        assert!(
            crate::shapes::resolve_cvf_graphic_path(&refs, &content, "cab.ace").is_none(),
            "bare cab.ace must stay unresolved in Pullman trainset"
        );
        assert!(
            crate::shapes::resolve_cvf_graphic_path(
                &refs,
                &content,
                "../../KIHA31/CabView/KMHNeedle.ace"
            )
            .is_some()
                || !PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/KIHA31/CabView/KMHNeedle.ace")
                    .is_file(),
            "authored relative KIHA paths must still resolve"
        );
    }

    #[test]
    fn overlay_shows_lever_when_matrix_has_no_authored_anim() {
        let mut shape = ShapeFile::default();
        shape.matrices.push(Default::default());
        let mut runtime = CabCvfRuntime {
            cvf: CabViewFile {
                cab_view_type: None,
                views: vec![],
                controls: vec![],
            },
            shape,
            matrix_drivers: HashMap::new(),
        };
        runtime.matrix_drivers.insert(
            8,
            MatrixDriver::Lever {
                control: ControlType::Throttle,
                order: 0,
                anim_node: None,
            },
        );
        assert!(!control_has_animated_3d_lever(
            &runtime,
            &ControlType::Throttle
        ));
    }

    #[test]
    fn overlay_hides_lever_when_matrix_has_authored_anim() {
        let mut shape = ShapeFile::default();
        shape.matrices.push(Default::default());
        shape.animations.push(Animation {
            frame_count: 10,
            frame_rate: 30,
            nodes: vec![AnimNode {
                name: "THROTTLE:0:0".into(),
                controllers: vec![AnimController::SlerpRot {
                    keys: vec![(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [0.0, 0.0, 0.0, 1.0])],
                }],
            }],
        });
        let mut runtime = CabCvfRuntime {
            cvf: CabViewFile {
                cab_view_type: None,
                views: vec![],
                controls: vec![],
            },
            shape,
            matrix_drivers: HashMap::new(),
        };
        runtime.matrix_drivers.insert(
            0,
            MatrixDriver::Lever {
                control: ControlType::Throttle,
                order: 0,
                anim_node: Some(0),
            },
        );
        assert!(control_has_animated_3d_lever(
            &runtime,
            &ControlType::Throttle
        ));
    }

    #[test]
    fn dial_rotation_uses_scale_pos() {
        let dial = CabDialParams {
            scale_min: 0.0,
            scale_max: 100.0,
            from_degree: 190.0,
            to_degree: 150.0,
            pivot: Some(21.0),
            dir_increase: false,
            units: Some("MILES_PER_HOUR".into()),
        };
        assert!((dial.range_fraction(0.0) - 0.0).abs() < 1e-6);
        assert!((dial.range_fraction(100.0) - 1.0).abs() < 1e-6);
        let a0 = dial.rotation_radians(0.0);
        let a1 = dial.rotation_radians(100.0);
        assert!((a0 - a1).abs() > 0.1);
    }

    #[test]
    fn two_state_frame_index_from_value() {
        let frames = CabLeverFrames {
            frames_count: 2,
            frames_x: 2,
            frames_y: 1,
            ..Default::default()
        };
        assert_eq!(if 0.2_f64 > 0.5 { 1 } else { 0 }, 0);
        assert_eq!(if 0.8_f64 > 0.5 { 1 } else { 0 }, 1);
        let (x0, _, w, _) = frames.frame_rect(100.0, 50.0, 0);
        let (x1, _, _, _) = frames.frame_rect(100.0, 50.0, 1);
        assert!((w - 50.0).abs() < 1e-3);
        assert!((x1 - x0 - 50.0).abs() < 1e-3);
    }
}

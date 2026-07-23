//! CVF 2D control sprites for a future Open Rails–style 2D `Cab` view (#152).
//!
//! **Not used on 3D cab (`DriverCam`)** — Open Rails never composites CVF ACE
//! sprites onto `ThreeDimCab` (#151). Lever/gauge ACE frame helpers stay here
//! for the 2D path; 3D lever meshes animate only with authored controllers (#147).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::math::{Rect, Rot2};
use bevy::prelude::*;
use bevy::ui::UiTransform;
use bevy::ui::Val2;
use bevy::ui::widget::ImageNode;
use openrailsrs_ace::read_ace;
use openrailsrs_formats::{CabControl, CabLeverFrames, CabViewFile, ControlType, ScreenRect};

use crate::cab_cvf::{
    self, CabCvfRuntime, CabCvfState, MatrixDriver, control_value, lever_has_authored_animation,
    pick_multi_state_index,
};
use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;
use crate::shapes::{RouteAssets, ace_to_image, cvf_texture_search_dirs, resolve_cvf_graphic_path};
use crate::viewer_log;

const OVERLAY_PANEL_WIDTH_PX: f32 = 480.0;
const OVERLAY_BOTTOM_PX: f32 = 300.0;

/// Open Rails never draws CVF ACE sprites in `ThreeDimCab` (#151).
/// Opt-in only for debugging until the 2D `Cab` view lands (#152).
fn cvf_overlay_in_3d_cab_enabled() -> bool {
    matches!(
        std::env::var("OPENRAILSRS_CAB_CVF_OVERLAY").ok().as_deref(),
        Some("1") | Some("true") | Some("on")
    )
}

#[derive(Resource, Default, Debug)]
pub struct CabCvfOverlayState {
    pub spawned_cvf: Option<PathBuf>,
    pub panel_size: (f32, f32),
    image_cache: HashMap<String, Handle<Image>>,
}

#[derive(Component)]
pub(crate) struct CabCvfOverlayRoot;

#[derive(Component)]
struct CabCvfOverlayPanel;

#[derive(Component, Clone, Debug)]
pub struct CabCvfOverlayWidget {
    pub control_type: ControlType,
    pub kind: CabCvfOverlayKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CabCvfOverlayKind {
    DialNeedle,
    Lever { frames: CabLeverFrames },
    TwoState,
    TriState,
    MultiState { state_index: usize },
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

fn panel_scale(panel_w: f32) -> f32 {
    OVERLAY_PANEL_WIDTH_PX / panel_w.max(1.0)
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
) -> Option<Handle<Image>> {
    if graphic.is_empty() {
        return None;
    }
    if let Some(handle) = cache.get(graphic) {
        return Some(handle.clone());
    }
    let path = resolve_cvf_graphic_path(tex_dirs, cab_dir, graphic)?;
    let ace = read_ace(&path).ok()?;
    let handle = images.add(ace_to_image(&ace));
    cache.insert(graphic.to_string(), handle.clone());
    Some(handle)
}

/// Overlay is suppressed only when a matching lever matrix has authored animation.
pub fn control_has_animated_3d_lever(runtime: &CabCvfRuntime, control: &ControlType) -> bool {
    runtime.matrix_drivers.values().any(|driver| match driver {
        MatrixDriver::Lever {
            control: lever,
            anim_node,
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
) {
    let mut image = ImageNode::new(handle);
    image.rect = rect;
    parent.spawn((
        widget,
        node,
        image,
        UiTransform::default(),
        Visibility::Visible,
    ));
}

pub(crate) fn sync_cab_cvf_overlay(
    follow: Res<CameraFollowMode>,
    cvf_state: Res<CabCvfState>,
    assets: Res<RouteAssets>,
    mut overlay_state: ResMut<CabCvfOverlayState>,
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    roots: Query<Entity, With<CabCvfOverlayRoot>>,
) {
    if !cvf_overlay_in_3d_cab_enabled() {
        for entity in roots.iter() {
            commands.entity(entity).despawn();
        }
        overlay_state.spawned_cvf = None;
        overlay_state.image_cache.clear();
        return;
    }

    let in_cab = *follow == CameraFollowMode::DriverCam;
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
    if !in_cab {
        for entity in roots.iter() {
            commands.entity(entity).despawn();
        }
        overlay_state.spawned_cvf = None;
        overlay_state.image_cache.clear();
        return;
    }
    if overlay_state.spawned_cvf.as_deref() == cvf_state.cvf_path.as_deref() && !roots.is_empty() {
        return;
    }
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }
    overlay_state.image_cache.clear();

    let tex_dirs = cvf_texture_search_dirs(&cab_shape, &assets.route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let (panel_w, panel_h) = reference_panel_size(&runtime.cvf);
    let scale = panel_scale(panel_w);
    overlay_state.panel_size = (panel_w, panel_h);

    let mut spawned = 0usize;
    let mut skipped = 0usize;
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
            overlay
                .spawn((
                    CabCvfOverlayPanel,
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(OVERLAY_BOTTOM_PX),
                        left: Val::Percent(50.0),
                        margin: UiRect::left(Val::Px(-OVERLAY_PANEL_WIDTH_PX * 0.5)),
                        width: Val::Px(panel_w * scale),
                        height: Val::Px(panel_h * scale),
                        ..default()
                    },
                    UiTransform::default(),
                ))
                .with_children(|panel| {
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
                        );
                        spawned += n;
                        skipped += skip;
                    }
                });
        });

    overlay_state.spawned_cvf = cvf_state.cvf_path.clone();
    viewer_log!(
        "openrailsrs-viewer3d: cab CVF overlay — {} controls, {} widgets ({} skipped, no ACE)",
        runtime.cvf.controls.len(),
        spawned,
        skipped,
    );
}

#[allow(clippy::too_many_arguments)]
fn spawn_one_widget(
    panel: &mut ChildSpawnerCommands,
    cab_dir: &Path,
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    control_type: ControlType,
    kind: CabCvfOverlayKind,
    position: &ScreenRect,
    panel_h: f32,
    scale: f32,
    graphic: &str,
    rect: Option<Rect>,
) -> usize {
    let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic) else {
        return 0;
    };
    spawn_widget_image(
        panel,
        ui_node_for_rect(position, panel_h, scale),
        handle,
        CabCvfOverlayWidget { control_type, kind },
        rect,
    );
    1
}

fn lever_frame_rect(images: &Assets<Image>, handle: &Handle<Image>, frames: &CabLeverFrames) -> Option<Rect> {
    let image = images.get(handle)?;
    let size = image.size();
    let (x, y, w, h) = frames.frame_rect(size.x as f32, size.y as f32, 0);
    Some(Rect::new(x, y, x + w, y + h))
}

#[allow(clippy::too_many_arguments)]
fn spawn_cvf_control(
    panel: &mut ChildSpawnerCommands,
    control: &CabControl,
    runtime: &CabCvfRuntime,
    cab_dir: &Path,
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    cache: &mut HashMap<String, Handle<Image>>,
    panel_h: f32,
    scale: f32,
) -> (usize, usize) {
    let mut skip = 0usize;

    match control {
        CabControl::Dial {
            control_type,
            position,
            graphic,
        } => {
            if control_has_animated_3d_lever(runtime, control_type) {
                return (0, 0);
            }
            let n = spawn_one_widget(
                panel,
                cab_dir,
                tex_dirs,
                images,
                cache,
                control_type.clone(),
                CabCvfOverlayKind::DialNeedle,
                position,
                panel_h,
                scale,
                graphic,
                None,
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
        } => {
            if control_has_animated_3d_lever(runtime, control_type) {
                return (0, 0);
            }
            let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic) else {
                return (0, 1);
            };
            let rect = if frames.frames_count > 1 && frames.frames_x > 0 && frames.frames_y > 0 {
                lever_frame_rect(images, &handle, frames)
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
                    },
                },
                rect,
            );
            (1, 0)
        }
        CabControl::TwoStateDisplay {
            control_type,
            position,
            graphic,
        } => {
            let n = spawn_one_widget(
                panel,
                cab_dir,
                tex_dirs,
                images,
                cache,
                control_type.clone(),
                CabCvfOverlayKind::TwoState,
                position,
                panel_h,
                scale,
                graphic,
                None,
            );
            if n == 0 {
                skip = 1;
            }
            (n, skip)
        }
        CabControl::TriStateDisplay {
            control_type,
            position,
            graphic,
        } => {
            if position.width <= 0.0 || position.height <= 0.0 {
                return (0, 1);
            }
            let n = spawn_one_widget(
                panel,
                cab_dir,
                tex_dirs,
                images,
                cache,
                control_type.clone(),
                CabCvfOverlayKind::TriState,
                position,
                panel_h,
                scale,
                graphic,
                None,
            );
            if n == 0 {
                skip = 1;
            }
            (n, skip)
        }
        CabControl::MultiStateDisplay {
            control_type,
            position,
            graphic,
            states,
        } => {
            if control_has_animated_3d_lever(runtime, control_type) {
                return (0, 0);
            }
            let Some(handle) = load_graphic(cab_dir, tex_dirs, images, cache, graphic) else {
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
                );
            }
            (states.len().max(1), 0)
        }
        CabControl::Lever { .. } | CabControl::Digital { .. } | CabControl::Unknown { .. } => {
            (0, 0)
        }
    }
}

pub(crate) fn update_cab_cvf_overlay(
    time: Res<Time>,
    follow: Res<CameraFollowMode>,
    cvf_state: Res<CabCvfState>,
    live: Option<Res<LiveDrive>>,
    images: Res<Assets<Image>>,
    mut roots: Query<&mut Visibility, With<CabCvfOverlayRoot>>,
    mut widgets: Query<
        (
            &CabCvfOverlayWidget,
            &mut UiTransform,
            &mut Visibility,
            &mut ImageNode,
        ),
        Without<CabCvfOverlayRoot>,
    >,
) {
    let Ok(mut root_vis) = roots.single_mut() else {
        return;
    };
    if *follow != CameraFollowMode::DriverCam {
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

    for (widget, mut ui, mut visibility, mut image_node) in &mut widgets {
        let value = control_value(&widget.control_type, &tel);
        match &widget.kind {
            CabCvfOverlayKind::DialNeedle => {
                *visibility = Visibility::Visible;
                let angle = -0.65 + value * 1.3;
                ui.rotation = Rot2::radians(angle as f32);
                ui.translation = Val2::ZERO;
            }
            CabCvfOverlayKind::Lever { frames } => {
                *visibility = Visibility::Visible;
                ui.rotation = Rot2::IDENTITY;
                ui.translation = Val2::ZERO;
                if frames.frames_count > 1 && frames.frames_x > 0 && frames.frames_y > 0 {
                    if let Some(image) = images.get(&image_node.image) {
                        let size = image.size();
                        let index = frames.percent_to_index(value);
                        let (x, y, w, h) =
                            frames.frame_rect(size.x as f32, size.y as f32, index);
                        image_node.rect = Some(Rect::new(x, y, x + w, y + h));
                    }
                }
            }
            CabCvfOverlayKind::TwoState => {
                *visibility = Visibility::Visible;
                let pressed = value > 0.5;
                ui.rotation = Rot2::IDENTITY;
                ui.translation = Val2::px(0.0, if pressed { -6.0 } else { 0.0 });
            }
            CabCvfOverlayKind::TriState => {
                *visibility = Visibility::Visible;
                let slot = if value <= 0.25 {
                    -1.0
                } else if value >= 0.75 {
                    1.0
                } else {
                    0.0
                };
                ui.rotation = Rot2::IDENTITY;
                ui.translation = Val2::px(slot * 10.0, 0.0);
            }
            CabCvfOverlayKind::MultiState { state_index } => {
                let active = pick_multi_state_index(&runtime.cvf, &widget.control_type, value);
                *visibility = if active == *state_index {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
        }
        if widget.control_type.as_str().contains("WIPER") && tel.speed_kmh > 5.0 {
            let angle = (time.elapsed_secs() * 6.0).sin() * 0.9;
            ui.rotation = Rot2::radians(angle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{AnimController, AnimNode, Animation, ShapeFile};
    use std::path::PathBuf;

    #[test]
    fn cvf_overlay_disabled_by_default_in_3d_cab() {
        unsafe {
            std::env::remove_var("OPENRAILSRS_CAB_CVF_OVERLAY");
        }
        assert!(!cvf_overlay_in_3d_cab_enabled());
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
                anim_node: Some(0),
            },
        );
        assert!(control_has_animated_3d_lever(
            &runtime,
            &ControlType::Throttle
        ));
    }
}

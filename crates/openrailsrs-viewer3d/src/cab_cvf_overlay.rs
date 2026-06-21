//! CVF 2D control sprites in driver view (Open Rails `CabRenderer` analogue).
//!
//! Pullman and similar cabs without shape `animations` draw gauges, horn and wipers
//! from `.cvf` ACE sprites. 3D lever meshes (M4/M8/M9/M10) stay in [`crate::cab_cvf`].

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::ui::widget::ImageNode;
use openrailsrs_ace::read_ace;
use openrailsrs_formats::{CabControl, CabViewFile, ControlType, ScreenRect};

use crate::cab_cvf::{
    self, CabCvfRuntime, CabCvfState, MatrixDriver, control_value, pick_multi_state_index,
};
use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;
use crate::shapes::{
    RouteAssets, ace_to_image, resolve_texture_path_in_dirs, texture_search_dirs_for_shape,
};
use crate::viewer_log;

const OVERLAY_PANEL_WIDTH_PX: f32 = 480.0;
const OVERLAY_BOTTOM_PX: f32 = 300.0;

#[derive(Resource, Default, Debug)]
pub struct CabCvfOverlayState {
    pub spawned_cvf: Option<PathBuf>,
    pub panel_size: (f32, f32),
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CabCvfOverlayKind {
    DialNeedle,
    Lever,
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
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    graphic: &str,
) -> Option<Handle<Image>> {
    if graphic.is_empty() {
        return None;
    }
    let path = resolve_texture_path_in_dirs(tex_dirs, graphic)?;
    let ace = read_ace(&path).ok()?;
    Some(images.add(ace_to_image(&ace)))
}

fn control_has_3d_lever(runtime: &CabCvfRuntime, control: &ControlType) -> bool {
    runtime.matrix_drivers.values().any(|driver| {
        matches!(
            driver,
            MatrixDriver::Lever { control: lever, .. }
                if cab_cvf::types_match(lever, control)
        )
    })
}

fn spawn_widget_image(
    parent: &mut ChildSpawnerCommands,
    node: Node,
    handle: Handle<Image>,
    widget: CabCvfOverlayWidget,
) {
    parent.spawn((
        widget,
        node,
        ImageNode::new(handle),
        Transform::default(),
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
    let in_cab = *follow == CameraFollowMode::DriverCam;
    let Some(runtime) = cvf_state.runtime.as_ref() else {
        for entity in roots.iter() {
            commands.entity(entity).despawn();
        }
        overlay_state.spawned_cvf = None;
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
    if !in_cab {
        for entity in roots.iter() {
            commands.entity(entity).despawn();
        }
        overlay_state.spawned_cvf = None;
        return;
    }
    if overlay_state.spawned_cvf.as_deref() == cvf_state.cvf_path.as_deref() && !roots.is_empty() {
        return;
    }
    for entity in roots.iter() {
        commands.entity(entity).despawn();
    }

    let tex_dirs = texture_search_dirs_for_shape(&cab_shape, &assets.route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
    let (panel_w, panel_h) = reference_panel_size(&runtime.cvf);
    let scale = panel_scale(panel_w);
    overlay_state.panel_size = (panel_w, panel_h);

    let mut spawned = 0usize;
    commands
        .spawn((
            CabCvfOverlayRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
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
                ))
                .with_children(|panel| {
                    for control in &runtime.cvf.controls {
                        spawned += spawn_cvf_control(
                            panel,
                            control,
                            runtime,
                            &tex_refs,
                            &mut images,
                            panel_h,
                            scale,
                        );
                    }
                });
        });

    overlay_state.spawned_cvf = cvf_state.cvf_path.clone();
    viewer_log!(
        "openrailsrs-viewer3d: cab CVF overlay — {} controls, {} widgets",
        runtime.cvf.controls.len(),
        spawned,
    );
}

fn spawn_cvf_control(
    panel: &mut ChildSpawnerCommands,
    control: &CabControl,
    runtime: &CabCvfRuntime,
    tex_dirs: &[&Path],
    images: &mut Assets<Image>,
    panel_h: f32,
    scale: f32,
) -> usize {
    match control {
        CabControl::Dial {
            control_type,
            position,
            graphic,
        } => {
            if control_has_3d_lever(runtime, control_type) {
                return 0;
            }
            let Some(handle) = load_graphic(tex_dirs, images, graphic) else {
                return 0;
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::DialNeedle,
                },
            );
            1
        }
        CabControl::Lever {
            control_type,
            position: Some(position),
            graphic,
        } => {
            if control_has_3d_lever(runtime, control_type) {
                return 0;
            }
            let Some(handle) = load_graphic(tex_dirs, images, graphic) else {
                return 0;
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::Lever,
                },
            );
            1
        }
        CabControl::TwoStateDisplay {
            control_type,
            position,
            graphic,
        } => {
            let Some(handle) = load_graphic(tex_dirs, images, graphic) else {
                return 0;
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::TwoState,
                },
            );
            1
        }
        CabControl::TriStateDisplay {
            control_type,
            position,
            graphic,
        } => {
            if position.width <= 0.0 || position.height <= 0.0 {
                return 0;
            }
            let Some(handle) = load_graphic(tex_dirs, images, graphic) else {
                return 0;
            };
            spawn_widget_image(
                panel,
                ui_node_for_rect(position, panel_h, scale),
                handle,
                CabCvfOverlayWidget {
                    control_type: control_type.clone(),
                    kind: CabCvfOverlayKind::TriState,
                },
            );
            1
        }
        CabControl::MultiStateDisplay {
            control_type,
            position,
            graphic,
            states,
        } => {
            if control_has_3d_lever(runtime, control_type) {
                return 0;
            }
            let Some(handle) = load_graphic(tex_dirs, images, graphic) else {
                return 0;
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
                );
            }
            states.len().max(1)
        }
        CabControl::Lever { .. } | CabControl::Digital { .. } | CabControl::Unknown { .. } => 0,
    }
}

pub(crate) fn update_cab_cvf_overlay(
    time: Res<Time>,
    follow: Res<CameraFollowMode>,
    cvf_state: Res<CabCvfState>,
    live: Option<Res<LiveDrive>>,
    mut roots: Query<&mut Visibility, With<CabCvfOverlayRoot>>,
    mut widgets: Query<
        (&CabCvfOverlayWidget, &mut Transform, &mut Visibility),
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

    for (widget, mut transform, mut visibility) in &mut widgets {
        let value = control_value(&widget.control_type, &tel);
        match widget.kind {
            CabCvfOverlayKind::DialNeedle => {
                *visibility = Visibility::Visible;
                let angle = -0.65 + value * 1.3;
                transform.rotation = Quat::from_rotation_z(angle as f32);
                transform.translation = Vec3::ZERO;
            }
            CabCvfOverlayKind::Lever => {
                *visibility = Visibility::Visible;
                let travel = (value * 0.85 - 0.425) as f32;
                transform.translation = Vec3::new(0.0, travel * 24.0, 0.0);
                transform.rotation = Quat::IDENTITY;
            }
            CabCvfOverlayKind::TwoState => {
                *visibility = Visibility::Visible;
                let pressed = value > 0.5;
                transform.translation = Vec3::new(0.0, if pressed { -6.0 } else { 0.0 }, 0.0);
                transform.rotation = Quat::IDENTITY;
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
                transform.translation = Vec3::new(slot * 10.0, 0.0, 0.0);
                transform.rotation = Quat::IDENTITY;
            }
            CabCvfOverlayKind::MultiState { state_index } => {
                let active = pick_multi_state_index(&runtime.cvf, &widget.control_type, value);
                *visibility = if active == state_index {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                };
            }
        }
        if widget.control_type.as_str().contains("WIPER") && tel.speed_kmh > 5.0 {
            let angle = (time.elapsed_secs() * 6.0).sin() * 0.9;
            transform.rotation = Quat::from_rotation_z(angle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

//! OR `ThreeDimCabScreen` / ETCS DMI (#158–#162).
//!
//! Matches a cab interior prim whose ACE basename contains the CVF `ScreenDisplay`
//! Graphic, then replaces its texture with a CPU-updated RGBA buffer.
//! LMB on the mesh maps UV → DMI soft keys / subwindows.

use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use openrailsrs_formats::{CabControl, CabViewFile};

use crate::cab_cvf::CabCvfState;
use crate::cab_view::CabInteriorRoot;
use crate::camera::CameraFollowMode;
use crate::etcs::{DmiMode, EtcsUiState, uv_to_dmi};
use crate::live::LiveDrive;
use crate::or_cab_material::OrCabMaterial;
use crate::shapes::ShapePartAsset;
use crate::viewer_log;

/// Default OR DMI FullSize.
pub const CAB_SCREEN_W: u32 = 640;
pub const CAB_SCREEN_H: u32 = 480;

/// Marks a cab mesh part driven as a live screen texture.
#[derive(Component, Clone, Debug)]
pub struct CabNativeScreen {
    pub image: Handle<Image>,
    pub control_type: openrailsrs_formats::ControlType,
    pub graphic: String,
    pub hide_if_disabled: bool,
    pub dmi_mode: DmiMode,
}

/// OR-style match: material/texture key contains the Graphic basename (case-insensitive).
pub fn texture_matches_screen_graphic(texture_name: &str, graphic: &str) -> bool {
    let tex = Path::new(texture_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(texture_name)
        .to_ascii_lowercase();
    let gfx = Path::new(graphic)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(graphic)
        .to_ascii_lowercase();
    if gfx.is_empty() || tex.is_empty() {
        return false;
    }
    tex.contains(&gfx) || gfx.contains(&tex)
}

pub fn screen_controls(cvf: &CabViewFile) -> Vec<&CabControl> {
    cvf.controls
        .iter()
        .filter(|c| matches!(c, CabControl::Screen { .. }))
        .collect()
}

pub fn dmi_mode_from_params(params: &std::collections::HashMap<String, String>) -> DmiMode {
    params
        .get("mode")
        .map(|s| DmiMode::parse(s))
        .unwrap_or_default()
}

/// Create the dynamic screen image for a DMI mode.
pub fn create_screen_image(mode: DmiMode) -> Image {
    let (w, h) = mode.size();
    let px = (w * h * 4) as usize;
    let mut rgba = vec![0u8; px];
    let status = crate::etcs::EtcsStatus {
        dmi_mode: mode,
        ..Default::default()
    };
    crate::etcs::paint_dmi_full(&mut rgba, w, h, &status);
    Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// Attach a screen to a just-spawned cab part entity when ACE matches.
#[allow(clippy::too_many_arguments)]
pub fn try_attach_screen_to_part(
    entity: &mut EntityCommands,
    part: &ShapePartAsset,
    screens: &[&CabControl],
    claimed: &mut [bool],
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    or_materials: &mut Assets<OrCabMaterial>,
) -> bool {
    let Some(tex_name) = part.texture_name.as_deref() else {
        return false;
    };
    for (i, control) in screens.iter().enumerate() {
        if claimed.get(i).copied().unwrap_or(true) {
            continue;
        }
        let CabControl::Screen {
            control_type,
            graphic,
            hide_if_disabled,
            parameters,
            ..
        } = control
        else {
            continue;
        };
        if !texture_matches_screen_graphic(tex_name, graphic) {
            continue;
        }
        claimed[i] = true;
        let dmi_mode = dmi_mode_from_params(parameters);
        let image = images.add(create_screen_image(dmi_mode));
        if let Some(or_h) = part.or_cab_material.as_ref() {
            if let Some(base) = or_materials.get(or_h).cloned() {
                let mut mat = base;
                mat.base_texture = image.clone();
                entity.insert(MeshMaterial3d(or_materials.add(mat)));
            }
        } else if let Some(base) = materials.get(&part.material).cloned() {
            let mut mat = base;
            mat.base_color_texture = Some(image.clone());
            mat.unlit = true;
            entity.insert(MeshMaterial3d(materials.add(mat)));
        }
        entity.insert(CabNativeScreen {
            image,
            control_type: control_type.clone(),
            graphic: graphic.clone(),
            hide_if_disabled: *hide_if_disabled,
            dmi_mode,
        });
        viewer_log!(
            "openrailsrs-viewer3d: cab screen — {} {:?} → prim `{tex_name}`",
            control_type.as_str(),
            dmi_mode
        );
        return true;
    }
    false
}

/// Paint full DMI each frame (#159–#162).
pub fn update_cab_screens(
    follow: Res<CameraFollowMode>,
    live: Option<Res<LiveDrive>>,
    cvf_state: Option<Res<CabCvfState>>,
    mut ui: ResMut<EtcsUiState>,
    time: Res<Time>,
    interior: Query<Entity, With<CabInteriorRoot>>,
    mut screens: Query<(&CabNativeScreen, &mut Visibility)>,
    mut images: ResMut<Assets<Image>>,
) {
    if *follow != CameraFollowMode::DriverCam {
        return;
    }
    if interior.is_empty() {
        return;
    }
    let _ = cvf_state;
    let now = time.elapsed_secs_f64();
    ui.tick(now);

    for (screen, mut visibility) in &mut screens {
        if screen.hide_if_disabled {
            *visibility = Visibility::Visible;
        }
        let mut status = live
            .as_ref()
            .map(|l| crate::etcs::etcs_status_from_live(&l.session))
            .unwrap_or_default();
        status.dmi_mode = screen.dmi_mode;
        ui.apply_to_status(&mut status, now);

        let Some(mut image) = images.get_mut(&screen.image) else {
            continue;
        };
        let w = image.width();
        let h = image.height();
        if w == 0 || h == 0 {
            continue;
        }
        let Some(data) = image.data.as_mut() else {
            continue;
        };
        if data.len() < (w * h * 4) as usize {
            continue;
        }
        crate::etcs::paint_dmi(data, w, h, &status, &ui);
    }
}

/// LMB on `CabNativeScreen` mesh → DMI soft keys / subwindows.
#[allow(clippy::too_many_arguments)]
pub fn handle_cab_dmi_mouse(
    follow: Res<CameraFollowMode>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut ui: ResMut<EtcsUiState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    screens: Query<(&CabNativeScreen, &GlobalTransform, &Mesh3d)>,
    meshes: Res<Assets<Mesh>>,
    live: Option<Res<LiveDrive>>,
) {
    if *follow != CameraFollowMode::DriverCam {
        return;
    }
    let now = time.elapsed_secs_f64();

    if mouse_buttons.pressed(MouseButton::Right) {
        return;
    }
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if screens.is_empty() {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, cam_tf)) = cameras.single() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cam_tf, cursor) else {
        return;
    };

    let mut best: Option<(f32, Vec2, DmiMode)> = None;
    for (screen, gt, mesh3d) in &screens {
        let Some(mesh) = meshes.get(&mesh3d.0) else {
            continue;
        };
        let world = gt.to_matrix();
        if let Some((t, uv)) =
            crate::etcs::input::raycast_mesh_uv(mesh, world, ray.origin, Vec3::from(ray.direction))
        {
            if best.as_ref().is_none_or(|(bt, _, _)| t < *bt) {
                best = Some((t, uv, screen.dmi_mode));
            }
        }
    }
    let Some((_t, uv, mode)) = best else {
        return;
    };

    let mut status = live
        .as_ref()
        .map(|l| crate::etcs::etcs_status_from_live(&l.session))
        .unwrap_or_default();
    status.dmi_mode = mode;
    ui.apply_to_status(&mut status, now);

    let (x, y) = uv_to_dmi(uv, mode);
    let before_overlay = ui.overlay.clone();
    let before_page = ui.message_page;
    let before_scale = ui.planning_max_m;
    ui.handle_dmi_click(x, y, now, &status);
    let acted = ui.overlay != before_overlay
        || ui.message_page != before_page
        || ui.planning_max_m != before_scale
        || ui.last_action.is_some();
    if !acted {
        let (x2, y2) = uv_to_dmi(Vec2::new(uv.x, 1.0 - uv.y), mode);
        ui.handle_dmi_click(x2, y2, now, &status);
    }
    viewer_log!("openrailsrs-viewer3d: DMI click overlay={:?}", ui.overlay);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_match_is_case_insensitive_basename() {
        assert!(texture_matches_screen_graphic(
            "CabView/StaticTexture.ACE",
            "statictexture.ace"
        ));
        assert!(!texture_matches_screen_graphic(
            "speed.ace",
            "statictexture.ace"
        ));
    }

    #[test]
    fn paint_full_dmi_writes_non_uniform_pixels() {
        let mut rgba = vec![0u8; (CAB_SCREEN_W * CAB_SCREEN_H * 4) as usize];
        let status = crate::etcs::EtcsStatus::from_telemetry(72.0, 100.0, false, Some(800.0));
        crate::etcs::paint_dmi_full(&mut rgba, CAB_SCREEN_W, CAB_SCREEN_H, &status);
        let first = &rgba[0..4];
        let mid = &rgba[((CAB_SCREEN_H / 2 * CAB_SCREEN_W + CAB_SCREEN_W / 4) * 4) as usize..];
        assert_ne!(&mid[0..4], first);
    }

    #[test]
    fn mode_from_params() {
        let mut p = std::collections::HashMap::new();
        p.insert("mode".into(), "GaugeOnly".into());
        assert_eq!(dmi_mode_from_params(&p), DmiMode::GaugeOnly);
    }

    #[test]
    fn screen_controls_filters_cvf() {
        let cvf = CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![
                CabControl::Digital {
                    control_type: openrailsrs_formats::ControlType::Speedometer,
                    position: openrailsrs_formats::ScreenRect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    digital: Default::default(),
                },
                CabControl::Screen {
                    control_type: openrailsrs_formats::ControlType::OrtsEtcs,
                    position: openrailsrs_formats::ScreenRect {
                        x: 0.0,
                        y: 0.0,
                        width: 640.0,
                        height: 480.0,
                    },
                    graphic: "statictexture.ace".into(),
                    parameters: Default::default(),
                    hide_if_disabled: false,
                },
            ],
        };
        assert_eq!(screen_controls(&cvf).len(), 1);
    }
}

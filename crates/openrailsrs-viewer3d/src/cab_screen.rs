//! OR `ThreeDimCabScreen` / ETCS DMI (#158/#159/#160).
//!
//! Matches a cab interior prim whose ACE basename contains the CVF `ScreenDisplay`
//! Graphic, then replaces its texture with a CPU-updated 640×480 RGBA buffer.

use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use openrailsrs_formats::{CabControl, CabViewFile};

use crate::cab_cvf::CabCvfState;
use crate::cab_view::CabInteriorRoot;
use crate::camera::CameraFollowMode;
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

/// Create the dynamic screen image (blank; painted each frame).
pub fn create_screen_image() -> Image {
    let px = (CAB_SCREEN_W * CAB_SCREEN_H * 4) as usize;
    let mut rgba = vec![0u8; px];
    crate::etcs::paint_dmi_full(
        &mut rgba,
        CAB_SCREEN_W,
        CAB_SCREEN_H,
        &crate::etcs::EtcsStatus::default(),
    );
    Image::new(
        Extent3d {
            width: CAB_SCREEN_W,
            height: CAB_SCREEN_H,
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
            ..
        } = control
        else {
            continue;
        };
        if !texture_matches_screen_graphic(tex_name, graphic) {
            continue;
        }
        claimed[i] = true;
        let image = images.add(create_screen_image());
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
        });
        viewer_log!(
            "openrailsrs-viewer3d: cab screen — {} → prim texture `{tex_name}`",
            control_type.as_str()
        );
        return true;
    }
    false
}

/// Paint full DMI each frame (#159/#160).
pub fn update_cab_screens(
    follow: Res<CameraFollowMode>,
    live: Option<Res<LiveDrive>>,
    cvf_state: Option<Res<CabCvfState>>,
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
    let status = live
        .as_ref()
        .map(|l| crate::etcs::etcs_status_from_live(&l.session))
        .unwrap_or_default();

    for (screen, mut visibility) in &mut screens {
        if screen.hide_if_disabled {
            *visibility = Visibility::Visible;
        }
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
        crate::etcs::paint_dmi_full(data, w, h, &status);
    }
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
        assert!(texture_matches_screen_graphic(
            "statictexture.ace",
            "../cab/STATICTEXTURE.ACE"
        ));
        assert!(!texture_matches_screen_graphic("speed.ace", "statictexture.ace"));
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

//! OR `ThreeDimCabScreen` / ETCS DMI stub (#158).
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
    paint_dmi_stub(&mut rgba, CAB_SCREEN_W, CAB_SCREEN_H, 0.0, 0.0);
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

/// Paint stub DMI each frame (speed + limit). Full ETCS UI is out of scope.
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
    let (speed_kmh, limit_kmh) = live
        .as_ref()
        .map(|l| {
            let t = l.session.cab_telemetry();
            (t.speed_kmh, t.limit_kmh)
        })
        .unwrap_or((0.0, 0.0));

    for (screen, mut visibility) in &mut screens {
        // Power / HideIfDisabled: no power model yet — always show stub.
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
        paint_dmi_stub(data, w, h, speed_kmh, limit_kmh);
    }
}

/// CPU stub: dark panel + speed / limit readout (7-seg style digits).
pub fn paint_dmi_stub(rgba: &mut [u8], w: u32, h: u32, speed_kmh: f64, limit_kmh: f64) {
    let bg = [12u8, 18, 28, 255];
    let accent = [40u8, 180, 90, 255];
    let text = [230u8, 240, 255, 255];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }
    // Top bar
    fill_rect(rgba, w, h, 0, 0, w, (h / 12).max(8), [20, 40, 70, 255]);
    // Speed box
    let box_x = w / 8;
    let box_y = h / 4;
    let box_w = w / 2;
    let box_h = h / 3;
    fill_rect(rgba, w, h, box_x, box_y, box_w, box_h, [8, 12, 20, 255]);
    stroke_rect(rgba, w, h, box_x, box_y, box_w, box_h, accent);

    let speed = format!("{:.0}", speed_kmh.max(0.0));
    let limit = format!("{:.0}", limit_kmh.max(0.0));
    let digit_h = (box_h / 2).max(24);
    let digit_w = digit_h * 3 / 5;
    let mut x = box_x + box_w / 8;
    let y = box_y + box_h / 4;
    for ch in speed.chars() {
        blit_digit(rgba, w, h, x, y, digit_w, digit_h, ch, text);
        x += digit_w + digit_w / 5;
    }
    // Limit small
    let mut lx = box_x + box_w * 3 / 4;
    let ly = box_y + box_h / 8;
    let sw = digit_w / 2;
    let sh = digit_h / 2;
    for ch in limit.chars() {
        blit_digit(rgba, w, h, lx, ly, sw, sh, ch, [180, 200, 220, 255]);
        lx += sw + sw / 5;
    }
    // Bottom caption strip
    fill_rect(
        rgba,
        w,
        h,
        0,
        h.saturating_sub(h / 16),
        w,
        h / 16,
        [30, 90, 50, 255],
    );
}

fn fill_rect(rgba: &mut [u8], w: u32, h: u32, x: u32, y: u32, rw: u32, rh: u32, c: [u8; 4]) {
    let x1 = x.min(w);
    let y1 = y.min(h);
    let x2 = (x + rw).min(w);
    let y2 = (y + rh).min(h);
    for yy in y1..y2 {
        for xx in x1..x2 {
            let i = ((yy * w + xx) * 4) as usize;
            if i + 3 < rgba.len() {
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    }
}

fn stroke_rect(rgba: &mut [u8], w: u32, h: u32, x: u32, y: u32, rw: u32, rh: u32, c: [u8; 4]) {
    let t = 2u32;
    fill_rect(rgba, w, h, x, y, rw, t, c);
    fill_rect(rgba, w, h, x, y + rh.saturating_sub(t), rw, t, c);
    fill_rect(rgba, w, h, x, y, t, rh, c);
    fill_rect(rgba, w, h, x + rw.saturating_sub(t), y, t, rh, c);
}

/// Tiny 3×5 digit glyphs (bits rows).
fn blit_digit(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    dw: u32,
    dh: u32,
    ch: char,
    c: [u8; 4],
) {
    let glyph = digit_glyph(ch);
    for row in 0..5u32 {
        for col in 0..3u32 {
            if (glyph[row as usize] >> (2 - col)) & 1 == 0 {
                continue;
            }
            let px = x + col * dw / 3;
            let py = y + row * dh / 5;
            fill_rect(rgba, w, h, px, py, (dw / 3).max(1), (dh / 5).max(1), c);
        }
    }
}

fn digit_glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
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
    fn paint_stub_writes_non_uniform_pixels() {
        let mut rgba = vec![0u8; (CAB_SCREEN_W * CAB_SCREEN_H * 4) as usize];
        paint_dmi_stub(&mut rgba, CAB_SCREEN_W, CAB_SCREEN_H, 72.0, 100.0);
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

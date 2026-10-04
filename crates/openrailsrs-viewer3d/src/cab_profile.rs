//! Comfort adjustments are keyed by original CVF, preserving each authored eye.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CabProfile {
    pub seat_height_m: f32,
    pub seat_back_m: f32,
    pub wipe_scale: f32,
}
impl Default for CabProfile {
    fn default() -> Self {
        Self {
            seat_height_m: 0.0,
            seat_back_m: 0.0,
            wipe_scale: 1.0,
        }
    }
}

/// UV rectangle of the native windshield, including CVF letterboxing.
pub fn window_uv(view: &openrailsrs_formats::CabView, screen: Vec2) -> Vec4 {
    let scale = (screen.x / 640.0).min(screen.y / 480.0);
    let origin = (screen - Vec2::new(640.0, 480.0) * scale) * 0.5;
    let r = &view.window;
    if r.width <= 0.0 || r.height <= 0.0 {
        return Vec4::new(0.0, 0.0, 1.0, 1.0);
    }
    Vec4::new(
        (origin.x + r.x as f32 * scale) / screen.x,
        (origin.y + r.y as f32 * scale) / screen.y,
        r.width as f32 * scale / screen.x,
        r.height as f32 * scale / screen.y,
    )
}

pub fn blade_count(runtime: &crate::cab_cvf::CabCvfRuntime, path: &std::path::Path) -> usize {
    let named = runtime
        .shape
        .matrices
        .iter()
        .filter(|m| m.name.to_ascii_uppercase().contains("WIPER"))
        .count();
    if named > 0 {
        return named.min(2);
    }
    let name = path.to_string_lossy().to_ascii_lowercase();
    // Classic single-pane steam/railcar cabs have one blade. CVF has no blade
    // geometry; named 3D bones take precedence and unknown cabs use two blades.
    let steam = runtime
        .cvf
        .controls
        .iter()
        .any(|control| match control.control_type() {
            Some(openrailsrs_formats::ControlType::Generic(name)) => {
                name.to_ascii_uppercase().contains("BOILER")
            }
            _ => false,
        });
    if steam
        || ["hall", "king", "121", "rstock", "r-stock", "r_stock"]
            .iter()
            .any(|s| name.contains(s))
    {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windshield_rectangle_tracks_native_panel_letterboxing() {
        let v = openrailsrs_formats::CabView {
            texture_ace: String::new(),
            window: openrailsrs_formats::ScreenRect {
                x: 64.0,
                y: 48.0,
                width: 512.0,
                height: 288.0,
            },
            position_m: [0.0; 3],
            direction_deg: [0.0; 3],
        };
        let a = window_uv(&v, Vec2::new(640.0, 480.0));
        assert!(a.abs_diff_eq(Vec4::new(0.1, 0.1, 0.8, 0.6), 1e-6));
        let b = window_uv(&v, Vec2::new(1280.0, 720.0));
        assert!(b.abs_diff_eq(Vec4::new(0.2, 0.1, 0.6, 0.6), 1e-6));
    }
}

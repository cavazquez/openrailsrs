//! Planning area (OR `PlanningWindow` visual subset).

use super::colors;
use super::paint::{blit_digit3x5, fill_rect, stroke_line};
use super::status::EtcsStatus;
use super::symbols::EtcsSymbols;

pub const PLAN_W: i32 = 246;
pub const PLAN_H: i32 = 300;

/// Distance scale lines (m) — OR planning uses a log-ish set; we use linear 0–4 km.
const DIST_LINES_M: [f64; 9] = [0.0, 250.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 4000.0];

pub fn paint_planning(
    rgba: &mut [u8],
    stride_w: u32,
    stride_h: u32,
    origin_x: i32,
    origin_y: i32,
    status: &EtcsStatus,
    symbols: &EtcsSymbols,
) {
    fill_rect(
        rgba,
        stride_w,
        stride_h,
        origin_x,
        origin_y,
        PLAN_W,
        PLAN_H,
        colors::PASP_DARK,
    );

    // PASP light band for allowed speed (left strip width ∝ allowed/dial_max).
    let frac = (status.allowed_kmh / f64::from(status.dial_max_kmh)).clamp(0.05, 1.0);
    let band_w = ((PLAN_W as f64) * 0.35 * frac) as i32;
    fill_rect(
        rgba,
        stride_w,
        stride_h,
        origin_x + 40,
        origin_y,
        band_w.max(8),
        PLAN_H,
        colors::PASP_LIGHT,
    );

    // Distance grid (y = 0 at bottom of planning = train).
    for &d in &DIST_LINES_M {
        let y = dist_to_y(d, origin_y);
        stroke_line(
            rgba,
            stride_w,
            stride_h,
            origin_x + 36,
            y,
            origin_x + PLAN_W - 4,
            y,
            colors::FRAME,
        );
        let label = if d >= 1000.0 {
            format!("{:.0}", d / 1000.0)
        } else if d > 0.0 {
            format!("{d:.0}")
        } else {
            continue;
        };
        let mut lx = origin_x + 4;
        for ch in label.chars().take(4) {
            blit_digit3x5(
                rgba,
                stride_w,
                stride_h,
                lx,
                y - 5,
                6,
                9,
                ch,
                colors::GREY,
            );
            lx += 7;
        }
    }

    // Target marker + PL_ speed change symbol
    if let (Some(td), Some(ts)) = (status.target_distance_m, status.target_kmh) {
        let y = dist_to_y(td.min(4000.0), origin_y);
        stroke_line(
            rgba,
            stride_w,
            stride_h,
            origin_x + 40,
            y,
            origin_x + PLAN_W - 8,
            y,
            colors::YELLOW,
        );
        if let Some(tex) = status.planning_symbol.texture() {
            let _ = symbols.blit(rgba, stride_w, stride_h, origin_x + 44, y - 18, tex);
        }
        let txt = format!("{:.0}", ts);
        let mut tx = origin_x + PLAN_W - 50;
        for ch in txt.chars() {
            blit_digit3x5(
                rgba,
                stride_w,
                stride_h,
                tx,
                y - 14,
                10,
                14,
                ch,
                colors::YELLOW,
            );
            tx += 11;
        }
    }

    // Current allowed at bottom
    let allow = format!("{:.0}", status.allowed_kmh);
    let mut ax = origin_x + 50;
    let ay = origin_y + PLAN_H - 22;
    for ch in allow.chars() {
        blit_digit3x5(
            rgba,
            stride_w,
            stride_h,
            ax,
            ay,
            12,
            16,
            ch,
            colors::WHITE,
        );
        ax += 13;
    }
}

fn dist_to_y(dist_m: f64, origin_y: i32) -> i32 {
    // Bottom = 0 m, top = 4000 m
    let t = (dist_m / 4000.0).clamp(0.0, 1.0);
    origin_y + PLAN_H - 4 - (t * (PLAN_H - 8) as f64) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dist_y_order() {
        let y0 = dist_to_y(0.0, 15);
        let y1 = dist_to_y(4000.0, 15);
        assert!(y0 > y1);
    }
}

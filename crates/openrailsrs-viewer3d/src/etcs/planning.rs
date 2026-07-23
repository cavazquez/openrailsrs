//! Planning area (OR `PlanningWindow` visual subset, #162).

use super::colors;
use super::paint::{blit_digit3x5, fill_rect, stroke_line};
use super::status::EtcsStatus;
use super::symbols::EtcsSymbols;

pub const PLAN_W: i32 = 246;
pub const PLAN_H: i32 = 300;

/// OR `LinePositions` / `LineDistances` (‰ of max view).
const LINE_POS: [i32; 9] = [283, 250, 206, 182, 164, 150, 107, 64, 21];
const LINE_DIST: [f64; 9] = [0.0, 25.0, 50.0, 75.0, 100.0, 125.0, 250.0, 500.0, 1000.0];

pub fn paint_planning(
    rgba: &mut [u8],
    stride_w: u32,
    stride_h: u32,
    origin_x: i32,
    origin_y: i32,
    status: &EtcsStatus,
    symbols: &EtcsSymbols,
) {
    let max_m = status.planning_max_m.max(1000.0);

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

    paint_pasp(rgba, stride_w, stride_h, origin_x, origin_y, status, max_m);
    paint_gradient(rgba, stride_w, stride_h, origin_x, origin_y, status, max_m);

    for i in 0..9 {
        let d = LINE_DIST[i] * max_m / 1000.0;
        let y = origin_y + LINE_POS[i];
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
        if i == 0 || (1..=4).contains(&i) {
            continue;
        }
        let label = if d >= 1000.0 {
            format!("{:.0}", d / 1000.0)
        } else {
            format!("{d:.0}")
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

    if let Some(im) = status.indication_marker_m {
        if im > 0.0 && im < max_m {
            let y = origin_y + planning_height(im, max_m);
            stroke_line(
                rgba,
                stride_w,
                stride_h,
                origin_x + 14 + 133,
                y,
                origin_x + 14 + 133 + 93,
                y,
                colors::YELLOW,
            );
        }
    }

    for cond in &status.track_conditions {
        if cond.distance_m < 0.0 || cond.distance_m > max_m {
            continue;
        }
        let y = origin_y + planning_height(cond.distance_m, max_m);
        let _ = symbols.blit(
            rgba,
            stride_w,
            stride_h,
            origin_x + 48,
            y - 10,
            cond.kind.texture(),
        );
    }

    if let (Some(td), Some(ts)) = (status.target_distance_m, status.target_kmh) {
        if td <= max_m {
            let y = origin_y + planning_height(td, max_m);
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
                let _ = symbols.blit(rgba, stride_w, stride_h, origin_x + 90, y - 18, tex);
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
    }

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

fn paint_pasp(
    rgba: &mut [u8],
    stride_w: u32,
    stride_h: u32,
    origin_x: i32,
    origin_y: i32,
    status: &EtcsStatus,
    max_m: f64,
) {
    let dial = f64::from(status.dial_max_kmh).max(1.0);
    let targets = &status.speed_targets;
    if targets.is_empty() {
        return;
    }
    for i in 0..targets.len() {
        let cur = targets[i];
        let next_d = targets
            .get(i + 1)
            .map(|t| t.distance_m)
            .unwrap_or(max_m)
            .min(max_m);
        if cur.distance_m >= max_m {
            break;
        }
        let y0 = origin_y + planning_height(cur.distance_m.min(max_m), max_m);
        let y1 = origin_y + planning_height(next_d, max_m);
        let top = y1.min(y0);
        let bot = y0.max(y1);
        let frac = (cur.speed_kmh / dial).clamp(0.05, 1.0);
        let band_w = ((PLAN_W as f64) * 0.30 * frac) as i32;
        fill_rect(
            rgba,
            stride_w,
            stride_h,
            origin_x + 40,
            top,
            band_w.max(6),
            (bot - top).max(2),
            colors::PASP_LIGHT,
        );
    }
}

fn paint_gradient(
    rgba: &mut [u8],
    stride_w: u32,
    stride_h: u32,
    origin_x: i32,
    origin_y: i32,
    status: &EtcsStatus,
    max_m: f64,
) {
    // Gradient column ~x=115 (OR CreateGradient).
    let gx = origin_x + 115;
    for i in 0..status.gradient.len().saturating_sub(1) {
        let a = status.gradient[i];
        let b = status.gradient[i + 1];
        let d0 = a.distance_m.min(max_m);
        let d1 = b.distance_m.min(max_m);
        if d0 >= max_m {
            break;
        }
        let y0 = origin_y + planning_height(d0, max_m);
        let y1 = origin_y + planning_height(d1, max_m);
        let top = y1.min(y0);
        let h = (y0 - y1).abs().max(2);
        let color = if a.grade_permille >= 0 {
            colors::GREY
        } else {
            colors::DARK_GREY
        };
        fill_rect(rgba, stride_w, stride_h, gx, top, 18, h, color);
        let label = format!("{}", a.grade_permille.abs());
        let mut lx = gx + 2;
        for ch in label.chars().take(3) {
            blit_digit3x5(
                rgba,
                stride_w,
                stride_h,
                lx,
                top + 2,
                5,
                8,
                ch,
                colors::BLACK,
            );
            lx += 6;
        }
    }
}

/// OR `GetPlanningHeight` — y relative to planning area top (0..283).
pub fn planning_height(distance_m: f64, max_m: f64) -> i32 {
    let first_line = LINE_DIST[1] * max_m / 1000.0;
    if distance_m < first_line {
        LINE_POS[0]
            - ((LINE_POS[0] - LINE_POS[1]) as f64 / first_line.max(1.0) * distance_m) as i32
    } else {
        let log_span = (max_m / first_line.max(1.0)).log10().max(1e-6);
        let t = (distance_m / first_line.max(1.0)).log10() / log_span;
        LINE_POS[1] - ((LINE_POS[1] - LINE_POS[8]) as f64 * t) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_height_order() {
        let y0 = planning_height(0.0, 4000.0);
        let y1 = planning_height(4000.0, 4000.0);
        assert!(y0 > y1);
    }
}

//! Circular speed gauge (OR `CircularSpeedGauge` CPU subset).

use super::colors::{self, Rgba};
use super::paint::{blit_digit3x5, fill_rect, stroke_circle, stroke_line};
use super::status::{EtcsMonitor, EtcsStatus, EtcsSupervision};

/// Local rect: OR place at (54, 15), size 280×300.
pub const GAUGE_W: i32 = 280;
pub const GAUGE_H: i32 = 300;

const START_ANGLE: f32 = -144.0_f32.to_radians();
const END_ANGLE: f32 = 144.0_f32.to_radians();
const RADIUS_OUT: f32 = 125.0;
const RADIUS_TEXT: f32 = 99.0;
const LINE_FULL: f32 = 25.0;
const LINE_HALF: f32 = 15.0;

pub fn paint_circular_gauge(
    rgba: &mut [u8],
    stride_w: u32,
    stride_h: u32,
    origin_x: i32,
    origin_y: i32,
    status: &EtcsStatus,
) {
    fill_rect(
        rgba,
        stride_w,
        stride_h,
        origin_x,
        origin_y,
        GAUGE_W,
        GAUGE_H,
        colors::PANEL,
    );

    let max = status.dial_max_kmh as f32;
    let cx = origin_x as f32 + GAUGE_W as f32 / 2.0;
    let cy = origin_y as f32 + GAUGE_H as f32 / 2.0;

    // Dial ticks every 10 km/h, labels every 20 (or 50 on large scales).
    let label_step = if max >= 240.0 { 50 } else { 20 };
    let mut speed = 0i32;
    while speed <= status.dial_max_kmh as i32 {
        let angle = speed2angle(speed as f32, max);
        let (ox, oy) = polar(RADIUS_OUT, angle, cx, cy);
        let len = if speed % 20 == 0 {
            LINE_FULL
        } else {
            LINE_HALF
        };
        let (ix, iy) = polar(RADIUS_OUT - len, angle, cx, cy);
        stroke_line(
            rgba,
            stride_w,
            stride_h,
            ix as i32,
            iy as i32,
            ox as i32,
            oy as i32,
            colors::WHITE,
        );
        if speed % label_step == 0 {
            let (tx, ty) = polar(RADIUS_TEXT, angle, cx, cy);
            let label = speed.to_string();
            let digit_w = 8i32;
            let digit_h = 12i32;
            let mut x = tx as i32 - (label.len() as i32 * digit_w) / 2;
            let y = ty as i32 - digit_h / 2;
            for ch in label.chars() {
                blit_digit3x5(
                    rgba,
                    stride_w,
                    stride_h,
                    x,
                    y,
                    digit_w,
                    digit_h,
                    ch,
                    colors::WHITE,
                );
                x += digit_w + 1;
            }
        }
        speed += 10;
    }

    // Gauge arcs: target→allowed (yellow in TSM/RSM, white in CSM), then warn arc.
    let allowed = status.allowed_kmh as f32;
    let target = status.target_kmh.unwrap_or(status.allowed_kmh) as f32;
    let intervention = status.intervention_kmh as f32;
    let gauge_color = match status.monitor {
        EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed => colors::YELLOW,
        EtcsMonitor::CeilingSpeed => colors::WHITE,
    };
    let a0 = speed2angle(target.min(allowed), max);
    let a1 = speed2angle(allowed, max);
    let a2 = speed2angle(intervention.max(allowed), max);
    stroke_arc(
        rgba,
        stride_w,
        stride_h,
        cx,
        cy,
        RADIUS_OUT - 8.0,
        a0,
        a1,
        6.0,
        gauge_color,
    );
    let warn = if status.supervision == EtcsSupervision::Intervention {
        colors::RED
    } else {
        colors::ORANGE
    };
    // OR hides the orange/red arc while supervision is Normal.
    let show_warn_arc = matches!(
        status.supervision,
        EtcsSupervision::Overspeed | EtcsSupervision::Warning | EtcsSupervision::Intervention
    ) || status.overspeed;
    if show_warn_arc && intervention > allowed + 0.5 {
        stroke_arc(
            rgba,
            stride_w,
            stride_h,
            cx,
            cy,
            RADIUS_OUT - 8.0,
            a1,
            a2,
            6.0,
            warn,
        );
    }

    // Needle
    let needle_color = needle_color(status);
    let angle = speed2angle(status.speed_kmh as f32, max);
    let (nx, ny) = polar(RADIUS_OUT - 20.0, angle, cx, cy);
    stroke_line(
        rgba,
        stride_w,
        stride_h,
        cx as i32,
        cy as i32,
        nx as i32,
        ny as i32,
        needle_color,
    );
    // Needle hub
    stroke_circle(
        rgba,
        stride_w,
        stride_h,
        cx as i32,
        cy as i32,
        8,
        needle_color,
    );
    fill_rect(
        rgba,
        stride_w,
        stride_h,
        cx as i32 - 4,
        cy as i32 - 4,
        8,
        8,
        needle_color,
    );

    // Centre speed digits (3)
    let speed_text = format!("{:3}", (status.speed_kmh.round() as i32).clamp(0, 999));
    let digit_w = 22i32;
    let digit_h = 32i32;
    let text_color = if needle_color == colors::RED {
        colors::WHITE
    } else {
        colors::BLACK
    };
    // Digit background
    fill_rect(
        rgba,
        stride_w,
        stride_h,
        cx as i32 - 40,
        cy as i32 - 20,
        80,
        40,
        needle_color,
    );
    let mut dx = cx as i32 - 36;
    let dy = cy as i32 - 14;
    for ch in speed_text.chars() {
        if ch != ' ' {
            blit_digit3x5(
                rgba, stride_w, stride_h, dx, dy, digit_w, digit_h, ch, text_color,
            );
        }
        dx += digit_w + 2;
    }

    // Unit
    let unit = "km/h";
    let mut ux = cx as i32 - 18;
    let uy = cy as i32 + 28;
    for ch in unit.chars() {
        blit_digit3x5(rgba, stride_w, stride_h, ux, uy, 8, 10, ch, colors::GREY);
        ux += 9;
    }

    // Release speed digit (OR bottom-left of gauge ~26,274 local).
    if let Some(rel) = status.release_kmh {
        let txt = format!("{:.0}", rel);
        let mut rx = origin_x + 20;
        let ry = origin_y + GAUGE_H - 28;
        for ch in txt.chars() {
            blit_digit3x5(rgba, stride_w, stride_h, rx, ry, 12, 16, ch, colors::GREY);
            rx += 13;
        }
        // Thin release arc marker at release speed.
        let ar = speed2angle(rel as f32, max);
        stroke_arc(
            rgba,
            stride_w,
            stride_h,
            cx,
            cy,
            RADIUS_OUT - 14.0,
            ar - 0.03,
            ar + 0.03,
            4.0,
            colors::GREY,
        );
    }
}

/// Needle colours follow OR FS mode + `SupervisionStatus` / `Monitor`.
fn needle_color(status: &EtcsStatus) -> Rgba {
    // STM / SN: leave colour management to national system (OR TODO) — grey stub.
    if matches!(status.mode, super::status::EtcsMode::Sn) {
        return colors::GREY;
    }
    match status.supervision {
        EtcsSupervision::Intervention => colors::RED,
        EtcsSupervision::Warning | EtcsSupervision::Overspeed => colors::ORANGE,
        EtcsSupervision::Indication => colors::YELLOW,
        EtcsSupervision::Normal => match status.monitor {
            EtcsMonitor::TargetSpeed | EtcsMonitor::ReleaseSpeed => {
                let target = status.target_kmh.unwrap_or(status.allowed_kmh);
                if status.speed_kmh + 0.5 >= target && target + 0.5 < status.allowed_kmh {
                    colors::WHITE
                } else {
                    colors::GREY
                }
            }
            EtcsMonitor::CeilingSpeed => colors::GREY,
        },
    }
}

fn speed2angle(speed: f32, max_speed: f32) -> f32 {
    let t = (speed / max_speed.max(1.0)).clamp(0.0, 1.0);
    START_ANGLE + t * (END_ANGLE - START_ANGLE)
}

fn polar(radius: f32, angle: f32, cx: f32, cy: f32) -> (f32, f32) {
    // Zero angle = up; x right, y down (OR GetXY).
    let x = radius * angle.sin() + cx;
    let y = -radius * angle.cos() + cy;
    (x, y)
}

fn stroke_arc(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    cx: f32,
    cy: f32,
    radius: f32,
    a0: f32,
    a1: f32,
    thickness: f32,
    color: Rgba,
) {
    if (a1 - a0).abs() < 1e-4 {
        return;
    }
    let (lo, hi) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
    let steps = (((hi - lo).abs() / 0.02).ceil() as i32).max(2);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = lo + (hi - lo) * t;
        let (x0, y0) = polar(radius - thickness * 0.5, a, cx, cy);
        let (x1, y1) = polar(radius + thickness * 0.5, a, cx, cy);
        stroke_line(
            rgba, w, h, x0 as i32, y0 as i32, x1 as i32, y1 as i32, color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_angle_monotonic() {
        let a0 = speed2angle(0.0, 140.0);
        let a1 = speed2angle(70.0, 140.0);
        let a2 = speed2angle(140.0, 140.0);
        assert!(a0 < a1 && a1 < a2);
    }
}

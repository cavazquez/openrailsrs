//! Full DMI layout painter (OR non-soft FullSize 640×480).

use super::colors::{self, Rgba};
use super::gauge::{self, paint_circular_gauge};
use super::planning::paint_planning;
use super::status::{EtcsStatus, EtcsSupervision};
use super::symbols::EtcsSymbols;

pub const DMI_W: u32 = 640;
pub const DMI_H: u32 = 480;

/// Paint a recognisable ERA-style DMI into `rgba` (row-major RGBA8).
pub fn paint_dmi_full(rgba: &mut [u8], w: u32, h: u32, status: &EtcsStatus) {
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return;
    }
    if !status.active {
        fill_rect(rgba, w, h, 0, 0, w as i32, h as i32, colors::BG);
        return;
    }

    // Scale logical 640×480 into actual buffer (usually 1:1).
    let sx = w as f32 / DMI_W as f32;
    let sy = h as f32 / DMI_H as f32;
    if (sx - 1.0).abs() > 0.01 || (sy - 1.0).abs() > 0.01 {
        // Non-1:1: paint into temp 640×480 then nearest-neighbour scale.
        let mut tmp = vec![0u8; (DMI_W * DMI_H * 4) as usize];
        paint_dmi_full_1x(&mut tmp, status);
        scale_nearest(&tmp, DMI_W, DMI_H, rgba, w, h);
        return;
    }
    paint_dmi_full_1x(rgba, status);
}

fn paint_dmi_full_1x(rgba: &mut [u8], status: &EtcsStatus) {
    let w = DMI_W;
    let h = DMI_H;
    let symbols = EtcsSymbols::global();
    fill_rect(rgba, w, h, 0, 0, w as i32, h as i32, colors::BG);

    // Top margin strip
    fill_rect(rgba, w, h, 0, 0, 640, 15, colors::BG);

    // TTI / LSSM (0,15) 54×54 — OR `TTIandLSSMArea`
    paint_tti(rgba, w, h, 0, 15, status);

    // Target distance column (0, 69) 54×221
    paint_target_distance(rgba, w, h, 0, 69, 54, 221, status);

    // Circular gauge (54, 15)
    paint_circular_gauge(rgba, w, h, 54, 15, status);
    stroke_rect(rgba, w, h, 54, 15, gauge::GAUGE_W, gauge::GAUGE_H, colors::FRAME);

    // Message area (54, 365) 234×100 + scroll soft keys
    paint_message_area(rgba, w, h, status, symbols);

    // Planning (334, 15)
    paint_planning(rgba, w, h, 334, 15, status, symbols);
    stroke_rect(rgba, w, h, 334, 15, 246, 300, colors::FRAME);

    // Scale soft keys (NA_03/04 enabled)
    fill_rect(rgba, w, h, 334, 15, 40, 15, colors::PANEL);
    stroke_rect(rgba, w, h, 334, 15, 40, 15, colors::FRAME);
    if !symbols.blit_centered(rgba, w, h, 334, 15, 40, 15, "NA_03.bmp") {
        fill_rect(rgba, w, h, 334, 15, 40, 15, colors::GREY);
    }
    fill_rect(rgba, w, h, 334, 300, 40, 15, colors::PANEL);
    stroke_rect(rgba, w, h, 334, 300, 40, 15, colors::FRAME);
    if !symbols.blit_centered(rgba, w, h, 334, 300, 40, 15, "NA_04.bmp") {
        fill_rect(rgba, w, h, 334, 300, 40, 15, colors::GREY);
    }

    // Right menu bar soft keys (text; TCS icons TBD)
    paint_soft_keys(rgba, w, h, status);

    // Mode label FS in TTI cell when no TTI square
    if status.tti_indication_s.is_none() && status.tti_permitted_s.is_none() {
        blit_digit3x5(rgba, w, h, 8, 28, 10, 14, 'F', colors::WHITE);
        blit_digit3x5(rgba, w, h, 20, 28, 10, 14, 'S', colors::WHITE);
    }
}

/// OR `TTIandLSSMArea`: growing coloured square, `T_dispTTI = 14` s, 10 bands × 5 px.
fn paint_tti(rgba: &mut [u8], w: u32, h: u32, x: i32, y: i32, status: &EtcsStatus) {
    fill_rect(rgba, w, h, x, y, 54, 54, colors::PANEL);
    stroke_rect(rgba, w, h, x, y, 54, 54, colors::FRAME);

    let (tti, color, csm_white) = if let Some(t) = status.tti_permitted_s {
        let c = match status.supervision {
            EtcsSupervision::Intervention => colors::RED,
            EtcsSupervision::Warning | EtcsSupervision::Overspeed => colors::ORANGE,
            _ => colors::YELLOW,
        };
        (Some(t), c, false)
    } else if let Some(t) = status.tti_indication_s {
        (Some(t), colors::WHITE, true)
    } else {
        (None, colors::WHITE, false)
    };

    let Some(tti) = tti else {
        return;
    };
    const T_DISP: f64 = 14.0;
    let mut width = 0i32;
    for n in 1..=10 {
        let lo = T_DISP * f64::from(10 - n) / 10.0;
        let hi = T_DISP * f64::from(10 - (n - 1)) / 10.0;
        if tti >= lo && tti < hi {
            width = 5 * n;
            break;
        }
    }
    if width <= 0 {
        return;
    }
    if csm_white {
        fill_rect(rgba, w, h, x, y, 54, 54, colors::DARK_GREY);
    }
    let ox = x + (54 - width) / 2;
    let oy = y + (54 - width) / 2;
    fill_rect(rgba, w, h, ox, oy, width, width, color);
}

fn paint_message_area(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    status: &EtcsStatus,
    symbols: &EtcsSymbols,
) {
    fill_rect(rgba, w, h, 54, 365, 234, 100, colors::PANEL);
    stroke_rect(rgba, w, h, 54, 365, 234, 100, colors::FRAME);

    // Up to 5 rows × 20 px (OR Full layout).
    let lines: Vec<&str> = status.messages.iter().rev().take(5).map(|s| s.as_str()).collect();
    for (i, line) in lines.iter().rev().enumerate() {
        let ly = 365 + 4 + (i as i32) * 18;
        blit_text(rgba, w, h, 60, ly, 8, 12, line, colors::GREY);
    }

    fill_rect(rgba, w, h, 288, 365, 46, 50, colors::PANEL);
    fill_rect(rgba, w, h, 288, 415, 46, 50, colors::PANEL);
    stroke_rect(rgba, w, h, 288, 365, 46, 50, colors::FRAME);
    stroke_rect(rgba, w, h, 288, 415, 46, 50, colors::FRAME);
    let _ = symbols.blit_centered(rgba, w, h, 288, 365, 46, 50, "NA_13.bmp");
    let _ = symbols.blit_centered(rgba, w, h, 288, 415, 46, 50, "NA_14.bmp");
}

fn paint_soft_keys(rgba: &mut [u8], w: u32, h: u32, status: &EtcsStatus) {
    for i in 0..6 {
        let y = 15 + 50 * i;
        fill_rect(rgba, w, h, 580, y, 60, 48, colors::PANEL);
        stroke_rect(rgba, w, h, 580, y, 60, 48, colors::FRAME);
        if let Some(label) = status.soft_keys.get(i as usize) {
            if !label.is_empty() {
                blit_text(rgba, w, h, 586, y + 18, 7, 10, label, colors::GREY);
            }
        }
    }
}

pub(crate) fn blit_text(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    mut x: i32,
    y: i32,
    dw: i32,
    dh: i32,
    text: &str,
    c: Rgba,
) {
    for ch in text.chars().take(28) {
        blit_digit3x5(rgba, w, h, x, y, dw, dh, ch, c);
        x += dw + 1;
    }
}

fn paint_target_distance(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    bw: i32,
    bh: i32,
    status: &EtcsStatus,
) {
    fill_rect(rgba, w, h, x, y, bw, bh, colors::PANEL);
    stroke_rect(rgba, w, h, x, y, bw, bh, colors::FRAME);
    let Some(dist) = status.target_distance_m else {
        return;
    };
    // Bar fill from bottom: log-ish 0–4000 m
    let t = (dist / 4000.0).clamp(0.0, 1.0);
    let fill_h = ((1.0 - t) * (bh - 8) as f64) as i32;
    fill_rect(
        rgba,
        w,
        h,
        x + 12,
        y + bh - 4 - fill_h,
        bw - 24,
        fill_h.max(2),
        colors::YELLOW,
    );
    let label = if dist >= 1000.0 {
        format!("{:.1}", dist / 1000.0)
    } else {
        format!("{:.0}", dist)
    };
    let mut lx = x + 6;
    let ly = y + 8;
    for ch in label.chars().take(5) {
        blit_digit3x5(rgba, w, h, lx, ly, 8, 12, ch, colors::WHITE);
        lx += 9;
    }
}

pub(crate) fn fill_rect(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: i32,
    rh: i32,
    c: Rgba,
) {
    if rw <= 0 || rh <= 0 {
        return;
    }
    let x1 = x.max(0) as u32;
    let y1 = y.max(0) as u32;
    let x2 = ((x + rw) as u32).min(w);
    let y2 = ((y + rh) as u32).min(h);
    for yy in y1..y2 {
        for xx in x1..x2 {
            let i = ((yy * w + xx) * 4) as usize;
            if i + 3 < rgba.len() {
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    }
}

pub(crate) fn stroke_rect(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: i32,
    rh: i32,
    c: Rgba,
) {
    let t = 1i32;
    fill_rect(rgba, w, h, x, y, rw, t, c);
    fill_rect(rgba, w, h, x, y + rh - t, rw, t, c);
    fill_rect(rgba, w, h, x, y, t, rh, c);
    fill_rect(rgba, w, h, x + rw - t, y, t, rh, c);
}

pub(crate) fn stroke_line(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    c: Rgba,
) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    let mut x = x0;
    let mut y = y0;
    loop {
        put_pixel(rgba, w, h, x, y, c);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x += sx;
        }
        if e2 < dx {
            err += dx;
            y += sy;
        }
    }
}

pub(crate) fn stroke_circle(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    cx: i32,
    cy: i32,
    r: i32,
    c: Rgba,
) {
    let mut x = r;
    let mut y = 0;
    let mut err = 0;
    while x >= y {
        put_pixel(rgba, w, h, cx + x, cy + y, c);
        put_pixel(rgba, w, h, cx + y, cy + x, c);
        put_pixel(rgba, w, h, cx - y, cy + x, c);
        put_pixel(rgba, w, h, cx - x, cy + y, c);
        put_pixel(rgba, w, h, cx - x, cy - y, c);
        put_pixel(rgba, w, h, cx - y, cy - x, c);
        put_pixel(rgba, w, h, cx + y, cy - x, c);
        put_pixel(rgba, w, h, cx + x, cy - y, c);
        y += 1;
        err += 1 + 2 * y;
        if 2 * (err - x) + 1 > 0 {
            x -= 1;
            err += 1 - 2 * x;
        }
    }
}

fn put_pixel(rgba: &mut [u8], w: u32, h: u32, x: i32, y: i32, c: Rgba) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= w || y >= h {
        return;
    }
    let i = ((y * w + x) * 4) as usize;
    if i + 3 < rgba.len() {
        rgba[i..i + 4].copy_from_slice(&c);
    }
}

pub(crate) fn blit_digit3x5(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    dw: i32,
    dh: i32,
    ch: char,
    c: Rgba,
) {
    let glyph = glyph(ch);
    for row in 0..5i32 {
        for col in 0..3i32 {
            if (glyph[row as usize] >> (2 - col)) & 1 == 0 {
                continue;
            }
            let px = x + col * dw / 3;
            let py = y + row * dh / 5;
            fill_rect(
                rgba,
                w,
                h,
                px,
                py,
                (dw / 3).max(1),
                (dh / 5).max(1),
                c,
            );
        }
    }
}

fn glyph(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
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
        '-' | '.' => [0b000, 0b000, 0b111, 0b000, 0b000],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b111],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        'R' => [0b111, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        ' ' => [0b000, 0b000, 0b000, 0b000, 0b000],
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
    }
}

fn scale_nearest(
    src: &[u8],
    sw: u32,
    sh: u32,
    dst: &mut [u8],
    dw: u32,
    dh: u32,
) {
    for y in 0..dh {
        let sy = y * sh / dh;
        for x in 0..dw {
            let sx = x * sw / dw;
            let si = ((sy * sw + sx) * 4) as usize;
            let di = ((y * dw + x) * 4) as usize;
            if si + 3 < src.len() && di + 3 < dst.len() {
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etcs::status::EtcsStatus;

    #[test]
    fn full_paint_marks_gauge_and_planning() {
        let mut rgba = vec![0u8; (DMI_W * DMI_H * 4) as usize];
        let status = EtcsStatus::from_telemetry(72.0, 100.0, false, Some(1200.0));
        paint_dmi_full(&mut rgba, DMI_W, DMI_H, &status);
        // Background pixel
        assert_eq!(&rgba[0..3], &colors::BG[0..3]);
        // Gauge panel region should differ from pure BG
        let gi = (((15 + 150) * DMI_W + (54 + 140)) * 4) as usize;
        assert_ne!(&rgba[gi..gi + 3], &colors::BG[0..3]);
        // Planning PASP
        let pi = (((15 + 10) * DMI_W + (334 + 50)) * 4) as usize;
        assert_ne!(&rgba[pi..pi + 3], &colors::BG[0..3]);
    }

    #[test]
    fn tti_square_painted_when_close() {
        let mut rgba = vec![0u8; (DMI_W * DMI_H * 4) as usize];
        let status = EtcsStatus::from_telemetry(72.0, 100.0, false, Some(100.0));
        paint_dmi_full(&mut rgba, DMI_W, DMI_H, &status);
        // Centre of TTI cell should not stay plain PANEL if TTI is active.
        let ci = (((15 + 27) * DMI_W + 27) * 4) as usize;
        assert!(
            status.tti_permitted_s.is_some() || status.tti_indication_s.is_some(),
            "expected TTI in status"
        );
        assert_ne!(&rgba[ci..ci + 3], &colors::PANEL[0..3]);
    }
}

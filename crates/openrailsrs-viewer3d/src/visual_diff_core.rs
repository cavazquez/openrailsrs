//! Shared PNG compare helpers for visual regression (#43 / #71).

use image::{Rgba, RgbaImage};

#[derive(Clone, Debug)]
pub struct DiffThresholds {
    /// Per-channel Δ that marks a pixel "hot".
    pub tol: u8,
    /// Fail when hot pixels exceed this percent of the image.
    pub max_hot_pct: f32,
}

impl Default for DiffThresholds {
    fn default() -> Self {
        Self {
            tol: 16,
            max_hot_pct: 2.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiffSummary {
    pub width: u32,
    pub height: u32,
    pub hot: u64,
    pub total: u64,
    pub hot_pct: f64,
    pub rmse: f64,
    pub ok: bool,
}

fn channel_delta(a: u8, b: u8) -> u8 {
    a.abs_diff(b)
}

pub fn pixel_hot(a: Rgba<u8>, b: Rgba<u8>, tol: u8) -> bool {
    channel_delta(a[0], b[0]) > tol
        || channel_delta(a[1], b[1]) > tol
        || channel_delta(a[2], b[2]) > tol
}

/// Compare two same-size RGBA images. Returns `None` if dimensions differ.
pub fn compare_rgba(
    actual: &RgbaImage,
    golden: &RgbaImage,
    thresholds: &DiffThresholds,
    mut diff_out: Option<&mut RgbaImage>,
) -> Option<DiffSummary> {
    if actual.dimensions() != golden.dimensions() {
        return None;
    }
    let (w, h) = actual.dimensions();
    let total = (w as u64) * (h as u64);
    let mut hot: u64 = 0;
    let mut sum_sq: f64 = 0.0;

    for y in 0..h {
        for x in 0..w {
            let a = *actual.get_pixel(x, y);
            let b = *golden.get_pixel(x, y);
            let dr = channel_delta(a[0], b[0]) as f64;
            let dg = channel_delta(a[1], b[1]) as f64;
            let db = channel_delta(a[2], b[2]) as f64;
            sum_sq += dr * dr + dg * dg + db * db;
            let is_hot = pixel_hot(a, b, thresholds.tol);
            if is_hot {
                hot += 1;
            }
            if let Some(ref mut out) = diff_out {
                if is_hot {
                    out.put_pixel(x, y, Rgba([255, 32, 32, 255]));
                } else {
                    out.put_pixel(x, y, Rgba([a[0] / 3, a[1] / 3, a[2] / 3, 255]));
                }
            }
        }
    }

    let hot_pct = if total == 0 {
        0.0
    } else {
        (hot as f64) * 100.0 / (total as f64)
    };
    let rmse = if total == 0 {
        0.0
    } else {
        (sum_sq / ((total as f64) * 3.0)).sqrt()
    };
    let ok = hot_pct <= f64::from(thresholds.max_hot_pct);
    Some(DiffSummary {
        width: w,
        height: h,
        hot,
        total,
        hot_pct,
        rmse,
        ok,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    /// Synthetic "train" blob on a gray platform (stand-in for exterior capture).
    fn scene_with_train(w: u32, h: u32, train_w: u32, train_h: u32, cx: u32, cy: u32) -> RgbaImage {
        let mut img = solid(w, h, [120, 120, 120, 255]);
        let x0 = cx.saturating_sub(train_w / 2);
        let y0 = cy.saturating_sub(train_h / 2);
        for y in y0..(y0 + train_h).min(h) {
            for x in x0..(x0 + train_w).min(w) {
                img.put_pixel(x, y, Rgba([40, 40, 160, 255]));
            }
        }
        img
    }

    #[test]
    fn identical_images_pass() {
        let a = scene_with_train(64, 36, 20, 8, 32, 18);
        let b = a.clone();
        let s = compare_rgba(&a, &b, &DiffThresholds::default(), None).expect("same size");
        assert!(s.ok, "hot_pct={}", s.hot_pct);
        assert_eq!(s.hot, 0);
    }

    #[test]
    fn scale_train_1_5x_fails_diff() {
        // #71 AC: detect train scaled ×1.5 via hot-pixel budget.
        let golden = scene_with_train(64, 36, 20, 8, 32, 18);
        let scaled = scene_with_train(64, 36, 30, 12, 32, 18); // ×1.5
        let s =
            compare_rgba(&scaled, &golden, &DiffThresholds::default(), None).expect("same size");
        assert!(
            !s.ok,
            "scaled train must exceed hot budget (hot_pct={})",
            s.hot_pct
        );
        assert!(s.hot_pct > 2.0);
    }

    #[test]
    fn sink_train_5m_equivalent_fails_diff() {
        // #71 AC: detect ~5 m vertical sink (≈ several pixels at smoke/orbit framing).
        let golden = scene_with_train(64, 36, 20, 8, 32, 18);
        let sunk = scene_with_train(64, 36, 20, 8, 32, 26); // +8 px ≈ sink
        let s = compare_rgba(&sunk, &golden, &DiffThresholds::default(), None).expect("same size");
        assert!(
            !s.ok,
            "sunk train must exceed hot budget (hot_pct={})",
            s.hot_pct
        );
    }

    #[test]
    fn dimension_mismatch_returns_none() {
        let a = solid(8, 8, [0, 0, 0, 255]);
        let b = solid(16, 8, [0, 0, 0, 255]);
        assert!(compare_rgba(&a, &b, &DiffThresholds::default(), None).is_none());
    }
}

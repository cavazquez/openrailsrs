//! Compare two PNGs for visual regression (#43).
//!
//! Exit codes:
//! - 0: within thresholds
//! - 1: dimensions differ, too many hot pixels, or I/O error
//! - 2: usage error

use std::path::PathBuf;
use std::process::ExitCode;

use image::{Rgba, RgbaImage};

fn usage() -> ! {
    eprintln!(
        "Usage: openrailsrs-visual-diff <actual.png> <golden.png> [--diff <out.png>] [--tol <0-255>] [--max-hot-pct <0-100>]"
    );
    std::process::exit(2);
}

fn parse_args() -> (PathBuf, PathBuf, Option<PathBuf>, u8, f32) {
    let mut args = std::env::args().skip(1);
    let Some(actual) = args.next() else {
        usage();
    };
    let Some(golden) = args.next() else {
        usage();
    };
    let mut diff_out = None;
    let mut tol: u8 = 16;
    let mut max_hot_pct: f32 = 2.0;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--diff" => {
                let Some(p) = args.next() else { usage() };
                diff_out = Some(PathBuf::from(p));
            }
            "--tol" => {
                let Some(v) = args.next() else { usage() };
                tol = v.parse().unwrap_or_else(|_| usage());
            }
            "--max-hot-pct" => {
                let Some(v) = args.next() else { usage() };
                max_hot_pct = v.parse().unwrap_or_else(|_| usage());
            }
            _ => usage(),
        }
    }
    (
        PathBuf::from(actual),
        PathBuf::from(golden),
        diff_out,
        tol,
        max_hot_pct,
    )
}

fn channel_delta(a: u8, b: u8) -> u8 {
    a.abs_diff(b)
}

fn pixel_hot(a: Rgba<u8>, b: Rgba<u8>, tol: u8) -> bool {
    channel_delta(a[0], b[0]) > tol
        || channel_delta(a[1], b[1]) > tol
        || channel_delta(a[2], b[2]) > tol
}

fn main() -> ExitCode {
    let (actual_path, golden_path, diff_out, tol, max_hot_pct) = parse_args();

    let actual = match image::open(&actual_path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("error: cannot open actual {}: {e}", actual_path.display());
            return ExitCode::from(1);
        }
    };
    let golden = match image::open(&golden_path) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("error: cannot open golden {}: {e}", golden_path.display());
            return ExitCode::from(1);
        }
    };

    if actual.dimensions() != golden.dimensions() {
        eprintln!(
            "FAIL: resolution mismatch actual={}x{} golden={}x{}",
            actual.width(),
            actual.height(),
            golden.width(),
            golden.height()
        );
        return ExitCode::from(1);
    }

    let (w, h) = actual.dimensions();
    let total = (w as u64) * (h as u64);
    let mut hot: u64 = 0;
    let mut sum_sq: f64 = 0.0;
    let mut diff_img: Option<RgbaImage> = diff_out.as_ref().map(|_| RgbaImage::new(w, h));

    for y in 0..h {
        for x in 0..w {
            let a = *actual.get_pixel(x, y);
            let b = *golden.get_pixel(x, y);
            let dr = channel_delta(a[0], b[0]) as f64;
            let dg = channel_delta(a[1], b[1]) as f64;
            let db = channel_delta(a[2], b[2]) as f64;
            sum_sq += dr * dr + dg * dg + db * db;
            let is_hot = pixel_hot(a, b, tol);
            if is_hot {
                hot += 1;
            }
            if let Some(ref mut out) = diff_img {
                if is_hot {
                    out.put_pixel(x, y, Rgba([255, 32, 32, 255]));
                } else {
                    // Dim actual for context.
                    out.put_pixel(
                        x,
                        y,
                        Rgba([a[0] / 3, a[1] / 3, a[2] / 3, 255]),
                    );
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

    println!("visual-diff summary");
    println!("  actual:     {}", actual_path.display());
    println!("  golden:     {}", golden_path.display());
    println!("  size:       {w}x{h}");
    println!("  tol:        {tol}/255 per channel");
    println!("  hot pixels: {hot}/{total} ({hot_pct:.3}%)");
    println!("  rmse:       {rmse:.3}");
    println!("  max hot %:  {max_hot_pct}");

    if let (Some(path), Some(img)) = (diff_out, diff_img) {
        if let Err(e) = img.save(&path) {
            eprintln!("warning: could not write diff PNG {}: {e}", path.display());
        } else {
            println!("  diff png:   {}", path.display());
        }
    }

    if hot_pct > f64::from(max_hot_pct) {
        eprintln!("FAIL: hot pixel % {hot_pct:.3} > {max_hot_pct}");
        return ExitCode::from(1);
    }
    println!("OK");
    ExitCode::SUCCESS
}

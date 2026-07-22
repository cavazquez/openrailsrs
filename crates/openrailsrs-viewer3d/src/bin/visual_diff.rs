//! Compare two PNGs for visual regression (#43 / #71).
//!
//! Exit codes:
//! - 0: within thresholds
//! - 1: dimensions differ, too many hot pixels, or I/O error
//! - 2: usage error

use std::path::PathBuf;
use std::process::ExitCode;

use image::RgbaImage;
use openrailsrs_viewer3d::visual_diff_core::{DiffThresholds, compare_rgba};

fn usage() -> ! {
    eprintln!(
        "Usage: openrailsrs-visual-diff <actual.png> <golden.png> [--diff <out.png>] [--tol <0-255>] [--max-hot-pct <0-100>]"
    );
    std::process::exit(2);
}

fn parse_args() -> (PathBuf, PathBuf, Option<PathBuf>, DiffThresholds) {
    let mut args = std::env::args().skip(1);
    let Some(actual) = args.next() else {
        usage();
    };
    let Some(golden) = args.next() else {
        usage();
    };
    let mut diff_out = None;
    let mut thresholds = DiffThresholds::default();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--diff" => {
                let Some(p) = args.next() else { usage() };
                diff_out = Some(PathBuf::from(p));
            }
            "--tol" => {
                let Some(v) = args.next() else { usage() };
                thresholds.tol = v.parse().unwrap_or_else(|_| usage());
            }
            "--max-hot-pct" => {
                let Some(v) = args.next() else { usage() };
                thresholds.max_hot_pct = v.parse().unwrap_or_else(|_| usage());
            }
            _ => usage(),
        }
    }
    (
        PathBuf::from(actual),
        PathBuf::from(golden),
        diff_out,
        thresholds,
    )
}

fn main() -> ExitCode {
    let (actual_path, golden_path, diff_out, thresholds) = parse_args();

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

    let mut diff_img: Option<RgbaImage> = diff_out
        .as_ref()
        .map(|_| RgbaImage::new(actual.width(), actual.height()));

    let Some(summary) = compare_rgba(&actual, &golden, &thresholds, diff_img.as_mut()) else {
        eprintln!(
            "FAIL: resolution mismatch actual={}x{} golden={}x{}",
            actual.width(),
            actual.height(),
            golden.width(),
            golden.height()
        );
        return ExitCode::from(1);
    };

    println!("visual-diff summary");
    println!("  actual:     {}", actual_path.display());
    println!("  golden:     {}", golden_path.display());
    println!("  size:       {}x{}", summary.width, summary.height);
    println!("  tol:        {}/255 per channel", thresholds.tol);
    println!(
        "  hot pixels: {}/{} ({:.3}%)",
        summary.hot, summary.total, summary.hot_pct
    );
    println!("  rmse:       {:.3}", summary.rmse);
    println!("  max hot %:  {}", thresholds.max_hot_pct);

    if let (Some(path), Some(img)) = (diff_out, diff_img) {
        if let Err(e) = img.save(&path) {
            eprintln!("warning: could not write diff PNG {}: {e}", path.display());
        } else {
            println!("  diff png:   {}", path.display());
        }
    }

    if !summary.ok {
        eprintln!(
            "FAIL: hot pixel % {:.3} > {}",
            summary.hot_pct, thresholds.max_hot_pct
        );
        return ExitCode::from(1);
    }
    println!("OK");
    ExitCode::SUCCESS
}

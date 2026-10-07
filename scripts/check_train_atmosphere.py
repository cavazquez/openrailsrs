#!/usr/bin/env python3
"""Capture Hanabi precipitation and dense day/night fog on a real GPU.

Original content remains external. The off/low/high frames share camera,
clock and weather; an unchanged image cannot pass as headlight scattering.
"""
import argparse
import json
from pathlib import Path
import re
from check_viewer_streaming import run_checkpoint


def validate_fog(report):
    fog = report.get("fog") or {}
    if not (report.get("renderer") or {}).get("hardware"):
        raise ValueError("Fog scattering test requires a hardware renderer")
    if not fog.get("volumetric") or fog.get("visibility_m", 1000) > 150:
        raise ValueError("Dense local fog is not active")
    if fog.get("density_factor", 0) < 0.04 or not fog.get("punctual_light_attenuation_corrected"):
        raise ValueError("Punctual-light extinction or density is unverified")
    if report.get("shader_pipelines") != {"pending": 0, "failed": 0}:
        raise ValueError("Fog shader pipelines are incomplete")


def compare_lights(off, illuminated):
    from PIL import Image, ImageChops, ImageStat
    with Image.open(off) as a, Image.open(illuminated) as b:
        if a.size != (1280, 720) or b.size != a.size:
            raise ValueError("Scattering pair needs identical 1280x720 frames")
        # Exclude the text panels and action bar, including H's own HUD text.
        area = (270, 100, 980, 690)
        a, b = a.convert("RGB").crop(area), b.convert("RGB").crop(area)
        difference = sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / 3
        increase = sum(ImageStat.Stat(b).mean) / 3 - sum(ImageStat.Stat(a).mean) / 3
        # Allow the tiny lamp discs, but reject a beam that turns a substantial
        # part of the view white and hides all nearby track detail.
        pixels_a = (getattr(a, "get_flattened_data", None) or a.getdata)()
        pixels_b = (getattr(b, "get_flattened_data", None) or b.getdata)()
        newly_clipped = sum(min(y) >= 245 and min(x) < 245
                            for x, y in zip(pixels_a, pixels_b)) / (a.width * a.height)
    if difference < 0.2 or increase <= 0.05:
        raise ValueError(f"Headlamps did not brighten the fog: difference={difference}, increase={increase}")
    if newly_clipped > .025:
        raise ValueError(f"Headlight scattering clips the nearby view white: fraction={newly_clipped}")
    return {"mean_absolute_rgb_difference": difference, "mean_rgb_increase": increase,
            "newly_clipped_white_fraction": newly_clipped,
            "scope": "lamp response in the fixed fog scene; not Open Rails pixel parity"}


def main():
    root = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root", required=True, type=Path)
    p.add_argument("--scenario", required=True, type=Path)
    p.add_argument("--viewer", type=Path, default=root / "target/debug/openrailsrs-viewer3d")
    p.add_argument("--out-dir", type=Path, default=root / "tmp/train-atmosphere")
    p.add_argument("--timeout-s", type=float, default=240)
    p.add_argument("--max-rss-mib", type=float, default=6144)
    args = p.parse_args()
    args.repo = root
    for key in ("route_root", "scenario", "viewer", "out_dir"):
        setattr(args, key, getattr(args, key).resolve())
    if not args.route_root.is_dir() or not args.scenario.is_file() or not args.viewer.is_file() or args.timeout_s < 30:
        p.error("requires original content, an existing viewer and at least 30 seconds")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.follow, args.autodrive, args.speed_mul = "orbit", 0, 1
    args.camera_yaw, args.camera_pitch, args.camera_distance = -0.8, .12, 32
    args.ready_frames, args.view_radius_m = 45, 450
    args.software, args.headless_wayland, args.require_hardware = False, True, True
    args.renderer, args.weather_execution, args.train_effect_execution = "gpu", "gpu", "gpu"
    args.particle_budget, args.fog_quality = 2048, "auto"
    reports = {}
    for weather in ("rain", "snow"):
        args.weather = weather
        r = run_checkpoint(args, weather, 0, True)
        particles = r.get("weather_particles") or {}
        if particles.get("gpu_backend") != "bevy_hanabi 0.19.0" or particles.get("gpu_seed_initializations") != 1 or particles.get("gpu_mesh_updates") != 0 or particles.get("gpu_simulation_delta_s") != 0:
            raise ValueError(f"Persistent precipitation or pause is unverified: {particles}")
        reports[weather] = r
    original = args.scenario.read_text()
    def profile(name, clock):
        text, count = re.subn(r"(?m)^start_time_s\s*=.*$", f"start_time_s = {clock}", original, count=1)
        if count != 1:
            raise ValueError("The original scenario needs an explicit start_time_s for day/night evidence")
        path = args.out_dir / f"{name}.toml"
        path.write_text(text)
        return path
    args.scenario, args.weather = profile("day", 35700.0), "fog"
    args.capture_headlights = 0
    day = run_checkpoint(args, "fog-day", 0, True)
    validate_fog(day)
    if day.get("clock_time_s") != 35700.0 or (day.get("solar_direction") or [0, -1])[1] <= 0:
        raise ValueError("Daytime fog scene did not use the daytime clock and sun")
    reports["fog-day"] = day
    night = profile("night", 82800.0)
    args.scenario, args.weather = night, "fog"
    for camera in ("orbit", "driver"):
        args.follow = camera
        for level, suffix in ((0, "off"), (1, "low"), (2, "high")):
            args.capture_headlights = level
            name = f"fog-{camera}-{suffix}"
            r = run_checkpoint(args, name, 0, True)
            validate_fog(r)
            if r.get("headlights") != level:
                raise ValueError("Capture did not apply the requested lamp level")
            if r.get("clock_time_s") != 82800.0:
                raise ValueError("Night fog scene did not use the fixed night clock")
            reports[name] = r
        reports[f"comparison-{camera}"] = compare_lights(args.out_dir / f"fog-{camera}-off.png", args.out_dir / f"fog-{camera}-high.png")
        reports[f"comparison-{camera}-low"] = compare_lights(args.out_dir / f"fog-{camera}-off.png", args.out_dir / f"fog-{camera}-low.png")
    (args.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")


if __name__ == "__main__":
    main()

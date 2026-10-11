#!/usr/bin/env python3
"""Measure a complete station journey in clear weather, heavy rain and snow.

Uses original installed content on an isolated hardware Weston surface. A speed
multiplier above one is an accelerated streaming/physics stress test, not a
claim about real-time driving FPS. Whole-journey histograms include streaming;
startup and the bounded detailed diagnostic window are reported separately.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import tomllib

from benchmark_viewer import comparison, validate
from check_viewer_streaming import run_checkpoint


def main():
    repo = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--scenario", type=Path, required=True)
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d")
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--case", action="append", choices=("clear", "downpour", "heavy_snow"))
    p.add_argument("--speed-mul", type=float, default=1.0)
    p.add_argument("--timeout-s", type=int, default=3600)
    p.add_argument("--particle-budget", type=int, default=2048)
    p.add_argument("--formation-cars", type=int)
    args = p.parse_args()
    if not 0 < args.speed_mul <= 32 or not 128 <= args.particle_budget <= 8192:
        p.error("Use speed 0–32 and a particle budget between 128 and 8192")
    args.scenario = args.scenario.resolve()
    args.route_root = args.route_root.resolve()
    args.viewer = args.viewer.resolve()
    args.out_dir = args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    scenario = tomllib.loads(args.scenario.read_text())
    args.expected_station_names = [stop["name"] for stop in scenario["route"].get("stops", [])]
    if not args.expected_station_names:
        p.error("Choose a service with station stops")
    args.repo = repo
    args.software = False
    args.headless_wayland = True
    args.require_hardware = True
    args.renderer = "gpu"
    args.weather_execution = args.train_effect_execution = "gpu"
    args.after_service = True
    args.ready_frames = 180
    args.max_rss_mib = 6144
    args.autodrive = 0.75
    args.follow = "driver"
    args.dev_tools = 1
    args.weather_seed = 4242
    args.view_radius_m = 2000
    args.weather_quality = "medium"
    metadata = {
        "schema": 1, "scenario": scenario["scenario"]["name"],
        "scenario_sha256": hashlib.sha256(args.scenario.read_bytes()).hexdigest(),
        "viewer_sha256": hashlib.sha256(args.viewer.read_bytes()).hexdigest(),
        "resolution": [1280, 720], "speed_mul": args.speed_mul,
        "scope": "real-time complete service" if args.speed_mul == 1 else "accelerated complete-service stress test",
        "particle_budget": args.particle_budget, "weather_seed": args.weather_seed,
        "expected_stations": args.expected_station_names, "results": {},
    }
    for name in args.case or ("clear", "downpour", "heavy_snow"):
        config = copy.copy(args)
        config.weather = {"clear": "clear", "downpour": "rain", "heavy_snow": "snow"}[name]
        config.weather_profile = "automatic" if name == "clear" else name
        report = run_checkpoint(config, name, 0, False)
        validate(report, 120, config)
        journey = report.get("performance") or {}
        if journey.get("gameplay_frames", 0) < 180:
            raise ValueError("Whole-journey frame histogram is missing")
        metadata["results"][name] = {
            **comparison(report), "whole_journey": journey,
            "detailed_samples": report["dev_diagnostics"]["samples"],
            "detailed_scope": "last 12000 sampled gameplay frames, after 60 warmup frames",
            "odometer_m": report["odometer_m"], "service_complete": report["service_complete"],
            "stops": report["station_results"], "renderer": report["renderer"],
        }
        (args.out_dir / "comparison.json").write_text(json.dumps(metadata, indent=2) + "\n")


if __name__ == "__main__":
    main()

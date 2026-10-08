#!/usr/bin/env python3
"""Finish an original service under evolving weather and validate live contact.

Original route and train content stays outside Git. Renderer metrics describe
one machine and the requested time acceleration, not Open Rails pixel parity.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import tomllib

from check_viewer_streaming import run_checkpoint, scenario_service_distance


def validate_weather_journey(report, minimum_weather_types=3):
    journey = report.get("weather_journey") or {}
    samples = journey.get("samples") or []
    if not report.get("service_complete") or (report.get("drive_controls") or {}).get("missed_stops"):
        raise ValueError("The real service must finish without missed stations")
    if len(samples) < 20 or len(samples) > journey.get("sample_capacity", 0):
        raise ValueError("Journey observations are missing or unbounded")
    if len(journey.get("weather_frames") or {}) < minimum_weather_types:
        raise ValueError("The trip did not exercise enough effective weather conditions")
    if journey.get("maximum_grip_change_per_simulation_s", 1) > 0.045:
        raise ValueError("Wheel/rail grip jumped beyond the bounded wetting rate")
    if not 0.5 <= journey.get("minimum_rail_factor", 0) < 0.99:
        raise ValueError("Evolving weather did not reach wheel/rail contact")
    if journey.get("wet_braking_simulation_s", 0) <= 0:
        raise ValueError("No braking on wet rail was observed")
    last_clock, last_odometer = -math.inf, -math.inf
    contacts, moving_wet_braking = 0, 0
    for sample in samples:
        for key in ("simulation_s", "odometer_m", "speed_mps", "rain", "snow", "visibility_m", "applied_brake", "wheel_rail_brake_n"):
            if not isinstance(sample.get(key), (int, float)) or not math.isfinite(sample[key]):
                raise ValueError("A sampled simulation channel is non-finite or missing")
        if sample["simulation_s"] < last_clock or sample["odometer_m"] < last_odometer - 0.1:
            raise ValueError("Journey observations rewound during forward travel")
        last_clock, last_odometer = sample["simulation_s"], sample["odometer_m"]
        for key in ("rail_factor", "rail_target"):
            value = sample.get(key)
            if value is not None and not 0.5 <= value <= 1:
                raise ValueError("Wheel/rail contact escaped the normalized model bounds")
        contacts += sample.get("rail_target") is not None
        if (sample["speed_mps"] > 1 and sample["applied_brake"] > .01
                and sample["rail_factor"] is not None and sample["rail_factor"] < .99
                and sample["wheel_rail_brake_n"] > 0):
            moving_wet_braking += 1
    if contacts < len(samples) - 1:
        raise ValueError("Continuous contact was absent during the trip")
    if samples[-1]["completed_stops"] != len(report.get("station_results") or []):
        raise ValueError("The terminal observation did not include every completed stop")
    if not moving_wet_braking:
        raise ValueError("Wet braking was only observed while stationary or without rail force")
    return {"passed": True, "samples": len(samples), "effective_weather_types": len(journey["weather_frames"]),
            "minimum_rail_factor": journey["minimum_rail_factor"],
            "maximum_grip_change_per_simulation_s": journey["maximum_grip_change_per_simulation_s"],
            "wet_braking_simulation_s": journey["wet_braking_simulation_s"],
            "moving_wet_braking_samples": moving_wet_braking,
            "scope": "continuity and shared weather contact; no calibrated physical-parity claim"}


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--scenario", type=Path, required=True)
    parser.add_argument("--viewer", type=Path, default=root / "target/debug/openrailsrs-viewer3d")
    parser.add_argument("--out-dir", type=Path, default=root / "tmp/weather-journey")
    parser.add_argument("--timeout-s", type=float, default=1200)
    parser.add_argument("--max-rss-mib", type=float, default=6144)
    parser.add_argument("--view-radius-m", type=float, default=450)
    parser.add_argument("--speed-mul", type=float, default=16)
    parser.add_argument("--weather-seed", type=int, default=82)
    parser.add_argument("--weather-pace", choices=("slow", "normal", "fast"), default="normal")
    parser.add_argument("--weather-execution", choices=("auto", "gpu", "cpu", "hybrid"), default="gpu")
    parser.add_argument("--follow", choices=("driver", "orbit"), default="driver")
    parser.add_argument("--minimum-weather-types", type=int, default=3)
    args = parser.parse_args()
    args.repo = root
    for name in ("route_root", "scenario", "viewer", "out_dir"):
        setattr(args, name, getattr(args, name).resolve())
    if not args.route_root.is_dir() or not args.scenario.is_file() or not args.viewer.is_file():
        parser.error("requires original content, a scenario and a compiled viewer")
    if not 0.25 <= args.speed_mul <= 64 or not 0 <= args.weather_seed <= 0xffffffff or args.timeout_s < 30:
        parser.error("invalid time acceleration, weather sequence or timeout")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    scenario = tomllib.loads(args.scenario.read_text())
    args.expected_station_names = [stop["name"] for stop in scenario["route"].get("stops", [])]
    if not args.expected_station_names:
        parser.error("choose a service with scheduled stations")
    args.software, args.headless_wayland, args.require_hardware = False, True, True
    args.renderer, args.autodrive, args.ready_frames = "gpu", .75, 45
    args.weather, args.weather_profile, args.weather_quality = "clear", "random_journey", "high"
    args.train_effect_execution, args.fog_quality = "gpu", "auto"
    args.capture_wiper, args.capture_headlights = 1, 1
    distance = scenario_service_distance(args.scenario, scenario)
    report = run_checkpoint(args, "terminal", max(0, distance - 10), False)
    report["weather_journey_validation"] = validate_weather_journey(report, args.minimum_weather_types)
    report["viewer_sha256"] = hashlib.sha256(args.viewer.read_bytes()).hexdigest()
    report["scenario_sha256"] = hashlib.sha256(args.scenario.read_bytes()).hexdigest()
    (args.out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["weather_journey_validation"], indent=2))


if __name__ == "__main__":
    main()

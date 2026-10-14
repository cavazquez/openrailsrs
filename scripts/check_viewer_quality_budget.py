#!/usr/bin/env python3
"""Measure fixed F10 quality limits with identical weather and warmed content.

The first run seeds one private cache. Normal/reduced/normal then use that same
cache and fixed seed, camera and distance; neither physics nor weather is scaled.
The reduction is explicit in the report, rather than a hidden benchmark setting.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path

import check_player_expansion as expansion
import check_viewer_streaming as streaming


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", type=Path, required=True)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--checkpoint-m", type=float, default=2500)
    parser.add_argument("--reduced-level", type=int, choices=(1, 2, 3), default=2)
    parser.add_argument("--concurrency-note", default="")
    args = parser.parse_args()
    args.repo = Path(__file__).resolve().parents[1]
    args.viewer = args.viewer.resolve(strict=True)
    args.route_root = args.route_root.resolve(strict=True)
    args.out_dir = args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=False)
    scenario = args.repo / "examples/chiltern_extended/scenario.toml"
    results = {}
    for name, level in (
        ("priming-cold", 0),
        ("normal-warm", 0),
        ("reduced-warm", args.reduced_level),
        ("normal-repeat-warm", 0),
    ):
        expansion.settings(
            args, weather_profile="downpour", shadows=True,
            weather_particle_budget=2048,
            quality_limits={"enabled": True, "target_fps": 30,
                            "min_level": level, "max_level": level},
        )
        options = expansion.options(args, scenario, args.route_root)
        options.follow = "driver"
        options.audio_null = True
        options.dev_inspector = options.dev_tools = 0
        options.framepace = "off"
        options.autodrive = .5
        options.speed_mul = 4
        options.weather_profile = "downpour"
        options.weather_seed = 42
        options.profile_systems = True
        options.timeout_s = 300
        options.after_ready = True
        options.ready_frames = 30
        capture = streaming.run_checkpoint(options, name, args.checkpoint_m, pause=True)
        if (capture.get("pending_gpu_uploads") != 0
                or capture.get("unactivated_near_shapes") != 0):
            raise RuntimeError("Incomplete visible neighbourhood at a fixed quality limit")
        results[name] = {"quality_level": level, "capture": capture}
        report = {
            "schema": 1,
            "viewer_sha256": hashlib.sha256(args.viewer.read_bytes()).hexdigest(),
            "scenario_sha256": hashlib.sha256(scenario.read_bytes()).hexdigest(),
            "hardware": platform.uname()._asdict(),
            "configuration": {"weather": "downpour", "weather_seed": 42,
                              "camera": "driver", "window": [1280, 720],
                              "checkpoint_m": args.checkpoint_m, "view_distance_m": 500,
                              "autodrive": .5, "speed_multiplier": 4,
                              "configured_particles": 2048, "shadows": True,
                              "framepace": "off", "profile_systems": True,
                              "audio": "private ALSA null"},
            "concurrent_workloads": args.concurrency_note,
            "cache_scope": "first run seeds a fresh application cache; the three comparisons reuse that same cache, OS/Mesa caches are not cleared",
            "scope": "fixed PlayerSettings limits also exposed through F10; normal/reduced/normal controls repeat order; visual reduction is explicit and simulation/weather settings stay equal",
            "cases": results,
        }
        (args.out_dir / "quality-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(args.out_dir / "quality-report.json")


if __name__ == "__main__":
    main()

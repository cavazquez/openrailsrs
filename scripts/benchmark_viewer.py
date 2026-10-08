#!/usr/bin/env python3
"""Reproducible weather/VFX/pacing/scenery benchmarks using original content.

Each run uses an isolated player directory and compositor, bounded RSS, fixed
camera, clock, seeds and one viewer build. Raw reports accompany the comparison.
Input latency is the explicitly labelled ECS queue probe, not input-to-photon.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path

from check_viewer_streaming import run_checkpoint


def cases(suite):
    if suite == "pacing":
        return [("off", {"framepace": "off", "present_mode": "fifo", "camera_journey":"aba"}),
                ("30", {"framepace": "30", "present_mode": "auto_no_vsync", "camera_journey":"aba"}),
                ("60", {"framepace": "60", "present_mode": "auto_no_vsync", "camera_journey":"aba"}),
                ("unlimited", {"framepace": "unlimited", "present_mode": "auto_no_vsync", "camera_journey":"aba"})]
    if suite == "vfx":
        return [("off", {"train_effects_enabled": 0}),
                ("cpu", {"train_effect_execution": "cpu"}),
                ("gpu", {"train_effect_execution": "gpu"}),
                ("hybrid", {"train_effect_execution": "hybrid"})]
    if suite == "storm":
        return [(name, {"weather_profile": "storm_cycle", "weather": "storm", "weather_phase_s": phase})
                for name, phase in [("approaching", 40), ("active", 180), ("clearing", 420), ("clear", 540)]]
    if suite == "scenery":
        return [(name, {"scenery_profile": name, "camera_journey": "aba"})
                for name in ("authentic", "enhanced")]
    profiles = ["drizzle", "steady_rain", "downpour"] if suite == "rain" else ["light_snow", "heavy_snow", "after_snow"]
    return [(f"{profile}-{quality}", {"weather_profile": profile, "weather_quality": quality,
             "particle_budget": budget, "weather": "rain" if suite == "rain" else "snow"})
            for profile in profiles for quality, budget in [("low", 512), ("medium", 2048), ("high", 8192)]]


def validate(report, minimum_samples, config=None):
    diagnostics = report.get("dev_diagnostics") or {}
    if diagnostics.get("samples", 0) < minimum_samples:
        raise ValueError("Too few steady samples; build with dev-tools and increase --ready-frames")
    for key in ("p50", "p95", "p99"):
        value = diagnostics.get("frame_ms", {}).get(key)
        if value is None or not 0 < value < 4096:
            raise ValueError(f"Invalid frame {key}: {value}")
    if report.get("shader_pipelines") != {"pending": 0, "failed": 0}:
        raise ValueError("Shaders are not ready")
    if not report.get("renderer", {}).get("hardware"):
        raise ValueError("Hardware benchmark cannot use a software adapter")
    atmosphere = (report.get("weather_state") or {}).get("atmosphere", {})
    particles = report.get("weather_particles") or {}
    count = particles.get("cpu_particles", 0) + particles.get("gpu_particles", 0)
    if max(atmosphere.get("rain", 0), atmosphere.get("snow", 0)) > 0.05 and count == 0:
        raise ValueError("Precipitation is active but no particles are rendered")
    scenery = report.get("enhanced_scenery") or {}
    if scenery.get("instances", 0) > scenery.get("maximum_instances", 0):
        raise ValueError("Scenery exceeds its instance budget")
    if scenery.get("hash_mismatches", 0):
        raise ValueError("Vegetation changed when returning to a tile")
    if config is None:
        return
    if count > getattr(config, "particle_budget", 8192):
        raise ValueError("Precipitation exceeds the combined CPU/GPU budget")
    if getattr(config, "camera_journey", None) == "aba":
        journey = report.get("camera_journey") or {}
        if not journey.get("complete") or journey.get("distance_m", 0) < 6000:
            raise ValueError("Camera journey did not complete a streaming round trip")
        if scenery.get("profile") == "enhanced" and scenery.get("returned", 0) == 0:
            raise ValueError("Camera journey did not exercise regenerated vegetation")
    if getattr(config, "weather_profile", None) == "after_snow":
        if count or atmosphere.get("snow_cover", 0) < 0.9:
            raise ValueError("After-snow must retain coverage without falling particles")
    effects = report.get("train_effects") or {}
    if getattr(config, "train_effects_enabled", 1) == 0:
        if effects.get("enabled") or effects.get("cpu_capacity", 0) or effects.get("gpu_capacity", 0):
            raise ValueError("Disabled train effects still allocate particle buffers")
    elif getattr(config, "suite", None) == "vfx":
        from check_train_effects import validate as validate_emission
        validate_emission(report, config.train_effect_execution)


def comparison(report):
    metrics = report["dev_diagnostics"]
    memory = report.get("graphics_memory", {})
    return {"frame_ms": metrics["frame_ms"], "cpu_main_ms": metrics["cpu_main_ms"],
            "gpu_ms": metrics["gpu_ms"], "input_queue_ms": metrics["input_queue_ms"],
            "cpu_percent_one_core": report["host_usage"]["cpu_percent_one_core"],
            "gpu_busy_percent_mean": report["host_usage"]["gpu_busy_percent_mean"],
            "gpu_busy_percent_by_device": report["host_usage"].get("gpu_busy_percent_by_device"),
            "selected_gpu_pci": report["host_usage"].get("selected_gpu_pci"),
            "process_vram_mib": memory.get("process_vram_mib"),
            "peak_process_vram_mib": memory.get("peak_process_vram_mib"),
            "peak_rss_mib": report["peak_rss_mib"],
            "hitches_over_100_ms": metrics["hitches_over_100_ms"],
            "weather_particles": report.get("weather_particles"),
            "train_effects": report.get("train_effects"),
            "scenery": report.get("enhanced_scenery"),
            "draw_calls":report.get("draw_calls"),
            "camera_journey":report.get("camera_journey")}


def main():
    root = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--scenario", type=Path, required=True)
    p.add_argument("--viewer", type=Path, default=root / "target/debug/openrailsrs-viewer3d")
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--suite", choices=["pacing", "vfx", "rain", "snow", "storm", "scenery"], required=True)
    p.add_argument("--case", action="append", help="run only named cases")
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument("--ready-frames", type=int, default=900)
    p.add_argument("--timeout-s", type=int, default=300)
    p.add_argument("--max-rss-mib", type=int, default=6144)
    p.add_argument("--target-m", type=float, default=0)
    p.add_argument("--follow", choices=["orbit", "driver"], default="orbit")
    p.add_argument("--view-radius-m", type=float, default=450,
                   help="scenery radius; use 2000 for dense yards such as Paddington")
    p.add_argument("--camera-yaw", type=float, default=1.0)
    p.add_argument("--camera-pitch", type=float, default=0.52)
    p.add_argument("--camera-distance", type=float, default=75)
    p.add_argument("--fog-quality", choices=["auto", "distance", "volumetric32", "volumetric64"], default="auto")
    p.add_argument("--formation-cars", type=int, help="also verify this many player car roots and their spacing")
    p.add_argument("--weather", choices=["clear", "rain", "snow", "storm", "fog", "overcast"], default="clear")
    p.add_argument("--weather-execution", choices=["cpu", "gpu", "hybrid", "auto"], default="gpu")
    p.add_argument("--train-effect-execution", choices=["cpu", "gpu", "hybrid", "auto"], default="gpu")
    p.add_argument("--dev-inspector", type=int, choices=[0, 1], default=0)
    p.add_argument("--dev-inspector-select")
    p.add_argument("--dev-tools", type=int, choices=[0, 1], default=0)
    p.add_argument("--capture-wiper", type=int, choices=[0, 1], default=0)
    args = p.parse_args()
    if args.ready_frames < 180 or not 1 <= args.repeats <= 9:
        p.error("requires 180+ frames and 1–9 repeats")
    if not 0 < args.view_radius_m <= 16384 or args.camera_distance <= 0:
        p.error("requires a positive camera distance and scenery radius up to 16384 m")
    if args.formation_cars is not None and args.formation_cars < 1:
        p.error("--formation-cars must be positive")
    args.repo = root
    for name in ("route_root", "scenario", "viewer", "out_dir"):
        setattr(args, name, getattr(args, name).resolve())
    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.autodrive, args.speed_mul = 1 if args.target_m else 0, 4
    args.software, args.headless_wayland, args.require_hardware = False, True, True
    args.renderer, args.weather_seed = "gpu", 81
    args.weather_quality, args.present_mode, args.framepace = "high", "fifo", "off"
    args.train_effects_enabled = 1
    if args.suite == "vfx" and not args.target_m:
        p.error("VFX requires --target-m (a live emitter must have emitted before the paused comparison)")
    metadata = {
        "suite": args.suite, "repeats": args.repeats,
        "scenario_sha256": hashlib.sha256(args.scenario.read_bytes()).hexdigest(),
        "viewer_sha256": hashlib.sha256(args.viewer.read_bytes()).hexdigest(),
        "resolution": [1280, 720], "seed": args.weather_seed,
        "view_radius_m": args.view_radius_m,
        "camera": {"follow": args.follow, "yaw": args.camera_yaw,
                   "pitch": args.camera_pitch, "distance_m": args.camera_distance},
        "fog_quality": args.fog_quality,
        "weather_execution": args.weather_execution,
        "train_effect_execution": args.train_effect_execution,
        "target_m": args.target_m,
        "ready_frames": args.ready_frames,
        "formation_cars": args.formation_cars,
        "input_latency_scope": "synthetic queue to next ECS frame; excludes hardware/display",
        "presentation": "private Weston; off=fifo VSync, 30/60/unlimited=AutoNoVsync (compositor may cap)",
    }
    results = {}
    comparison_path = args.out_dir / "comparison.json"
    if comparison_path.is_file():
        previous = json.loads(comparison_path.read_text())
        if all(previous.get(key) == value for key, value in metadata.items()):
            results.update(previous.get("results", {}))
    selected = [item for item in cases(args.suite) if not args.case or item[0] in args.case]
    if not selected:
        p.error("No matching benchmark case")
    for repeat in range(args.repeats):
        for name, overrides in selected:
            config = copy.copy(args)
            for key, value in overrides.items():
                setattr(config, key, value)
            case = f"{name}-{repeat + 1}"
            report = run_checkpoint(config, case, args.target_m, True)
            validate(report, min(120, args.ready_frames - 60), config)
            results[case] = comparison(report)
            comparison_path.write_text(json.dumps({**metadata, "results": results}, indent=2) + "\n")


if __name__ == "__main__":
    main()

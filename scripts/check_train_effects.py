#!/usr/bin/env python3
"""Exercise native smoke/steam on Hanabi, CPU, hybrid and a software renderer.

Captures a moving train after floating-origin shifts, then pauses for a stable
picture. This checks execution and memory bounds, not an unpaced GPU speedup.
Original route/train assets remain external. All runs are sequential.
"""
import argparse
import json
from pathlib import Path

from check_viewer_streaming import run_checkpoint


def validate(report, mode, paused=True):
    hardware = mode != "software"
    if (report.get("renderer") or {}).get("hardware") != hardware:
        raise ValueError("Wrong renderer for this execution test")
    effects = report.get("train_effects") or {}
    expected = "cpu" if mode == "software" else mode
    if effects.get("execution") != expected:
        raise ValueError(f"Expected {expected} plumes, received {effects}")
    gpu, cpu = effects.get("gpu_capacity", 0), effects.get("cpu_capacity", 0)
    if not 0 < gpu + cpu <= 512:
        raise ValueError("Plume capacity is empty or exceeds the shared budget")
    if effects.get("cpu_live_particles", 0) > cpu:
        raise ValueError("CPU plumes exceed their partition")
    if expected in ("gpu", "hybrid"):
        if gpu <= 0 or effects.get("gpu_spawn_requests", 0) <= 0:
            raise ValueError("Hanabi did not receive native emission")
    if expected == "gpu":
        if cpu or effects.get("cpu_live_particles") or effects.get("cpu_mesh_updates", 0) > 2:
            raise ValueError("GPU mode is still rebuilding CPU plume vertices")
    elif expected == "hybrid":
        if cpu <= 0 or effects.get("cpu_mesh_updates", 0) < 2:
            raise ValueError("Hybrid did not exercise both particle paths")
    else:
        if gpu or effects.get("gpu_emitters") or effects.get("cpu_mesh_updates", 0) < 2:
            raise ValueError("CPU fallback did not use its own bounded mesh")
    if paused and effects.get("gpu_simulation_delta_s") != 0.0:
        raise ValueError("Hanabi's clock is still moving while the train is paused")
    if report.get("shader_pipelines") != {"pending": 0, "failed": 0}:
        raise ValueError("Incomplete or failed GPU pipelines")


def main():
    root = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root", required=True, type=Path)
    p.add_argument("--scenario", required=True, type=Path)
    p.add_argument("--viewer", type=Path, default=root / "target/debug/openrailsrs-viewer3d")
    p.add_argument("--out-dir", type=Path, default=root / "tmp/train-effects")
    p.add_argument("--mode", action="append", choices=["gpu", "cpu", "hybrid", "software"])
    p.add_argument("--weather", choices=["clear", "rain", "snow", "storm"], default="clear")
    p.add_argument("--target-m", type=float, default=500.0)
    p.add_argument("--timeout-s", type=float, default=240.0)
    p.add_argument("--max-rss-mib", type=float, default=6144.0)
    p.add_argument("--view-radius-m", type=int, default=450)
    p.add_argument("--speed-mul", type=float, default=4.0)
    p.add_argument("--camera-distance", type=float, default=35.0)
    args = p.parse_args()
    if args.target_m <= 0 or args.timeout_s < 30 or args.max_rss_mib <= 0:
        p.error("requires a positive travel distance, RSS bound and at least 30 seconds")
    args.repo = root
    args.route_root = args.route_root.resolve()
    args.scenario = args.scenario.resolve()
    args.viewer = args.viewer.resolve()
    args.out_dir = args.out_dir.resolve()
    if not args.viewer.is_file() or not args.scenario.is_file() or not args.route_root.is_dir():
        p.error("requires a built viewer, scenario and original route directory")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.autodrive, args.follow = 1.0, "orbit"
    args.camera_yaw, args.camera_pitch = -1.5, 0.25
    args.ready_frames = 30
    reports = {}
    for mode in dict.fromkeys(args.mode or ["gpu", "cpu", "hybrid"]):
        args.software = mode == "software"
        args.headless_wayland = not args.software
        args.require_hardware = not args.software
        args.renderer = "cpu" if args.software else "gpu"
        args.train_effect_execution = "gpu" if args.software else mode
        report = run_checkpoint(args, mode, args.target_m, True)
        validate(report, mode)
        reports[mode] = report
        (args.out_dir / "report.json").write_text(json.dumps({
            "resolution": [1280, 720], "maximum_plume_capacity": 512,
            "modes": reports,
            "scope": "moving checkpoint; capacities and requests, no GPU alive-count readback or throughput claim",
        }, indent=2) + "\n")


if __name__ == "__main__":
    main()

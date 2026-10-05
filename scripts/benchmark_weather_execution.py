#!/usr/bin/env python3
"""Same frozen scene, clock, particle seeds and budget on GPU, CPU and hybrid.

CPU particles still use the hardware renderer. The separate software run uses
lavapipe for the entire drawing pipeline. No RSS/VRAM sums or claimed GPU speedup
under presentation throttling. Runs are sequential and guarded by peak RSS.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import tomllib

from capture_route_views import station_chainages, station_scenario
from check_viewer_streaming import run_checkpoint


def validate_mode(name, report, budget):
    device = report.get("renderer") or {}
    particles = report.get("weather_particles") or {}
    hardware = name != "software"
    if device.get("hardware") != hardware:
        raise ValueError(f"{name}: wrong graphics adapter: {device}")
    expected = {"gpu": (budget, 0), "cpu": (0, budget),
                "hybrid": (budget * 3 // 4, budget - budget * 3 // 4),
                "software": (0, budget)}[name]
    if (particles.get("gpu_particles"), particles.get("cpu_particles")) != expected:
        raise ValueError(f"{name}: different particle budget: {particles}")
    if name == "gpu" and particles.get("gpu_mesh_updates") != 1:
        raise ValueError("GPU seeds were uploaded repeatedly; rebuild the viewer")
    if name in ("cpu", "hybrid", "software") and particles.get("cpu_mesh_updates", 0) < 2:
        raise ValueError("CPU vertex path did not run")
    if (report.get("performance") or {}).get("gameplay_frames", 0) < 30:
        raise ValueError("Too few gameplay samples, or viewer needs rebuilding")


def comparable(a, b):
    # Sheltering and native texture variants must be identical as well as the
    # obvious camera/clock controls; renderer metadata intentionally differs.
    for key in ("clock_time_s", "camera", "snow_cover", "solar_direction",
                "odometer_m", "gpu_tiles", "gpu_entities", "headlights", "wiper"):
        def equal(x, y):
            if isinstance(x, dict) and isinstance(y, dict):
                return x.keys() == y.keys() and all(equal(x[k], y[k]) for k in x)
            if isinstance(x, list) and isinstance(y, list):
                return len(x) == len(y) and all(equal(v, w) for v, w in zip(x, y))
            if isinstance(x, (int, float)) and isinstance(y, (int, float)):
                # Bevy's normalized camera quaternion can differ by a float ULP.
                # Absolute tolerance only: large world coordinates get no slack.
                return math.isclose(x, y, rel_tol=0, abs_tol=1e-5)
            return x == y
        if not equal(a.get(key), b.get(key)):
            raise ValueError(f"Cannot compare runs with different {key}")


def main():
    repo = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--scenario", type=Path, default=repo / "examples/chiltern_extended/scenario.toml")
    p.add_argument("--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d")
    p.add_argument("--out-dir", type=Path, default=repo / "tmp/weather-execution")
    p.add_argument("--mode", action="append", choices=["gpu", "cpu", "hybrid", "software"])
    p.add_argument("--particle-budget", type=int, default=2048)
    p.add_argument("--view-radius-m", type=int, default=450)
    p.add_argument("--clock-time-s", type=float, default=35700)
    p.add_argument("--ready-frames", type=int, default=120)
    p.add_argument("--max-rss-mib", type=int, default=6144)
    p.add_argument("--timeout-s", type=int, default=240)
    args = p.parse_args()
    if not 128 <= args.particle_budget <= 2048:
        p.error("same-budget CPU comparison requires 128–2048 particles")
    if args.ready_frames < 30 or args.timeout_s <= 10 or args.max_rss_mib <= 0:
        p.error("at least 30 ready frames and positive RSS/time bounds required")
    args.repo = repo
    args.route_root = args.route_root.resolve()
    args.viewer = args.viewer.resolve()
    args.out_dir = args.out_dir.resolve()
    source = args.scenario.resolve()
    if not args.viewer.is_file() or not args.route_root.is_dir():
        p.error("requires original route assets and an existing viewer")
    scenario = tomllib.loads(source.read_text())
    graph_path = source.parent / scenario["route"]["path"] / "track.toml"
    graph = tomllib.loads(graph_path.read_text())
    chainages = station_chainages(scenario, graph)
    stop = scenario["route"]["stops"][0]
    args.scenario = station_scenario(source, scenario,
        chainages[stop["node"]] + stop.get("offset_m", 0), 0,
        args.out_dir / "scenario")
    # Set the native start clock rather than advancing a paused session.
    import re
    args.scenario.write_text(re.sub(r"(?m)^start_time_s\s*=.*$",
        f"start_time_s = {args.clock_time_s}", args.scenario.read_text(), count=1))
    args.weather = "snow"
    args.follow, args.autodrive, args.speed_mul = "orbit", 0, 1
    args.camera_yaw, args.camera_pitch, args.camera_distance = 1.6, 0.25, 28
    args.cab_fov_deg = 45
    reports = {}
    for mode in dict.fromkeys(args.mode or ["gpu", "cpu", "hybrid", "software"]):
        args.software = mode == "software"
        args.headless_wayland = not args.software
        args.require_hardware = not args.software
        args.renderer = "cpu" if args.software else "gpu"
        args.weather_execution = "cpu" if args.software else mode
        report = run_checkpoint(args, mode, 0, True)
        validate_mode(mode, report, args.particle_budget)
        if reports:
            comparable(next(iter(reports.values())), report)
        reports[mode] = report
        (args.out_dir / "report.json").write_text(json.dumps({
            "scenario_sha256": hashlib.sha256(args.scenario.read_bytes()).hexdigest(),
            "track_sha256": hashlib.sha256(graph_path.read_bytes()).hexdigest(),
            "particle_budget": args.particle_budget, "resolution": [1280, 720],
            "presentation": "private compositor, paced; no throughput speedup claim",
            "modes": reports,
        }, indent=2) + "\n")


if __name__ == "__main__":
    main()

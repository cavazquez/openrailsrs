#!/usr/bin/env python3
"""Run an identical railway/render fixture on two viewer binaries on this machine.

The output is a replayable performance contract, raw captures and the comparison.
Only a hardware GPU is accepted. Startup is separate, missing VRAM stays unknown.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path
from check_player_expansion import options, settings
from check_viewer_streaming import run_checkpoint
from check_perf_contract import compare


def sha(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def make_contract(args, capture, scenario):
    import tomllib
    s = tomllib.loads(scenario.read_text())
    route = (scenario.parent / s["route"]["path"]).resolve()
    track = route if route.is_file() else route / "track.toml"
    stock = (scenario.parent / s["train"]["consist"]).resolve()
    cpu = "unknown"
    try:
        cpu = next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.startswith("model name"))
    except (OSError, StopIteration):
        pass
    return {"machine": {"cpu": cpu, "kernel": platform.release(), "architecture": platform.machine(), "renderer": capture.get("renderer")},
            "resolution": [1280, 720], "route_sha256": sha(track), "consist_sha256": sha(stock),
            "camera": {"follow": "orbit", "yaw": 1.6, "pitch": .28, "distance_m": 42, "fov_y_rad": capture["camera"]["fov_y_rad"]},
            "seed": 42, "quality": {"weather": args.weather, "particles": 2048, "view_distance_m": 450, "shadows": True, "contact_shadows": 1, "target_m": args.target_m}}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--baseline", type=Path, required=True)
    p.add_argument("--current", type=Path, required=True)
    p.add_argument("--baseline-resources", type=Path)
    p.add_argument("--baseline-version", default="baseline")
    p.add_argument("--current-version", default="working-tree")
    p.add_argument("--scenario", type=Path, required=True)
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--weather", choices=("clear", "rain", "snow", "fog"), default="rain")
    p.add_argument("--target-m", type=float, default=100)
    p.add_argument("--max-percent", type=float, default=15)
    a = p.parse_args()
    a.repo = Path(__file__).resolve().parents[1]
    for k in ("baseline", "current", "scenario", "route_root", "out_dir"):
        setattr(a, k, getattr(a, k).resolve())
    if a.baseline_resources:
        a.baseline_resources = a.baseline_resources.resolve()
        if not (a.baseline_resources / "assets/shaders").is_dir():
            p.error("--baseline-resources requires a resource root containing assets/shaders")
    a.out_dir.mkdir(parents=True, exist_ok=True)
    a.viewer = a.current
    settings(a, quality_limits={"enabled": False, "target_fps": 60, "min_level": 0, "max_level": 3})
    results = []
    for name, viewer in (("baseline", a.baseline), ("current", a.current)):
        opts = options(a, a.scenario, a.route_root)
        opts.viewer = viewer
        opts.weather = a.weather
        opts.ready_frames = 360
        opts.autodrive = .65
        opts.speed_mul = 2
        opts.timeout_s = 180
        if name == "baseline" and a.baseline_resources:
            opts.resources_dir = a.baseline_resources
        capture = run_checkpoint(opts, name, a.target_m, True)
        contract = make_contract(a, capture, a.scenario)
        report = {"contract": contract, "version": a.baseline_version if name == "baseline" else a.current_version, "binary_sha256": sha(viewer),
                  "vram_mib": (capture.get("graphics_memory") or {}).get("peak_process_vram_mib"), "capture": capture}
        (a.out_dir / (name + "-contract.json")).write_text(json.dumps(report, indent=2) + "\n")
        results.append(report)
    result = compare(*results, percent=a.max_percent)
    (a.out_dir / "comparison.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"passed": result["passed"], "regressions": result["regressions"]}), flush=True)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Comparable native-route measurements; loading and driving stay separate.

The two executables run sequentially on the same isolated hardware renderer.
Reports are observations, never a blanket frame-rate or visual-parity claim.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import subprocess
import shutil

import check_player_expansion as expansion
import check_viewer_streaming as streaming


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline-resources", type=Path, help="Matching shader/resources tree from the reference revision")
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=75, help="Compatibility option; capture is gated by the fixed distance checkpoint")
    parser.add_argument("--checkpoint-m", type=float, default=2500)
    parser.add_argument("--timeout-s", type=int, default=300)
    parser.add_argument("--only", choices=("baseline", "candidate"), help="Continue one side while keeping already completed reference receipts")
    parser.add_argument("--concurrency-note", default="", help="Record other known host workloads; they are not removed or paused")
    parser.add_argument("--release-case-cache", action="store_true", help="Remove only this run's disposable cache after its cold/warm receipts are saved")
    parser.add_argument("--weather", choices=("both", "clear", "downpour"), default="both")
    parser.add_argument("--profile-systems", action="store_true", help="Record bounded CPU-system histograms in both executables")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    out = args.out_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    scenario = repo / "examples/chiltern_extended/scenario.toml"
    existing = out / "comparison.json"
    versions = json.loads(existing.read_text())["versions"] if args.only and existing.exists() else {}
    for version, executable in (("baseline", args.baseline), ("candidate", args.candidate)):
        if args.only and version != args.only:continue
        args.repo = repo
        args.viewer = executable.resolve(strict=True)
        version_out = out / version
        versions[version] = {"binary_sha256": sha256(args.viewer), "cases": {}}
        weather_cases = ("clear", "downpour") if args.weather == "both" else (args.weather,)
        for weather in weather_cases:
            args.out_dir = version_out / weather
            args.out_dir.mkdir(parents=True, exist_ok=False)
            for cache in ("cold", "warm"):
                # One private cache per executable: its first launch is cold;
                # later launches reuse only that executable's derivatives.
                expansion.settings(args, weather_profile=weather, shadows=True, weather_particle_budget=2048,
                                   quality_limits={"enabled": False, "target_fps": 30,
                                                   "min_level": 0, "max_level": 3})
                opt = expansion.options(args, scenario, args.route_root.resolve(strict=True))
                opt.follow = "driver"
                opt.audio_null = True
                opt.dev_inspector = 0
                opt.dev_tools = 0
                opt.framepace = "off"
                opt.profile_systems = args.profile_systems
                if version == "baseline" and args.baseline_resources:
                    opt.resources_dir = args.baseline_resources.resolve(strict=True)
                opt.autodrive = .5
                opt.speed_mul = 4
                opt.weather_profile = weather
                opt.weather_seed = 42
                opt.timeout_s = args.timeout_s
                opt.after_ready = True
                opt.ready_frames = 30
                # Freeze at the same physical checkpoint for both executables,
                # then drain streaming/uploads. This retains streamed hitches
                # but cannot capture an incomplete neighbourhood in heavy rain.
                name = f"{weather}-{cache}"
                result = streaming.run_checkpoint(opt, name, args.checkpoint_m, pause=True)
                versions[version]["cases"][name] = result
                (out / "comparison.json").write_text(json.dumps({
                    "schema": 1, "hardware": platform.uname()._asdict(),
                    "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
                    "scenario_sha256": sha256(scenario), "scenario": str(scenario),
                    "route_root": str(args.route_root.resolve()),
                    "seed": 42, "camera": "driver", "window": [1280,720],
                    "view_distance_m": 500, "speed_multiplier": 4, "weather_particle_budget": 2048,
                    "audio": "private ALSA null", "checkpoint_m": args.checkpoint_m,
                    "cache_scope": "cold/warm refers to this executable's private application-data cache; OS file cache and Mesa shader cache are not cleared",
                    "framepace": "off", "concurrent_workloads": args.concurrency_note,
                    "profile_systems": args.profile_systems, "weathers": list(weather_cases),
                    "scope": "sequential launches; initial-load and gameplay histograms are separate; host-wide GPU busy may include other processes",
                    "versions": versions,
                }, indent=2) + "\n")
            if args.release_case_cache:
                cache_dir = args.out_dir / ".player-data/cache"
                if cache_dir.is_dir():
                    if not cache_dir.resolve().is_relative_to(args.out_dir.resolve()):
                        raise RuntimeError("Disposable cache escapes the freshly created case directory")
                    files = [f for f in cache_dir.rglob("*") if f.is_file()]
                    receipt = {"path": str(cache_dir), "files": len(files),
                               "bytes": sum(f.stat().st_size for f in files),
                               "reason": "cold and warm comparison receipts saved; regenerable cache only"}
                    (args.out_dir / "released-cache.json").write_text(json.dumps(receipt, indent=2) + "\n")
                    shutil.rmtree(cache_dir)
    print(out / "comparison.json")


if __name__ == "__main__":
    main()

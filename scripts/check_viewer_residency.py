#!/usr/bin/env python3
"""Exercise real DRM residency against an explicitly reduced process budget.

The physical GPU is not filled to exhaustion. A configured process budget is
compared with measured residency; original content and near-field readiness
remain mandatory. This is a policy test, not a device-out-of-memory test.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

import check_player_expansion as expansion
import check_viewer_streaming as streaming


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", type=Path, required=True)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--budget-mib", type=int, default=1024)
    parser.add_argument("--checkpoint-m", type=float, default=8000)
    parser.add_argument("--profile-systems", action="store_true", help="Record bounded CPU-system histograms in both policy runs")
    args = parser.parse_args()
    if not 256 <= args.budget_mib <= 65536:
        parser.error("Process budget must be 256–65536 MiB")
    args.repo = Path(__file__).resolve().parents[1]
    args.viewer = args.viewer.resolve(strict=True)
    args.out_dir = args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    source = args.repo / "examples/chiltern_extended/scenario.toml"
    results = {}
    for case, budget in (("physical", None), ("reduced-process", args.budget_mib)):
        options = expansion.options(args, source, args.route_root.resolve(strict=True))
        options.audio_null = True
        options.follow = "driver"
        options.autodrive = .5
        options.speed_mul = 4
        options.weather_profile = "clear"
        options.vram_budget_mib = budget
        options.timeout_s = 420
        options.after_ready = True
        options.ready_frames = 20
        options.dev_inspector = 0
        options.dev_tools = 0
        options.profile_systems = args.profile_systems
        expansion.settings(options, weather_profile="clear", weather_particle_budget=2048,
                           quality_limits={"enabled": False, "target_fps": 30,
                                           "min_level": 0, "max_level": 3})
        result = streaming.run_checkpoint(options, case, args.checkpoint_m, pause=True)
        policy = result.get("residency") or {}
        measured = result.get("graphics_memory") or {}
        if measured.get("process_vram_mib") is None:
            raise RuntimeError("DRM residency is unavailable; cannot prove pressure")
        if result.get("unactivated_near_shapes") != 0 or result.get("pending_gpu_uploads") != 0:
            raise RuntimeError("Visible neighbourhood did not finish under pressure")
        log = (args.out_dir / f"{case}.log").read_text(errors="replace")
        releases = [tuple(map(int, match)) for match in re.findall(
            r"freed (\d+) mesh\(es\)/(\d+) material\(s\)/(\d+) texture\(s\)", log)]
        released = {"terrain_meshes": sum(r[0] for r in releases),
                    "terrain_materials": sum(r[1] for r in releases),
                    "terrain_images": sum(r[2] for r in releases),
                    "shape_release_calls": (result.get("shape_stages") or {}).get(
                        "release_gpu_assets", {}).get("calls", 0)}
        if budget is not None:
            if policy.get("budget_mib") != budget or policy.get("transitions", 0) < 1:
                raise RuntimeError("Reduced budget never activated real residency pressure")
            if not released["terrain_meshes"] and not released["shape_release_calls"]:
                raise RuntimeError("No real resources were freed during reduced-budget travel")
        results[case] = {"capture": result, "released": released}
        report = {"schema": 1, "viewer_sha256": hashlib.sha256(args.viewer.read_bytes()).hexdigest(),
                  "scenario_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                  "checkpoint_m": args.checkpoint_m, "process_budget_mib": args.budget_mib,
                  "scope": "measured process residency against a reduced budget; physical device capacity is unchanged; no forced device OOM",
                  "cache_scope": "one fresh private application cache; the physical-policy run seeds it and the reduced-budget run reuses it; this is not a cache-speed comparison",
                  "configuration": {"camera": "driver", "seed": 42, "window": [1280, 720],
                                    "view_distance_m": 500, "speed_multiplier": 4,
                                    "autodrive": .5, "audio": "private ALSA null", "profile_systems": args.profile_systems},
                  "cases": results}
        (args.out_dir / "residency-report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(args.out_dir / "residency-report.json")


if __name__ == "__main__":
    main()

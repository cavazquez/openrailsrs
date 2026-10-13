#!/usr/bin/env python3
"""Graphical acceptance for independent equipment and environmental effects.

Runs the built viewer on a private hardware Weston display. Reports and disposable
scenarios remain in --out-dir; original route resources are read only.
"""
import argparse
import hashlib
import json
import math
import re
import tomllib
from pathlib import Path

from capture_route_views import station_chainages, station_scenario
from check_player_expansion import options, settings
from check_player_third_batch import scenario as lab_scenario
from check_viewer_streaming import run_checkpoint


def actions(*operations):
    return json.dumps([
        {"at_s": i + 1.0, "operation": operation}
        for i, operation in enumerate(operations)
    ])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--route-root", type=Path)
    parser.add_argument("--cases", nargs="+")
    args = parser.parse_args()
    args.repo = Path(__file__).resolve().parents[1]
    args.viewer = args.viewer.resolve()
    args.out_dir = args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    report_file = args.out_dir / "report.json"
    reports = json.loads(report_file.read_text()) if report_file.exists() else {}

    def run(name, opts):
        report = run_checkpoint(opts, name, 0, False)
        report["binary_sha256"] = hashlib.sha256(args.viewer.read_bytes()).hexdigest()
        report["accepted"] = False
        reports[name] = report
        report_file.write_text(json.dumps(reports, indent=2) + "\n")
        assert report["shader_pipelines"]["failed"] == 0
        assert report["shader_pipelines"]["pending"] == 0
        commands = report.get("equipment_actions", [])
        expected = json.loads(getattr(opts, "capture_actions", "[]"))
        assert len(commands) == len(expected), (name, commands, expected)
        assert all(command["result"] == {"Ok": None} for command in commands), commands
        return report

    def accept(name, report):
        report["accepted"] = True
        report_file.write_text(json.dumps(reports, indent=2) + "\n")
        print("PASS", name, flush=True)

    owned = [
        "doors-left", "doors-right", "passenger-walk", "station-night",
        "collectors", "cab-switch", "fog-banks", "cloud-shadows",
        "fog-origin", "cloud-origin", "station-origin",
    ]
    for name in owned:
        if args.cases and name not in args.cases:
            continue
        settings(args, quality_limits={"enabled": False, "target_fps": 60,
                                       "min_level": 0, "max_level": 3},
                 fog_quality="volumetric64")
        night = name in ("station-night", "station-origin")
        path = lab_scenario(args, name, hour=21 if night else 12)
        root = args.repo / "examples/player_lab/route"
        if name in ("collectors", "cab-switch"):
            example = ("examples/electric_supply/scenario_two_pantographs.toml"
                       if name == "collectors" else
                       "examples/traction_operation/scenario_multiple_unit.toml")
            path = args.repo / example
            root = path.parent / "route"
        opts = options(args, path, root)
        opts.after_ready = False
        opts.timeout_s = 28
        opts.capture_headlights = 2
        opts.camera_yaw, opts.camera_distance, opts.camera_pitch = math.pi, 32, 0.2
        if name.startswith("doors-"):
            side = name.split("-")[1]
            opts.capture_actions = json.dumps([
                {"at_s": 1, "operation": {"action": "doors", "vehicle": 1,
                                          "side": side, "open": True}},
                {"at_s": 5, "operation": {"action": "pause", "value": True}},
            ])
        elif name == "passenger-walk":
            opts.capture_actions = actions(
                {"action": "view", "mode": "passenger"},
                {"action": "walk", "delta": [100, 0, 0]},
                {"action": "walk", "delta": [0, 0, 100]},
                {"action": "pause", "value": True})
        elif name == "collectors":
            opts.capture_actions = actions(
                {"action": "collector", "vehicle": 0, "index": 1},
                {"action": "collector", "vehicle": 0, "index": 0})
        elif name == "cab-switch":
            opts.follow = "driver"
            opts.capture_actions = actions(
                {"action": "cab", "vehicle": 1}, {"action": "cab", "vehicle": 0},
                {"action": "cab", "vehicle": 1}, {"action": "pause", "value": True})
        elif name in ("fog-banks", "cloud-shadows", "fog-origin", "cloud-origin"):
            kind = "fog" if name.startswith("fog-") else "clear"
            with path.open("a") as stream:
                stream.write(
                    '\n[simulation.weather_track]\nschema=1\nseed=42\n'
                    '[[simulation.weather_track.phases]]\nstart_s=0.0\n'
                    f'transition_s=0.0\nkind="{kind}"\nintensity=0.8\n'
                    'wind_mps=[12.0,3.0]\n')
            opts.weather_profile = "edited"
        if name.endswith("-origin"):
            opts.camera_journey = "aba"
            opts.timeout_s = 120
        report = run(name, opts)
        if name.startswith("doors-"):
            car = next(c for c in report["vehicle_equipment"]["cars"]
                       if c["definition"]["vehicle"] == 1)
            expected = [1.0, 0.0] if name == "doors-left" else [0.0, 1.0]
            assert car["door_fraction"] == expected, car
        elif name == "passenger-walk":
            eye = report["passenger_eye"]
            assert eye["vehicle"] == 1
            assert abs(eye["head_msts"][0]) <= 0.401, eye
            assert abs(eye["head_msts"][2]) <= 8.001, eye
        elif name in ("station-night", "station-origin"):
            assert 0 < report["station_lighting"]["active"] <= 24
        elif name == "collectors":
            assert report["electric_contacts"]["animated_collectors"] == 2
        elif name == "cab-switch":
            assert report["vehicle_equipment"]["active_cab"] == 1
        elif name == "fog-banks":
            assert report["fog"]["volumetric"]
            assert report["weather_state"]["advection"]["offset_m"][0] > 100
        if name.endswith("-origin"):
            journey = report["camera_journey"]
            assert journey["complete"] and journey["distance_m"] > 2000, journey
        accept(name, report)

    if args.route_root:
        source = args.repo / "examples/chiltern_extended/scenario.toml"
        data = tomllib.loads(source.read_text())
        graph = tomllib.loads((source.parent / data["route"]["path"] / "track.toml").read_text())
        chain = station_chainages(data, graph)
        for index, label in [(0, "northolt"), (1, "south-ruislip"), (5, "gerrards-cross")]:
            for follow in ("orbit", "driver"):
                name = label + "-night-" + follow
                if args.cases and name not in args.cases:
                    continue
                stop = data["route"]["stops"][index]
                distance = chain[stop["node"]] + stop.get("offset_m", 0.0)
                path = station_scenario(source, data, distance, index,
                                        args.out_dir / "scenarios" / name)
                path.write_text(re.sub(r"(?m)^start_time_s\s*=.*$",
                                       "start_time_s = 82800.0", path.read_text(), count=1))
                settings(args, quality_limits={"enabled": False, "target_fps": 60,
                                               "min_level": 0, "max_level": 3})
                opts = options(args, path, args.route_root.resolve())
                opts.follow, opts.timeout_s, opts.capture_headlights = follow, 100, 1
                report = run(name, opts)
                assert 0 < report["station_lighting"]["active"] <= 24, report["station_lighting"]
                accept(name, report)
        for wipe in (0, 1):
            name = "native-rain-glass-" + str(wipe)
            if args.cases and name not in args.cases:
                continue
            stop = data["route"]["stops"][1]
            distance = chain[stop["node"]] + stop.get("offset_m", 0.0)
            path = station_scenario(source, data, distance, 1,
                                    args.out_dir / "scenarios" / name)
            settings(args, quality_limits={"enabled": False, "target_fps": 60,
                                           "min_level": 0, "max_level": 3})
            opts = options(args, path, args.route_root.resolve())
            opts.follow, opts.timeout_s = "driver", 100
            opts.weather, opts.capture_wiper = "rain", wipe
            report = run(name, opts)
            assert report["cab_view"] is not None
            assert report["weather_state"]["atmosphere"]["rain"] > 0.1
            accept(name, report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

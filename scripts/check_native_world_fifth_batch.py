#!/usr/bin/env python3
"""Hardware captures of original Chiltern snow and a road/rail crossing.

Author assets stay in --route-root. The crossing's graph is derived from its TDB.
Park Royal road items 813/814 lie inside the original spawner ranges 811/812 and
107/108 on their respective RDB vectors; nearby but unrelated roads are excluded.
Use a private compositor and a single hardware adapter per invocation.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import subprocess
import tomllib

from capture_route_views import station_chainages, station_scenario
from check_player_expansion import options, settings
from check_viewer_streaming import run_checkpoint


def scalar(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False)


def crossing_scenario(args, start):
    directory = args.out_dir / "crossing-graph"
    directory.mkdir(parents=True, exist_ok=True)
    imported = directory / "original-track.toml"
    if not imported.exists():
        subprocess.run([str(args.cli), "import-msts", str(args.route_root),
                        "--out-dir", str(directory)], check=True)
        (directory / "track.toml").rename(imported)
    original = tomllib.loads(imported.read_text())
    if not any(edge["id"] == "e1994" for edge in original["edges"]):
        raise ValueError("This acceptance route needs the original Chiltern v4 TDB vectors")
    # Preserve all neighbouring track metadata. Otherwise a genuine crossing
    # on an unrepresented parallel track would correctly remain unresolved.
    (directory / "track.toml").write_bytes(imported.read_bytes())
    path = directory / ("scenario-" + str(start) + ".toml")
    path.write_text(
        '[scenario]\nname="Cruce original de Chiltern: Park Royal"\nstart_time_s=43200\n'
        '[route]\npath="."\nstart="n1993"\ndestination="n2017"\n'
        'waypoints=["n1993","n1995","n2021","n2019","n2015","n2017"]\n'
        'assume_signals_clear=true\n'
        f'start_offset_m={start}\n[train]\nconsist='
        + scalar(str(args.repo / "examples/chiltern_extended/consists/birmingham_pullman.con"))
        + '\n[simulation]\nduration=600\ntime_step=0.05\nseed=42\nmulti_body=true\n'
        '[gameplay]\nobjective="arrive"\ndifficulty="normal"\n'
    )
    return path


def native_snow_scenario(args):
    source = args.repo / "examples/chiltern_extended/scenario.toml"
    scenario = tomllib.loads(source.read_text())
    graph = tomllib.loads((source.parent / "track.toml").read_text())
    stop = scenario["route"]["stops"][1]
    chainage = station_chainages(scenario, graph)[stop["node"]] + stop["offset_m"]
    return station_scenario(source, scenario, chainage, 1, args.out_dir / "snow-scenario")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", type=Path, required=True)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--cli", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--cases", nargs="+", choices=["roofs-snow", "crossing-closed", "crossing-released"])
    args = parser.parse_args()
    args.repo = Path(__file__).resolve().parents[1]
    args.cli = (args.cli or args.repo / "target/debug/openrailsrs").resolve()
    args.viewer = args.viewer.resolve()
    args.route_root = args.route_root.resolve()
    args.out_dir = args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    reports = {}
    report_path = args.out_dir / "report.json"
    if report_path.exists():
        reports = json.loads(report_path.read_text())
    for name in (args.cases or ["roofs-snow", "crossing-closed", "crossing-released"]):
        settings(args, improved_transparency=True,
                 quality_limits={"enabled": False, "target_fps": 60, "min_level": 0, "max_level": 3})
        path = native_snow_scenario(args) if name == "roofs-snow" else crossing_scenario(
            args, 239.8314 if name == "crossing-closed" else 190.0)
        opt = options(args, path, args.route_root)
        opt.after_ready = False
        opt.timeout_s = 120
        opt.capture_or_focus = True
        opt.camera_yaw = 1.6
        opt.camera_distance = 60
        opt.particle_budget = 512
        released = name == "crossing-released"
        if released:
            opt.autodrive = .75
            opt.speed_mul = 4
            # Hold the camera over the original crossing as the train clears
            # it. Following the head would end up over an industrial roof and
            # hide precisely the barrier/road interaction being inspected.
            opt.capture_actions = json.dumps([
                {"at_s": 1, "operation": {"action": "fly_camera",
                                          "position_m": [88., 17., -282.],
                                          "look_at_m": [31., 1., -272.]}}
            ])
        if name == "roofs-snow":
            opt.weather = "snow"
        if name == "crossing-closed":
            # Let the native gate finish closing, then hold/restore it. Pausing
            # at odometer zero would correctly preserve its initial open pose.
            opt.capture_actions = json.dumps([
                {"at_s": 12, "operation": {"action": "pause", "value": True}},
                {"at_s": 13, "operation": {"action": "restore"}},
                {"at_s": 17, "operation": {"action": "pause", "value": True}},
            ])
        result = run_checkpoint(opt, name, 380 if released else 0,
                                pause=name != "crossing-closed")
        if name == "roofs-snow":
            assert result["snow_cover"] > .05
        else:
            for uid in (12473, 12474):
                crossing = result["crossings"][f"-6083:14926:{uid}"]
                assert crossing["road_closed"] == (not released), crossing
                assert abs(crossing["fraction"] - (0 if released else 1)) < .001, crossing
                assert crossing["definition"]["lanes"] == [{
                    "edge": "e1994", "position_m": 239.8314 if uid == 12473 else 242.8448}], crossing
            cars = result["road_cars"]
            assert cars and all(car["follows_rdb"] for car in cars), cars
            assert len(cars) <= 128
            assert all(math.isfinite(value) for car in cars
                       for value in car["position_render_m"])
            if released:
                assert result["odometer_m"] >= 380
                assert len(result["equipment_actions"]) == 1
                assert result["equipment_actions"][0]["result"] == {"Ok": None}
            else:
                assert len(result["equipment_actions"]) == 3
                assert all(action["result"] == {"Ok": None}
                           for action in result["equipment_actions"])
                before, after = result["equipment_actions"][:2]
                assert abs(before["time_s"] - after["time_s"]) < 1e-9
                # ECS poses have the floating-origin shift removed, whereas
                # road geometry/stop points retain focus-relative coordinates.
                # Check an actual queue head at its length-dependent stop gap.
                origin = result["floating_origin_shift"]
                closed_stops = [stop["position_render_m"]
                                for stop in result["road_crossings"]["stops"]
                                if stop["closed"]]
                distances = [
                    (min(math.dist([value + shift for value, shift in
                                    zip(car["position_render_m"], origin)], stop)
                         for stop in closed_stops), 10 + car["length_m"] / 2)
                    for car in cars
                ]
                assert any(abs(distance - gap) < 1 for distance, gap in distances), distances
                result["queued_head_stop_errors_m"] = [abs(distance - gap)
                                                       for distance, gap in distances
                                                       if abs(distance - gap) < 1]
            result["scope"] = "Original Park Royal WORLD gates and RDB, original TDB graph, chosen author-track itinerary"
        result["binary_sha256"] = hashlib.sha256(args.viewer.read_bytes()).hexdigest()
        result["runtime_asset_sha256"] = {
            str(path.relative_to(args.repo)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in (args.repo / "crates/openrailsrs-bevy-scenery/assets/shaders").glob("*.wgsl")}
        reports[name] = result
        report_path.write_text(json.dumps(reports, indent=2) + "\n")
        print("PASS " + name, flush=True)


if __name__ == "__main__":
    main()

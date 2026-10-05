#!/usr/bin/env python3
"""Capture scenery/HUD at each authored station, sequentially with an RSS guard.

These frozen station poses complement check_viewer_streaming.py's actual full
journey. They are visual inspections, not a physics or pixel-parity acceptance.
Requires the existing viewer build and original route assets; Linux/Xvfb.
"""
import argparse
import json
from pathlib import Path
import re
import tomllib

from check_viewer_streaming import run_checkpoint


def station_chainages(scenario, graph):
    waypoints = scenario["route"]["waypoints"]
    result = {waypoints[0]: 0.0}
    distance = 0.0
    for start, end in zip(waypoints, waypoints[1:]):
        edges = [edge for edge in graph["edges"]
                 if edge["from"] == start and edge["to"] == end]
        if len(edges) != 1:
            raise ValueError(f"Ambiguous authored path {start} → {end}")
        distance += edges[0]["length_m"]
        result[end] = distance
    return result


def station_scenario(source, scenario, chainage, stop_index, directory):
    text = source.read_text()
    base = source.parent
    # Preserve the full authored physics/configuration and resolve paths before
    # writing a disposable scenario outside the original scenario directory.
    section = ""
    lines = []
    for line in text.splitlines():
        if line.strip().startswith("["):
            section = line.strip()
        if section == "[route]" and re.match(r"path\s*=", line):
            line = f"path = {json.dumps(str((base / scenario['route']['path']).resolve()))}"
        elif section == "[route]" and re.match(r"start_offset_m\s*=", line):
            line = f"start_offset_m = {chainage:.12f}"
        elif section in ("[train]", "[[extra_trains]]") and re.match(r"consist\s*=", line):
            # Every service keeps its own consist and native initial position.
            value = tomllib.loads(line)["consist"]
            line = f"consist = {json.dumps(str((base / value).resolve()))}"
        lines.append(line)
    text = "\n".join(lines) + "\n"
    index = -1

    def keep_stop(match):
        nonlocal index
        index += 1
        return match.group(0) if index >= stop_index else ""

    text = re.sub(r"\[\[route\.stops\]\].*?(?=\n\[|\Z)",
                  keep_stop, text, flags=re.DOTALL)
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "scenario.toml"
    path.write_text(text)
    return path


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--scenario", type=Path, default=repo / "examples/chiltern_extended/scenario.toml")
    parser.add_argument("--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d")
    parser.add_argument("--out-dir", type=Path, default=repo / "tmp/route-views")
    parser.add_argument("--software", action="store_true")
    parser.add_argument("--headless-wayland", action="store_true")
    parser.add_argument("--require-hardware", action="store_true")
    parser.add_argument("--weather", choices=["clear","rain","fog","snow"], default="clear")
    parser.add_argument("--weather-execution", choices=["auto", "gpu", "cpu", "hybrid"], default="auto")
    parser.add_argument("--renderer", choices=["auto", "gpu", "cpu"], default="auto")
    parser.add_argument("--texture-cache", choices=["on", "off"], default="on")
    parser.add_argument("--texture-upload", choices=["auto", "rgba"], default="auto")
    parser.add_argument("--particle-budget", type=int)
    parser.add_argument("--clock-time-s", type=float, default=35700)
    parser.add_argument("--station", action="append", help="station slug; repeat to select views")
    parser.add_argument("--max-rss-mib", type=int, default=6144)
    parser.add_argument("--timeout-s", type=int, default=180)
    parser.add_argument("--camera-yaw", type=float, default=1.6)
    parser.add_argument("--camera-pitch", type=float, default=0.6)
    parser.add_argument("--camera-distance", type=float, default=160)
    parser.add_argument("--view-radius-m", type=int, default=450)
    parser.add_argument("--with-cab", action="store_true", help="inspect the 3D cab/HUD at every station")
    parser.add_argument("--cab-fov-deg", type=float, default=45, help="native OR reference uses 45 degrees")
    args = parser.parse_args()
    args.repo = repo
    args.scenario = args.scenario.resolve()
    source = args.scenario
    args.viewer = args.viewer.resolve()
    args.route_root = args.route_root.resolve()
    args.out_dir = args.out_dir.resolve()
    if not args.viewer.is_file() or not args.route_root.is_dir():
        parser.error("requires an existing viewer build and original route assets")
    if args.max_rss_mib <= 0 or args.timeout_s <= 10:
        parser.error("RSS must be positive and timeout must exceed ten seconds")
    scenario = tomllib.loads(source.read_text())
    graph = tomllib.loads((source.parent / scenario["route"]["path"] / "track.toml").read_text())
    chainages = station_chainages(scenario, graph)
    args.out_dir.mkdir(parents=True, exist_ok=True)
    args.follow = "orbit"
    args.autodrive = 0
    args.speed_mul = 1
    args.ready_frames = 45
    reports = {}
    for index, stop in enumerate(scenario["route"]["stops"]):
        name = re.sub(r"[^a-z0-9]+", "-", stop.get("name", stop["node"]).lower()).strip("-")
        if args.station and name not in args.station:
            continue
        chainage = chainages[stop["node"]] + stop.get("offset_m", 0)
        args.scenario = station_scenario(source, scenario, chainage, index,
                                         args.out_dir / "scenarios" / name)
        station_text = args.scenario.read_text()
        station_text = re.sub(r"(?m)^start_time_s\s*=.*$", f"start_time_s = {args.clock_time_s}", station_text, count=1)
        args.scenario.write_text(station_text)
        report = run_checkpoint(args, name, 0, True)
        if not report.get("camera") or not report.get("solar_direction"):
            raise RuntimeError("Viewer lacks camera / sun metadata; rebuild the workspace")
        reports[name] = report
        if args.with_cab:
            args.follow = "driver"
            reports[name + "-cab"] = run_checkpoint(args, name + "-cab", 0, True)
            args.follow = "orbit"
    (args.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")


if __name__ == "__main__":
    main()

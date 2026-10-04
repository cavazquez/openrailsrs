#!/usr/bin/env python3
"""Audit installed formations and inspect representative diesel/electric/steam stock.

Captures each original exterior and cab sequentially in private Xvfb displays.
The checks certify resource loading, not traction or C# script parity.
"""
import argparse
import array
import json
import math
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import wave

from capture_route_views import station_scenario
from check_viewer_streaming import run_checkpoint


FORMATIONS = [
    ("Bristol Pullman.con", "diesel"), ("121single.con", "diesel"),
    ("1960CentralWR8Car.con", "electric"), ("R Stock 6 Car.con", "electric"),
    ("Downton Hall LE.con", "steam"), ("KingLE.con", "steam"),
]


def formation_scenario(source, scenario, consist, directory):
    # Preserve the authored station pose. Only replace the player's stock.
    path = station_scenario(source, scenario, scenario["route"]["start_offset_m"], 0, directory)
    text = path.read_text()
    text = re.sub(r"\[\[extra_trains\]\].*?(?=\n\[|\Z)", "", text, flags=re.DOTALL)
    text = re.sub(r'(\[train\]\s*\nconsist\s*=\s*)"[^\n]*"',
                  lambda m: m[1] + json.dumps(str(consist)), text)
    path.write_text(text)
    return path


def audio_checkpoint(repo, route, consist, wav, view):
    oracle = repo / "target/debug/examples/native_oracle"
    if not oracle.is_file():
        raise RuntimeError("Build cargo build -p openrailsrs-audio --example native_oracle first")
    report = json.loads(subprocess.check_output([
        str(oracle), str(consist), str(route), str(wav), view,
    ], cwd=repo, text=True))
    with wave.open(str(wav), "rb") as source:
        if source.getsampwidth() != 2:
            raise RuntimeError(f"{wav}: expected 16-bit PCM")
        samples = array.array("h", source.readframes(source.getnframes()))
        if sys.byteorder != "little":
            samples.byteswap()
        report["duration_s"] = source.getnframes() / source.getframerate()
    report["rms"] = math.sqrt(sum((s / 32768) ** 2 for s in samples) / max(1, len(samples)))
    report["clipped_percent"] = 100 * sum(abs(s) >= 32767 for s in samples) / max(1, len(samples))
    wav.with_suffix(".audio.json").write_text(json.dumps(report, indent=2) + "\n")
    if not report["programs"] or not report["samples"] or report["warnings"]:
        raise RuntimeError(f"{consist.name}: native sound loading failed: {report['warnings']}")
    if report["rms"] <= 1e-5 or report["clipped_percent"]:
        raise RuntimeError(f"{consist.name} {view}: silent or clipped native mix: {report}")
    return report


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--scenario", type=Path, default=repo / "examples/chiltern_local/scenario.toml")
    parser.add_argument("--out-dir", type=Path, default=repo / "tmp/rolling-stock")
    parser.add_argument("--formation", action="append", help="one of the six representative .con names")
    parser.add_argument("--software", action="store_true")
    parser.add_argument("--headless-wayland", action="store_true", help="use private Weston for hardware Vulkan presentation")
    parser.add_argument("--require-hardware", action="store_true")
    parser.add_argument("--max-rss-mib", type=int, default=6144)
    parser.add_argument("--timeout-s", type=int, default=240)
    parser.add_argument("--audio", action="store_true", help="also render offline native WAV demonstrations")
    parser.add_argument("--travel-m", type=float, default=0,
                        help="drive to this distance before capture; exercise instruments and emitters")
    args = parser.parse_args()
    if not math.isfinite(args.travel_m) or args.travel_m < 0:
        parser.error("travel distance must be finite and nonnegative")
    args.repo = repo
    args.viewer = repo / "target/debug/openrailsrs-viewer3d"
    args.route_root = args.route_root.resolve()
    args.out_dir = args.out_dir.resolve()
    source = args.scenario.resolve()
    scenario = tomllib.loads(source.read_text())
    consists = args.route_root.parents[1] / "TRAINS/CONSISTS"
    if not consists.is_dir() or not args.viewer.is_file():
        parser.error("requires a viewer build and original Content/TRAINS/CONSISTS")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    audit = json.loads(subprocess.check_output([
        str(repo / "target/debug/openrailsrs"), "audit-consists", str(consists), "--json",
    ], cwd=repo, text=True))
    (args.out_dir / "installed-audit.json").write_text(json.dumps(audit, indent=2) + "\n")
    by_name = {Path(f["report"]["path"]).name: f for f in audit["formations"]}
    selected = set(args.formation or [name for name, _ in FORMATIONS])
    if selected - {name for name, _ in FORMATIONS}:
        parser.error("--formation must name a representative formation")
    args.autodrive = 0.75 if args.travel_m else 0
    args.speed_mul = 16 if args.travel_m else 1
    args.ready_frames = 20
    args.view_radius_m = 450
    args.cab_fov_deg = 45
    args.camera_yaw = 1.6
    args.camera_pitch = 0.25
    results = []
    for name, kind in FORMATIONS:
        if name not in selected:
            continue
        stock = by_name[name]
        if not stock["player_ready"]:
            raise RuntimeError(f"{name}: incomplete original content: {stock['report']['errors']}")
        slug = re.sub(r"[^a-z0-9]+", "-", Path(name).stem.lower())
        args.scenario = formation_scenario(source, scenario, consists / name, args.out_dir / "scenarios" / slug)
        args.camera_distance = max(40, stock["report"]["length_m"] * 0.8)
        views = {}
        modes = [("exterior", "orbit"), ("cab", "driver" if stock["report"]["cab_3d"] else "cab2d")]
        if stock["report"]["cab_3d"] and stock["report"]["cab_2d"]:
            modes.append(("cab2d", "cab2d"))
        for view, follow in modes:
            args.follow = follow
            label = f"{slug}-{view}"
            views[view] = run_checkpoint(args, label, args.travel_m, True)
            if args.travel_m:
                effects = views[view].get("train_effects") or {}
                if not (0 < effects.get("live_particles", 0) <= effects.get("particle_limit", 0)):
                    raise RuntimeError(f"{label}: moving emitters missing or unbounded: {effects}")
            log = (args.out_dir / f"{label}.log").read_text()
            matched = re.search(r"live drive — (\d+) vehicle\(s\) \((\d+) shape / (\d+) fallback", log)
            if not matched or int(matched[3]) or int(matched[1]) != stock["report"]["vehicles"]:
                raise RuntimeError(f"{label}: missing native vehicle geometry")
            if view != "exterior" and "cab CVF " not in log:
                raise RuntimeError(f"{label}: original CVF did not load")
            if follow == "cab2d" and "cab 2D CVF —" not in log:
                raise RuntimeError(f"{label}: original 2D instruments did not load")
        audio = {}
        if args.audio:
            for view in ("exterior", "cab"):
                wav = args.out_dir / f"{slug}-{view}.wav"
                audio[view] = audio_checkpoint(repo, args.route_root, consists / name, wav, view)
        results.append({"formation": name, "traction": kind, "audit": stock["report"], "views": views, "audio": audio})
        # Persist successful progress even when a later content file fails.
        (args.out_dir / "report.json").write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Reproduce the second player batch's graphics on a private Weston display.

The built viewer must include dev-tools. Author content is read only and optional;
all settings, reports and disposable scenarios go into --out-dir. This verifies
rendering/state, not pixel parity with Open Rails. See docs/PLAN_30_2.md.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tomllib
from types import SimpleNamespace

from capture_route_views import station_chainages, station_scenario
from check_viewer_streaming import run_checkpoint, validate_formation


def options(args, scenario, route_root):
    return SimpleNamespace(
        repo=args.repo, viewer=args.viewer, scenario=scenario, route_root=route_root,
        out_dir=args.out_dir, software=False, headless_wayland=True,
        require_hardware=True, renderer="gpu", view_radius_m=450,
        max_rss_mib=6144, timeout_s=180, follow="orbit", ready_frames=60,
        weather="clear", weather_execution="gpu", train_effect_execution="gpu",
        autodrive=0, speed_mul=1, camera_yaw=1.6, camera_pitch=0.28,
        camera_distance=42, cab_fov_deg=55, weather_seed=42,
        particle_budget=2048, train_motion="smooth", after_service=False,
    )


def settings(args, **extra):
    data = args.out_dir / ".player-data"
    data.mkdir(exist_ok=True)
    (data / "settings.json").write_text(json.dumps({
        "sky_date": "2000-01-21", "weather_seed": 42, "shadows": True,
        "contact_shadow_quality": 1, "view_distance_m": 500, **extra,
    }))


def laboratory(args, reports):
    settings(args)
    opt = options(args, args.repo / "examples/player_lab/scenario.toml",
                  args.repo / "examples/player_lab/route")
    opt.camera_distance = 26
    opt.camera_pitch = 0.1
    opt.capture_or_focus = True
    opt.capture_equipment = True
    opt.capture_bug_report = True
    opt.after_ready = False
    opt.timeout_s = 50
    result = run_checkpoint(opt, "lab-equipment-report", 0, False)
    result["viewer_sha256"] = hashlib.sha256(opt.viewer.read_bytes()).hexdigest()
    result["formation_validation"] = validate_formation(result, 2)
    assert "LABORATORIO" in "\n".join(result["destination_displays"]).upper()
    cab = result["cab_equipment"][0]
    assert cab["windows"] == [1., 1.] and cab["door"] == 1.
    assert 0 < cab["water_l"] < 2 and cab["spraying"]
    assert any(p["kind"] == "LeftWindow" and abs(p["translation"][1]) > .5
               for p in result["equipment_poses"])
    package = max((args.out_dir / ".player-data/reports").glob("report-*"),
                  key=lambda p: p.stat().st_mtime_ns)
    diagnostic = json.loads((package / "diagnostic.json").read_text())
    assert len(diagnostic["binary_sha256"]) == 64
    assert "/home/" not in (package / "report.txt").read_text()
    assert (package / "screenshot.png").is_file()
    result["report_package"] = {"files": sorted(p.name for p in package.iterdir()),
                                "diagnostic": diagnostic}
    reports["lab-equipment-report"] = result


def native(args, reports):
    source = args.repo / "examples/chiltern_extended/scenario.toml"
    scenario = tomllib.loads(source.read_text())
    graph = tomllib.loads((source.parent / scenario["route"]["path"] / "track.toml").read_text())
    chainages = station_chainages(scenario, graph)
    cases = [
        ("northolt-exterior", "orbit", 0, 42, 1),
        ("northolt-cab", "driver", 0, 42, 1),
        ("south-ruislip-exterior", "orbit", 1, 48, 1),
        ("south-ruislip-cab", "driver", 1, 42, 1),
        ("gerrards-cross-exterior", "orbit", 5, 80, 1),
        ("gerrards-cross-cab", "driver", 5, 42, 1),
        ("contact-off", "orbit", 0, 24, 0),
        ("contact-high", "orbit", 0, 24, 2),
        ("forest-far", "orbit", 0, 320, 1),
        ("bridge-low", "orbit", 1, 24, 1),
        ("current-drive", "orbit", 0, 42, 1),
        ("edited-rain-cab", "driver", 1, 42, 1),
        ("edited-snow-exterior", "orbit", 1, 48, 1),
        ("edited-fog-cab", "driver", 1, 42, 1),
    ]
    if args.baseline:
        cases += [("baseline-drive", "orbit", 0, 42, 0)]
    for name, follow, index, distance, contact in cases:
        if args.cases and name not in args.cases:
            continue
        index = min(index, len(scenario["route"]["stops"]) - 1)
        stop = scenario["route"]["stops"][index]
        chainage = (scenario["route"]["start_offset_m"] if index == 0 else
                    chainages[stop["node"]] + stop.get("offset_m", 0))
        # Weather captures depart immediately from the chosen station. Future
        # stops retain their authored schedule; no installed activity is edited.
        first_stop = index + 1 if name.startswith("edited-") else index
        disposable = station_scenario(source, scenario, chainage, first_stop,
                                      args.out_dir / "scenarios" / name)
        settings(args, contact_shadow_quality=contact)
        opt = options(args, disposable, args.route_root)
        opt.follow, opt.camera_distance = follow, distance
        opt.camera_pitch = -.1 if name == "bridge-low" else .28
        opt.capture_headlights = 2 if "fog" in name else 1
        moving = name.endswith("drive") or name.startswith("edited-")
        if moving:
            opt.autodrive = .75
        if name.startswith("edited-"):
            kind = name.split("-")[1]
            with disposable.open("a") as stream:
                stream.write(
                    '\n[simulation.weather_track]\nschema = 1\nseed = 42\n'
                    '[[simulation.weather_track.phases]]\nstart_s = 0.0\ntransition_s = 0.0\n'
                    'kind = "clear"\nintensity = 0.0\nwind_mps = [1.0, 0.3]\n'
                    '[[simulation.weather_track.phases]]\nstart_s = 8.0\ntransition_s = 4.0\n'
                    f'kind = "{kind}"\nintensity = 0.85\nwind_mps = [3.0, 1.0]\n'
                )
            opt.weather_profile = "edited"
            if kind == "snow":
                opt.camera_journey = "aba"
        if name == "baseline-drive":
            opt.viewer = args.baseline
            if args.baseline_resources:
                opt.resources_dir = args.baseline_resources
        result = run_checkpoint(opt, name, 100 if moving else 0, True)
        result["viewer_sha256"] = hashlib.sha256(opt.viewer.read_bytes()).hexdigest()
        if name.startswith("edited-"):
            atmosphere = result["weather_state"]["atmosphere"]
            if "rain" in name:
                assert abs(atmosphere["rain"] - .85) < .001
            elif "snow" in name:
                assert abs(atmosphere["snow"] - .85) < .001
                assert result["camera_journey"]["complete"]
            else:
                assert atmosphere["visibility_m"] < 1000
        reports[name] = result
        (args.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", type=Path, default=repo / "target/debug/openrailsrs-viewer3d")
    parser.add_argument("--out-dir", type=Path, default=repo / "tmp/player-expansion-qa")
    parser.add_argument("--route-root", type=Path, help="Installed original Chiltern ROUTES/Chiltern")
    parser.add_argument("--baseline", type=Path, help="Unmodified reference viewer binary")
    parser.add_argument("--baseline-resources", type=Path, help="Reference shaders/resources")
    parser.add_argument("--cases", nargs="+", help="Run only these named native cases")
    parser.add_argument("--skip-lab", action="store_true")
    args = parser.parse_args()
    args.repo = repo
    args.viewer, args.out_dir = args.viewer.resolve(), args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    reports = {}
    if not args.skip_lab:
        laboratory(args, reports)
    if args.route_root:
        args.route_root = args.route_root.resolve()
        native(args, reports)
    (args.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")
    (args.out_dir / "provenance.json").write_text(json.dumps({
        "viewer_sha256": hashlib.sha256(args.viewer.read_bytes()).hexdigest(),
        "scope": "own fixture and installed author content; internal graphics acceptance",
        "settings": {"sky_date": "2000-01-21", "seed": 42, "view_radius_m": 450},
        "cases": list(reports),
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()

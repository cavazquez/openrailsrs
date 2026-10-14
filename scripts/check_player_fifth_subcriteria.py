#!/usr/bin/env python3
"""Repeat pause, camera and floating-origin acceptance on owned fixtures.

Requires an already-built dev-tools viewer and an isolated hardware compositor.
Never edits installed author content or the player's settings. Captures are new
evidence; earlier receipts are deliberately not rewritten.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

from check_player_expansion import options, settings
from check_player_fifth_batch import scenario
from check_viewer_streaming import run_checkpoint


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def action(at, name, **values):
    return {"at_s": at, "operation": {"action": name, **values}}


def sequence(final_view):
    return [
        action(2, "pause", value=True),
        action(3, "restore"),
        action(4, "pause", value=False),
        action(5, "fly_camera", position_m=[500., 30., 0.], look_at_m=[0., 0., 0.]),
        action(9, "orbit", yaw=.35, pitch=.25, distance_m=38.),
        action(11, "view", mode="driver"),
        action(14, "view", mode="orbit"),
        action(17, "view", mode=final_view),
        action(19, "pause", value=True),
    ]


def validate_actions(report, actions):
    observed = report["equipment_actions"]
    assert len(observed) == len(actions), observed
    assert all(row.get("result") == {"Ok": None} for row in observed), observed
    paused = next(i for i, row in enumerate(actions)
                  if row["operation"] == {"action": "pause", "value": True})
    assert actions[paused + 1]["operation"]["action"] == "restore"
    assert abs(observed[paused]["time_s"] - observed[paused + 1]["time_s"]) < 1e-9, observed
    shifts = [row["origin_m"] for row in observed]
    assert all(len(x) == 3 and all(math.isfinite(v) for v in x) for x in shifts)
    assert any(math.hypot(x[0], x[2]) >= 256 for x in shifts), shifts
    assert report["system_stages"], "System cost counters unavailable"
    return {"paused_time_s": observed[paused]["time_s"],
            "restored_time_s": observed[paused + 1]["time_s"],
            "maximum_observed_origin_m": max(math.hypot(x[0], x[2]) for x in shifts),
            "successful_actions": len(observed)}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--viewer", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--cases", nargs="+", choices=[
        "mirrors", "glass-driver", "glass-exterior", "wet-driver", "wet-exterior",
        "collision", "snow-driver", "snow-exterior", "snow-low-quality"])
    a = p.parse_args()
    a.repo = Path(__file__).resolve().parents[1]
    a.viewer, a.out_dir = a.viewer.resolve(strict=True), a.out_dir.resolve()
    if a.out_dir.exists():
        p.error("Use a fresh output directory; do not reuse old screenshots")
    a.out_dir.mkdir(parents=True)
    identity = {"viewer_sha256": digest(a.viewer), "viewer_bytes": a.viewer.stat().st_size,
                "qa_helper_sha256": digest(Path(__file__)),
                "shaders": {str(x.relative_to(a.repo)): digest(x) for x in
                            (a.repo / "crates/openrailsrs-bevy-scenery/assets/shaders").glob("*.wgsl")},
                "fixtures": {str(x.relative_to(a.repo)): digest(x) for x in
                             (a.repo / "examples/player_lab").rglob("*") if x.is_file()},
                "scope": "Owned fixtures; actual pause/restore, camera changes and observed origin shift. No OR pixel parity or exclusive FPS claim."}
    (a.out_dir / "identity.json").write_text(json.dumps(identity, indent=2) + "\n")
    cases = [("mirrors", "driver", "rain", "high"),
             ("glass-driver", "driver", "clear", "high"),
             ("glass-exterior", "orbit", "clear", "high"),
             ("wet-driver", "driver", "rain", "high"),
             ("wet-exterior", "orbit", "rain", "high"),
             ("collision", "orbit", "clear", "high"),
             ("snow-driver", "driver", "snow", "high"),
             ("snow-exterior", "orbit", "snow", "high"),
             ("snow-low-quality", "orbit", "snow", "low")]
    reports = {}
    for name, view, weather, quality in cases:
        if a.cases and name not in a.cases:
            continue
        source = a.repo / "examples/player_lab" / (
            "scenario_jacobs_curve.toml" if name.startswith("snow") else "scenario.toml")
        path = scenario(a, source, name)
        settings(a, improved_transparency=True,
                 quality_limits={"enabled": False, "target_fps": 60, "min_level": 0, "max_level": 3})
        o = options(a, path, source.parent / "route")
        o.capture_or_focus = True
        o.follow, o.weather = view, weather
        o.profile_systems = True
        o.scenery_quality = quality
        o.weather_quality = quality
        o.after_ready = False
        o.capture_delay_s, o.timeout_s = 30, 90
        o.ready_frames = 10
        o.framepace = "30"
        actions = sequence(view)
        if name.startswith("snow"):
            o.weather_profile = "heavy_snow"
        if name == "collision":
            actions = [
                action(1, "fly_camera", position_m=[-65., 2.5, 20.], look_at_m=[-65., 2.5, 30.]),
                action(2, "fly_camera", position_m=[-72., 2.5, 20.], look_at_m=[-72., 2.5, 30.]),
                action(3, "fly_camera", position_m=[-72., 2.5, 29.], look_at_m=[-72., 2.5, 35.]),
                action(4, "pause", value=True), action(5, "restore"),
                action(7, "fly_camera", position_m=[500., 30., 0.], look_at_m=[0., 0., 0.]),
                action(11, "fly_camera", position_m=[-72., 2.5, 20.], look_at_m=[-72., 2.5, 30.]),
                action(13, "fly_camera", position_m=[-72., 2.5, 29.], look_at_m=[-72., 2.5, 35.]),
            ]
        o.capture_actions = json.dumps(actions)
        report = run_checkpoint(o, name, 0, pause=False)
        report["subcriteria"] = validate_actions(report, actions)
        report["requested_quality"] = quality
        if name == "mirrors":
            mirrors = report["mirrors"]
            assert mirrors["views"] == 2 and mirrors["frames"] > 2 and mirrors["resolution"] == [256, 160], mirrors
        if name == "collision":
            assert report["camera_collision"]["corrections"] > 0, report["camera_collision"]
            assert report["obstacles"]["solid_count"] > 0, report["obstacles"]
            assert report["camera"]["position_world"][2] < -18.2, report["camera"]
        if name.startswith("wet"):
            assert report["wetness"] > .05, report["wetness"]
        if name.startswith("snow"):
            assert report["snow_cover"] > .05, report["snow_cover"]
            expected_level = 2 if quality == "low" else 0
            assert report["weather_particles"]["quality_level"] == expected_level, report["weather_particles"]
        report["capture_sha256"] = digest((a.out_dir / name).with_suffix(".png"))
        reports[name] = report
        (a.out_dir / "report.json").write_text(json.dumps(reports, indent=2) + "\n")
        print("PASS subcriteria " + name, flush=True)


if __name__ == "__main__":
    main()

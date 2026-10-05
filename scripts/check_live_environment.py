#!/usr/bin/env python3
"""Capture actual manual weather/lightning and optionally the live provider.

Reuses the bounded native scenery capture runner. Runs one renderer at a time;
no provider override, synthetic screenshot, or visual baseline rewrite.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
from types import SimpleNamespace

from check_viewer_streaming import run_checkpoint


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, default=root / "tmp/live-environment")
    parser.add_argument("--online", action="store_true", help="explicit public Open-Meteo integration")
    parser.add_argument("--software", action="store_true")
    parser.add_argument("--case", action="append", help="capture only a named case; repeat for several cases")
    args = parser.parse_args()
    args.out_dir = args.out_dir.resolve()
    args.out_dir.mkdir(parents=True, exist_ok=True)
    source = root / "examples/chiltern_local/scenario.toml"
    text = source.read_text()
    text = text.replace('path = "."', "path = " + json.dumps(str(source.parent)))
    text = text.replace('consist = "../chiltern/consists/birmingham_pullman.con"',
                        "consist = " + json.dumps(str(root / "examples/chiltern/consists/birmingham_pullman.con")))
    night = args.out_dir / "storm-night.toml"
    night.write_text(re.sub(r"^start_time_s = .*", "start_time_s = 82800", text, flags=re.M))
    captures = {}
    cases = [("manual-clear", source, "clear", False, False, False),
             ("manual-storm-night", night, "storm", False, False, True)]
    if args.online:
        cases.extend([("local-clock-manual-snow", source, "snow", True, False, False),
                      ("manual-clock-local-weather", source, "clear", False, True, False),
                      ("local-clock-local-weather", source, "clear", True, True, False)])
    if args.case:
        unknown = set(args.case) - {case[0] for case in cases}
        if unknown:
            parser.error(f"Unknown or online-disabled cases: {sorted(unknown)}")
        cases = [case for case in cases if case[0] in args.case]
    for name, scenario, weather, real_time, real_weather, lightning in cases:
        options = SimpleNamespace(
            repo=root, viewer=root / "target/debug/openrailsrs-viewer3d",
            scenario=scenario, route_root=args.route_root.resolve(), out_dir=args.out_dir,
            software=args.software, headless_wayland=not args.software,
            require_hardware=not args.software, timeout_s=180, max_rss_mib=6144,
            follow="driver", view_radius_m=900, weather=weather, speed_mul=1,
            autodrive=0, ready_frames=8, real_time=real_time, real_weather=real_weather,
            during_lightning=lightning,
        )
        report = run_checkpoint(options, name, 0, True)
        environment = report["environment"]
        assert environment["selection"]["time"] == ("local_now" if real_time else "manual")
        assert environment["selection"]["weather"] == ("local_now" if real_weather else "manual")
        if not real_weather:
            assert environment["effective_weather"] == weather
            assert environment["sample"] is None
        if real_time:
            assert environment["timezone"] == "Europe/London" and environment["local_clock"]
            assert report["clock_time_s"] == 35700, "wall clock changed timetable"
        if real_weather:
            assert environment["sample"], environment["status"]
            assert environment["timezone"] == "Europe/London"
            assert report["clock_time_s"] == 35700, "weather changed timetable"
        if lightning:
            assert report["storm"]["strikes"] >= 1 and report["storm"]["flash"] > 0.3
            assert report["solar_direction"][1] < 0 and report["weather_particles"]["gpu_particles"] + report["weather_particles"]["cpu_particles"] > 0
        if not real_time and not real_weather:
            assert environment["local_clock"] is None and environment["status"] == ""
        captures[name] = {"sha256":hashlib.sha256((args.out_dir / f"{name}.png").read_bytes()).hexdigest(),
                          "environment":environment,"storm":report["storm"],"solar_direction":report["solar_direction"],
                          "peak_rss_mib":report["peak_rss_mib"],"renderer":report["renderer"],"shader_pipelines":report["shader_pipelines"]}
    (args.out_dir / "report.json").write_text(json.dumps({"passed":True,"captures":captures}, indent=2)+"\n")
    print("PASS: independent clocks/weather and actual lightning renderer", flush=True)


if __name__ == "__main__":
    main()

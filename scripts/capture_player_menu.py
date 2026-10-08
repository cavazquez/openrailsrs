#!/usr/bin/env python3
"""Render start screens on a private Weston display and check persistent actions.

Requires Weston with its headless GL backend. Never attaches to the player's
display, downloads content or writes their settings. Screenshots include UI
bounds exported by the viewer, in addition to its usual renderer report.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


PAGES = (
    "home", "route", "train", "weather", "weather-options", "continue",
    "library", "downloads", "settings", "weather-settings", "controls",
)


def check_layout(report, page, width, height):
    nodes = {node["name"]: node for node in report["ui_layout"]}
    root = nodes["player-menu"]

    def inside(node, container):
        for axis in range(2):
            assert node["min"][axis] >= container["min"][axis] - 1, node
            assert node["max"][axis] <= container["max"][axis] + 1, node
            assert node["max"][axis] > node["min"][axis], node

    inside(root, {"min": [0, 0], "max": [width, height]})
    for text in report["ui_text"]:
        if text.get("visible", True):
            assert all(size > 0 for size in text["size"]), "Collapsed text: " + text["text"]
    if page == "home":
        cards = [nodes["home-" + title] for title in (
            "Nueva partida", "Continuar", "Biblioteca", "Ajustes",
        )]
        for card in cards:
            inside(card, root)
        for before, after in zip(cards, cards[1:]):
            assert before["max"][1] <= after["min"][1], cards
        inside(nodes["ui-Exit"], root)
    elif page in ("route", "train", "weather", "weather-options"):
        footer = nodes["launch-footer"]
        inside(footer, root)
        inside(nodes.get("ui-Start", nodes.get("launch-unavailable")), footer)
        for name, node in nodes.items():
            if name.startswith("ui-NewGameStep("):
                inside(node, root)
    elif page in ("settings", "weather-settings", "controls"):
        inside(nodes["ui-SaveSettings"], root)
    pipelines = report.get("shader_pipelines")
    if pipelines:
        assert pipelines["failed"] == 0, pipelines


def capture(args):
    repo = Path(__file__).resolve().parents[1]
    output = args.out_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    viewer = args.viewer.resolve()
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("OPENRAILSRS_")}
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    env.update(OPENRAILSRS_DISABLE_AUDIO="1", OPENRAILSRS_SCREENSHOT_MENU="1",
               OPENRAILSRS_SCREENSHOT_DELAY_S="6",
               OPENRAILSRS_WINDOW_WIDTH=str(args.width),
               OPENRAILSRS_WINDOW_HEIGHT=str(args.height),
               __EGL_VENDOR_LIBRARY_FILENAMES="/usr/share/glvnd/egl_vendor.d/50_mesa.json")
    results = []
    with tempfile.TemporaryDirectory(prefix="openrailsrs-menu-qa-") as runtime:
        env.update(XDG_RUNTIME_DIR=runtime, WAYLAND_DISPLAY="openrailsrs-menu-qa")
        with (output / "weston.log").open("w") as log:
            display = subprocess.Popen([
                "weston", "--backend=headless", "--renderer=gl", "--no-config",
                "--socket=" + env["WAYLAND_DISPLAY"], "--idle-time=0",
                "--width=" + str(args.width), "--height=" + str(args.height),
            ], env=env, stdout=log, stderr=log)
            process = None
            try:
                deadline = time.monotonic() + 15
                while not (Path(runtime) / env["WAYLAND_DISPLAY"]).exists():
                    assert display.poll() is None and time.monotonic() < deadline, "Weston did not start"
                    time.sleep(0.05)
                for page in args.pages:
                    data = output / (page + "-player-data")
                    data.mkdir(exist_ok=True)
                    (data / "settings.json").write_text(json.dumps({
                        "ui_scale": args.scale, "weather_profile": args.weather_profile,
                        "weather_pace": args.weather_pace,
                        "environment": {"manual_weather": args.weather},
                    }))
                    shot = output / (page + ".png")
                    env.update(OPENRAILSRS_PLAYER_DIR=str(data),
                               OPENRAILSRS_SCREENSHOT=str(shot),
                               OPENRAILSRS_SCREENSHOT_MENU_PAGE=page)
                    command = [str(viewer), "--menu"]
                    if args.route_root:
                        command += ["--route-root", str(args.route_root.resolve())]
                    with (output / (page + ".log")).open("w") as viewer_log:
                        started = time.monotonic()
                        peak = 0
                        process = subprocess.Popen(command, cwd=repo, env=env,
                                                   stdout=viewer_log, stderr=viewer_log)
                        while process.poll() is None:
                            assert time.monotonic() - started < 60, "Capture timed out: " + page
                            try:
                                status = Path(f"/proc/{process.pid}/status").read_text()
                                rss = next(int(line.split()[1]) for line in status.splitlines()
                                           if line.startswith("VmRSS:"))
                                peak = max(peak, rss)
                                assert rss < 6144 * 1024, "Viewer exceeded 6 GiB RSS"
                            except (FileNotFoundError, StopIteration):
                                pass
                            time.sleep(0.1)
                        assert process.returncode == 0 and shot.is_file(), "Capture failed: " + page
                    report = json.loads(shot.with_suffix(".stream.json").read_text())
                    check_layout(report, page, args.width, args.height)
                    result = {"page": page, "width": args.width, "height": args.height,
                              "scale": args.scale, "elapsed_s": time.monotonic() - started,
                              "peak_rss_mib": peak / 1024, "exit_code": process.returncode}
                    results.append(result)
                    (output / "menu-qa.json").write_text(json.dumps(results, indent=2) + "\n")
                    print(json.dumps(result), flush=True)
            finally:
                if process and process.poll() is None:
                    process.kill()
                    process.wait()
                display.terminate()
                try:
                    display.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    display.kill()
                    display.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--viewer", type=Path, default=Path("target/debug/openrailsrs-viewer3d"))
    parser.add_argument("--out-dir", type=Path, default=Path("tmp/player-menu-qa"))
    parser.add_argument("--route-root", type=Path)
    parser.add_argument("--pages", nargs="+", choices=PAGES, default=list(PAGES))
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=720)
    parser.add_argument("--scale", type=float, default=1.0)
    parser.add_argument("--weather", choices=("clear", "rain", "fog", "snow", "overcast", "storm"), default="clear")
    parser.add_argument("--weather-profile", choices=("automatic", "drizzle", "steady_rain", "downpour", "light_snow", "steady_snow", "heavy_snow", "after_snow", "storm_cycle", "random_journey"), default="automatic")
    parser.add_argument("--weather-pace", choices=("slow", "normal", "fast"), default="normal")
    args = parser.parse_args()
    if args.width < 640 or args.height < 360 or not 0.8 <= args.scale <= 1.5:
        parser.error("Use at least 640×360 and an interface scale between 0.8 and 1.5")
    capture(args)


if __name__ == "__main__":
    main()

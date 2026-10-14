#!/usr/bin/env python3
"""Click the real itinerary editor at 800x600 on an isolated X11 display.

This is a UI interaction test using software Vulkan, not a performance result.
Each stage replays the already verified input prefix in a fresh editor, then
captures its actual layout. Scrolling uses XTEST wheel events, never hidden UI
commands or edited saves. Needs python-xlib, Xvfb and Mesa lavapipe.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import tomllib

from Xlib import X, display
from Xlib.ext import xtest

from capture_player_menu import check_layout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--viewer", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    a = p.parse_args()
    a.viewer = a.viewer.resolve(strict=True)
    a.out_dir = a.out_dir.resolve()
    if a.out_dir.exists():
        p.error("Use a fresh output directory")
    a.out_dir.mkdir(parents=True)
    repo = Path(__file__).resolve().parents[1]
    # This owned example is deliberately one-way. A separate fixture adds the
    # directed return edges needed to test successful reversals; author routes
    # and the tracked example are never altered to manufacture that success.
    original_graph = repo / "examples/player_lab/route/track.toml"
    original_graph_sha = digest(original_graph)
    resources = a.out_dir / "resources"
    shutil.copytree(repo / "examples/player_lab", resources / "examples/player_lab")
    shutil.copy2(repo / "Cargo.toml", resources / "Cargo.toml")
    (resources / "crates").symlink_to(repo / "crates", target_is_directory=True)
    private_graph = resources / "examples/player_lab/route/track.toml"
    graph = tomllib.loads(private_graph.read_text())
    with private_graph.open("a") as stream:
        for edge in graph["edges"]:
            values = {**edge, "id": edge["id"] + "_r", "from": edge["to"],
                      "to": edge["from"], "grade_percent": -edge.get("grade_percent", 0)}
            stream.write("\n[[edges]]\n")
            for key, value in values.items():
                stream.write(key + " = " + json.dumps(value) + "\n")
    source = resources / "examples/player_lab/scenario.toml"
    original = digest(source)
    inputs = []
    stages = []
    env = {k: v for k, v in os.environ.items() if not k.startswith("OPENRAILSRS_")}
    env.pop("WAYLAND_DISPLAY", None)
    env.update(OPENRAILSRS_RESOURCES=str(resources),
               OPENRAILSRS_DISABLE_AUDIO="1", OPENRAILSRS_SCREENSHOT_MENU="1",
               OPENRAILSRS_SCREENSHOT_MENU_PAGE="service-editor",
               OPENRAILSRS_WINDOW_WIDTH="800", OPENRAILSRS_WINDOW_HEIGHT="600",
               OPENRAILSRS_RENDERER="cpu", WGPU_BACKEND="vulkan",
               VK_ICD_FILENAMES="/usr/share/vulkan/icd.d/lvp_icd.json")
    if not Path(env["VK_ICD_FILENAMES"]).is_file():
        env["VK_ICD_FILENAMES"] = "/usr/share/vulkan/icd.d/lvp_icd.x86_64.json"
    read_fd, write_fd = os.pipe()
    with (a.out_dir / "xvfb.log").open("w") as log:
        xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(write_fd), "-screen", "0",
                                 "800x600x24", "-nolisten", "tcp"],
                                pass_fds=(write_fd,), stdout=log, stderr=log)
        os.close(write_fd)
        with os.fdopen(read_fd) as number:
            env["DISPLAY"] = ":" + number.readline().strip()
        connection = display.Display(env["DISPLAY"])
        viewer = None

        def capture(name):
            nonlocal viewer
            stage = a.out_dir / name
            stage.mkdir()
            data = stage / "player"
            data.mkdir()
            (data / "settings.json").write_text('{"ui_scale":1.0}')
            image = stage / "editor.png"
            delay = 6 + len(inputs) * .9
            env.update(OPENRAILSRS_PLAYER_DIR=str(data), OPENRAILSRS_SCREENSHOT=str(image),
                       OPENRAILSRS_SCREENSHOT_DELAY_S=str(delay))
            started_ns = time.time_ns()
            started = time.monotonic()
            with (stage / "viewer.log").open("w") as log:
                viewer = subprocess.Popen([str(a.viewer), "--menu"], cwd=repo, env=env,
                                          stdout=log, stderr=log)
                deadline = started + 20
                window = None
                while window is None:
                    assert viewer.poll() is None and time.monotonic() < deadline, "No editor window"
                    for candidate in connection.screen().root.query_tree().children:
                        if "openrailsrs" in (candidate.get_wm_name() or "").lower():
                            window = candidate
                            break
                    time.sleep(.1)
                connection.set_input_focus(window, X.RevertToParent, X.CurrentTime)
                connection.sync()
                time.sleep(2)
                for item in inputs:
                    if item["kind"] == "click":
                        x, y = item["center"]
                        xtest.fake_input(connection, X.MotionNotify, x=round(x), y=round(y))
                        connection.sync()
                        time.sleep(.1)
                        xtest.fake_input(connection, X.ButtonPress, 1)
                        connection.sync()
                        time.sleep(.08)
                        xtest.fake_input(connection, X.ButtonRelease, 1)
                    else:
                        xtest.fake_input(connection, X.MotionNotify, x=600, y=400)
                        for _ in range(item["notches"]):
                            xtest.fake_input(connection, X.ButtonPress, 5)
                            xtest.fake_input(connection, X.ButtonRelease, 5)
                            connection.sync()
                            time.sleep(.08)
                    connection.sync()
                    time.sleep(.65)
                while viewer.poll() is None:
                    assert time.monotonic() - started < delay + 20, "Editor capture timed out"
                    time.sleep(.1)
            report = json.loads(image.with_suffix(".stream.json").read_text())
            assert viewer.returncode == 0 and image.is_file()
            assert image.stat().st_mtime_ns >= started_ns
            assert f"screenshot saved → {image}" in (stage / "viewer.log").read_text()
            check_layout(report, "service-editor", 800, 600)
            stages.append({"name": name, "exit_code": viewer.returncode,
                           "elapsed_s": time.monotonic() - started,
                           "input_prefix": list(inputs), "capture_sha256": digest(image),
                           "renderer": report.get("renderer"), "ui_layout": report["ui_layout"],
                           "ui_text": report["ui_text"]})
            (a.out_dir / "stages.json").write_text(json.dumps(stages, indent=2) + "\n")
            print("PASS editor stage " + name, flush=True)
            return report, data

        def click(report, name):
            nodes = {n["name"]: n for n in report["ui_layout"]}
            node, scroll = nodes[name], nodes["player-scroll"]
            assert node["min"][1] >= scroll["min"][1] and node["max"][1] <= scroll["max"][1], node
            inputs.append({"kind": "click", "name": name,
                           "center": [(lo + hi) / 2 for lo, hi in zip(node["min"], node["max"])]})

        try:
            report, _ = capture("00-map")
            nodes = {n["name"]: n for n in report["ui_layout"]}
            bounds = nodes["itinerary-map"]
            assert bounds["max"][1] - bounds["min"][1] >= 219, bounds
            click(report, "itinerary-node:mid")
            report, _ = capture("01-point")
            assert any("yard_a → mid" in x["text"] for x in report["ui_text"])
            click(report, "ui-ItineraryWait(30.0)")
            report, _ = capture("02-wait")
            assert any("espera 30 s" in x["text"] for x in report["ui_text"])
            click(report, "ui-ItineraryReverse")
            report, _ = capture("03-reverse")
            assert any("mid → yard_a" in x["text"] and "invertido" in x["text"] for x in report["ui_text"])
            inputs.append({"kind": "scroll_down", "notches": 7})
            report, _ = capture("04-scroll")
            click(report, "ui-ServiceUndo")
            report, _ = capture("05-undo")
            assert any("yard_a → mid" in x["text"] for x in report["ui_text"])
            assert not any("mid → yard_a" in x["text"] for x in report["ui_text"])
            # The production rebuild after undo returns this scroll view to
            # its top. Scroll through the visible UI again before saving.
            inputs.append({"kind": "scroll_down", "notches": 7})
            report, _ = capture("06-scroll-after-undo")
            click(report, "ui-ServiceSave")
            report, data = capture("07-save")
            saved = list((data / "custom-services").rglob("*.toml"))
            assert len(saved) == 1, saved
            scenario = tomllib.loads(saved[0].read_text())
            legs = scenario["route"]["legs"]
            assert len(legs) == 1 and legs[0]["start"] == "yard_a" and legs[0]["destination"] == "mid", legs
            assert legs[0]["wait_s"] == 30 and not legs[0].get("reversed", False), legs
            assert digest(source) == original, "Source scenario changed"
            assert digest(original_graph) == original_graph_sha, "Tracked source graph changed"
            receipt = {"status": "pass", "viewer_sha256": digest(a.viewer), "window": [800, 600],
                       "source_fixture_sha256_before_and_after": original,
                       "tracked_original_graph_sha256_before_and_after": original_graph_sha,
                       "owned_private_bidirectional_graph_sha256": digest(private_graph),
                       "fixture_change": "Copied owned example; explicitly added directed reverse edges for successful inversion testing",
                       "saved_scenario_sha256": digest(saved[0]), "saved_itinerary": legs,
                       "stages": len(stages), "actual_mouse_input": True,
                       "scope": "Private XTEST input with software Vulkan; no gameplay or FPS claim. Fresh map/pause GPU captures are separate."}
            (a.out_dir / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        finally:
            if viewer is not None and viewer.poll() is None:
                viewer.terminate()
                viewer.wait(timeout=10)
            connection.close()
            xvfb.terminate()
            xvfb.wait(timeout=10)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""One declared protocol for native OR 1.6.1 and Bevy station captures.

Uses immutable existing native views by default. --capture-native regenerates
views in a private runtime/prefix, preserving the references and failed runs.
Neither importing this module nor --plan-only launches a renderer.
"""
import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time
import tomllib

from capture_chiltern_service_or import clone_prefix
from compare_station_views import compare, TOLERANCES
from oracle_wine import owned_processes
from run_oracles import verify

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / "docs/fixtures/visual/or_reference/chiltern_station_views"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def load_plan(path, selected=()):
    plan = json.loads(Path(path).read_text())
    expected = {"clock_time_s": 35700, "weather": "clear", "season": "summer",
                "width": 1280, "height": 720, "view_radius_m": 450, "cab_fov_deg": 45,
                "orbit_yaw_rad": 1.6, "orbit_pitch_rad": 0.6, "orbit_distance_m": 160}
    if plan.get("schema") != 1 or plan.get("reference_version") != "1.6.1" or plan.get("conditions") != expected:
        raise ValueError("This native adapter requires the declared 1.6.1 conditions")
    stations = plan.get("stations", [])
    names = [s.get("slug", "") for s in stations]
    if not names or len(names) != len(set(names)) or not all(re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", s) for s in names):
        raise ValueError("Station names must be unique safe filenames")
    if set(selected)-set(names):
        raise ValueError("Unknown selected station")
    for station in stations:
        if "platform_id" in station:
            if type(station["platform_id"]) is not int or station["platform_id"] < 0:
                raise ValueError("Invalid native platform")
        elif not isinstance(station.get("advance_m"), (float, int)) or not 0 <= station["advance_m"] <= 100_000:
            raise ValueError("Invalid native advance")
    if selected:
        plan["stations"] = [s for s in stations if s["slug"] in selected]
    return plan


def reference_inputs(directory, stations):
    result = {}
    for s in stations:
        for view in ("exterior", "cab"):
            path = directory / (s["slug"] + "-" + view + ".json")
            data = json.loads(path.read_text())
            image = path.with_suffix(".png")
            if data.get("version") != "1.6.1" or digest(image) != data.get("image_sha256", data.get("png_sha256")):
                raise ValueError("Changed native reference: " + str(path))
            for p in (path, image):
                result[str(p.resolve())] = digest(p)
    return result


def stop_private_wine(prefix, env):
    server = shutil.which("wineserver") or shutil.which("wineserver-stable")
    if server:
        subprocess.run([server, "-k"], env=env, check=False, timeout=15,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for row in owned_processes(prefix):
            current = next((r for r in owned_processes(prefix) if r["pid"] == row["pid"] and r["start_ticks"] == row["start_ticks"]), None)
            if current:
                try:
                    os.kill(row["pid"], sig)
                except ProcessLookupError:
                    pass
        time.sleep(0.2)
    if owned_processes(prefix):
        raise RuntimeError("Private native renderer did not stop")


def gallery(out, stations, comparison):
    parts = ['<!doctype html><meta charset="utf-8"><title>Open Rails / Bevy</title>',
             '<style>body{background:#111;color:#eee;font:16px sans-serif;margin:2rem}section{margin:2rem 0}figure{display:inline-block;width:48%;margin:.5%}img{width:100%}</style>',
             '<h1>Open Rails 1.6.1 / Bevy</h1><p>Camera and condition checks: ' + str(comparison['passed']) +
             '. Inspect geometry, materials and authored resources; lighting differences may be intentional.</p>']
    for s in stations:
        for view in ("exterior", "cab"):
            slug = s["slug"]
            label = slug + ("-cab" if view == "cab" else "")
            parts.append('<section><h2>' + html.escape(label) + '</h2><figure><figcaption>Open Rails</figcaption><img src="native/' + slug + '-' + view + '.png"></figure><figure><figcaption>Bevy</figcaption><img src="bevy/' + label + '.png"></figure></section>')
    (out / "comparison.html").write_text("\n".join(parts), encoding="utf-8")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--plan", type=Path, default=ROOT / "examples/visual_comparison/chiltern.json")
    p.add_argument("--station", action="append", default=[])
    p.add_argument("--scenario", type=Path, default=ROOT / "examples/chiltern_extended/scenario.toml")
    p.add_argument("--route-root", type=Path, required=True)
    p.add_argument("--viewer", type=Path, default=ROOT / "target/debug/openrailsrs-viewer3d")
    p.add_argument("--references", type=Path, default=REFERENCE)
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--plan-only", action="store_true")
    p.add_argument("--capture-native", action="store_true")
    p.add_argument("--activity", type=Path)
    p.add_argument("--installation-root", type=Path, default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    p.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    p.add_argument("--software", action="store_true")
    p.add_argument("--headless-wayland", action="store_true")
    args = p.parse_args()
    plan = load_plan(args.plan, args.station)
    out = args.out_dir.resolve()
    for original in (args.references, args.route_root, args.installation_root, args.wine_source_prefix, ROOT.parent / "openrails"):
        protected = original.resolve()
        if out.is_relative_to(protected) or protected.is_relative_to(out):
            raise ValueError("Output must be separate from original inputs")
    if out.exists():
        raise ValueError("Use a fresh output directory; prior attempts are evidence")
    out.mkdir(parents=True)
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root if args.capture_native else None)
    frozen = reference_inputs(args.references, plan["stations"])
    scenario=tomllib.loads(args.scenario.read_text())
    formation=(args.scenario.parent/scenario["train"]["consist"]).resolve()
    if digest(formation)!=plan["formation"]["sha256"]:
        raise ValueError("The selected formation differs from the predeclared visual protocol")
    files=[args.plan,args.scenario,formation,Path(__file__),ROOT/"scripts/capture_route_views.py",
           ROOT/"scripts/compare_station_views.py",ROOT/"scripts/capture_openrails_station.py",
           ROOT/"tools/openrails-reference/ExtendedSceneryCapture.cs",
           (args.scenario.parent/scenario["route"]["path"]/"track.toml").resolve()]
    # Hash author definitions and the physical database without copying them
    # into version control. Texture/shape coverage remains the native audit's
    # job; the receipt never claims a byte audit of an entire route installation.
    files.extend(p for p in args.scenario.parent.rglob("*") if p.suffix.lower() in (".eng",".wag"))
    files.extend(p for p in args.route_root.iterdir() if p.is_file() and p.suffix.lower() in (".tdb",".trk"))
    if args.activity is not None: files.append(args.activity)
    if args.capture_native:
        files.extend(args.installation_root/e["name"] for e in [*pin["binaries"],*pin.get("scenery_binaries",[])])
    inputs = {str(x.resolve()): digest(x) for x in files}
    inputs.update(frozen)
    receipt = {"schema": 1, "status": "planned", "plan": plan, "limits": TOLERANCES,
               "reference_commit": pin["commit"], "input_sha256": inputs, "commands": [],
               "native_mode": "fresh_private_capture" if args.capture_native else "immutable_reference_replay"}
    record = out / "manifest.json"
    record.write_text(json.dumps(receipt, indent=2) + "\n")
    if args.plan_only:
        return
    receipt["viewer_sha256"] = digest(args.viewer)
    inputs[str(args.viewer.resolve())]=receipt["viewer_sha256"]
    native = out / "native"
    native.mkdir()
    def run(command, label, env=None):
        receipt["commands"].append(list(map(str, command)))
        record.write_text(json.dumps(receipt, indent=2) + "\n")
        with (out / (label + ".log")).open("w") as log:
            subprocess.run(list(map(str, command)), env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    try:
        if args.capture_native:
            if args.activity is None or not args.activity.is_file():
                raise ValueError("Fresh captures require an installed original activity")
            prefix = out / "private-prefix"
            runtime = out / "private-runtime"
            clone_prefix(args.wine_source_prefix, prefix)
            shutil.copytree(args.installation_root, runtime)
            private_run = out / "private-xdg"
            private_run.mkdir(mode=0o700)
            env = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "XAUTHORITY") and not k.startswith("OPENRAILS_REFERENCE_")}
            env.update(WINEPREFIX=str(prefix), WINEDEBUG="-all", XDG_RUNTIME_DIR=str(private_run), WINEDLLOVERRIDES="winex11.drv,winewayland.drv=d")
            try:
                run(["wine", "reg", "add", r"HKCU\Software\Wine\WineDbg", "/v", "ShowCrashDialog", "/t", "REG_DWORD", "/d", "0", "/f"], "native-disable-dialog", env)
                for station in plan["stations"]:
                    for view in ("exterior", "cab"):
                        label = station["slug"] + "-" + view
                        cmd = [sys.executable, ROOT / "scripts/capture_openrails_station.py", "--prefix", prefix, "--runtime", runtime, "--activity", args.activity.resolve(), "--out-dir", native, "--label", label, "--view", view]
                        if "platform_id" in station:
                            cmd += ["--platform-id", str(station["platform_id"])]
                        else:
                            cmd += ["--advance-m", str(station["advance_m"])]
                        run(cmd, label, env)
                        shutil.copy2(native / (label + "-metadata.json"), native / (label + ".json"))
            finally:
                stop_private_wine(prefix, env)
        else:
            for path in frozen:
                shutil.copy2(path, native / Path(path).name)
        command = [sys.executable, ROOT / "scripts/capture_route_views.py", "--route-root", args.route_root.resolve(), "--scenario", args.scenario.resolve(), "--viewer", args.viewer.resolve(), "--out-dir", out / "bevy", "--references", native, "--with-cab", "--capture-or-focus", "--match-native-lighting", "--clock-time-s", "35700", "--cab-fov-deg", "45", "--view-radius-m", "450", "--weather", "clear", "--camera-yaw", "1.6", "--camera-pitch", "0.6", "--camera-distance", "160"]
        for s in plan["stations"]:
            command += ["--station", s["slug"]]
        if args.software:
            command += ["--software"]
        if args.headless_wayland:
            command += ["--headless-wayland", "--require-hardware"]
        run(command, "bevy")
        result = compare(out / "bevy", [s["slug"] for s in plan["stations"]], references=native)
        (out / "comparison.json").write_text(json.dumps(result, indent=2) + "\n")
        gallery(out, plan["stations"], result)
        receipt["status"] = "pass" if result["passed"] else "failed_conditions"
        receipt["visual_review"] = "required; equivalence does not certify artistic or content parity"
        if not result["passed"]:
            raise RuntimeError("Equivalent view checks failed; retain attempt")
    except BaseException as error:
        receipt.update(status="failed", error=str(error))
        raise
    finally:
        receipt["original_inputs_unchanged"] = all(Path(path).is_file() and digest(path) == h for path, h in inputs.items())
        if not receipt["original_inputs_unchanged"]: receipt["status"]="failed_inputs_changed"
        receipt["artifacts"] = {str(path.relative_to(out)): digest(path) for directory in (native, out / "bevy") if directory.is_dir() for path in sorted(directory.glob("*")) if path.suffix in (".json", ".png")}
        record.write_text(json.dumps(receipt, indent=2) + "\n")
    if not receipt["original_inputs_unchanged"]:
        raise RuntimeError("Original inputs changed during comparison")


if __name__ == "__main__":
    main()

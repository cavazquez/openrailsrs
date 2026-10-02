#!/usr/bin/env python3
"""Capture the complete Chiltern local service using the pinned, original OR DLL.

All generated activity files, runtime assemblies and outputs stay in --out-dir.
Wine uses a private prefix; the reference installation and Content are read-only.
"""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib

from prepare_chiltern_service import msts_text, msts_blocks, PAT
from run_oracles import verify

ROOT = Path(__file__).resolve().parents[1]
NAME = "openrailsrs_chiltern_local"
HELPER = ROOT / "tools/openrails-reference/HeadlessCapture.cs"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def driver_keyframes(trace, output, step):
    """A row at t+dt records controls used during [t,t+dt), not the next step."""
    with trace.open() as source, output.open("w", newline="") as target:
        reader = csv.DictReader(source)
        writer = csv.writer(target, lineterminator="\n")
        writer.writerow(("time_s", "throttle", "brake"))
        previous = None
        last_written_time = None
        next(reader)  # Initial state has not yet consumed an input.
        for row in reader:
            controls = (float(row["throttle"]), float(row["brake"]))
            time = f'{float(row["time_s"]) - step:.8f}'
            if controls != previous:
                writer.writerow((time, *controls))
                previous = controls
                last_written_time = time
        # Keep the full replay horizon even when terminal brakes never change.
        if previous is not None and time != last_written_time:
            writer.writerow((time, *controls))


def windows_path(path):
    return str(path.resolve()) if os.name == "nt" else "Z:" + str(path.resolve()).replace("/", "\\")


def clone_prefix(source, destination):
    """Copy private files, preserving links *within* the copy, never to source."""
    marker = destination / ".openrailsrs-private-prefix.json"
    if destination.exists():
        if not marker.is_file() or json.loads(marker.read_text()).get("source") != str(source.resolve()):
            raise ValueError(f"Refusing an existing Wine prefix not created for this capture: {destination}")
        for required in ("system.reg", "user.reg", "drive_c/windows/Microsoft.NET/Framework64/v4.0.30319/csc.exe"):
            if not (destination / required).is_file():
                raise ValueError(f"Incomplete private Wine prefix: {destination / required}")
            if os.path.samefile(source / required, destination / required):
                raise ValueError(f"Private Wine prefix shares a writable reference file: {required}")
        return
    destination.mkdir(parents=True)
    copied = {}
    for entry in source.rglob("*"):
        relative = entry.relative_to(source)
        if any(part.startswith("NativeImages_v4.") for part in relative.parts):
            continue  # The installed CLR can JIT its managed assemblies.
        target = destination / relative
        if entry.is_symlink():
            target.parent.mkdir(parents=True, exist_ok=True)
            if relative.parts[:2] == ("drive_c", "users"):
                target.mkdir(exist_ok=True)  # Do not retain Desktop/Documents links.
            else:
                target.symlink_to(os.readlink(entry))
        elif entry.is_dir():
            target.mkdir(parents=True, exist_ok=True)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            stat = entry.stat()
            key = (stat.st_dev, stat.st_ino)
            if key in copied:
                os.link(copied[key], target)
            else:
                shutil.copy2(entry, target)
                copied[key] = target
    marker.write_text(json.dumps({"source": str(source.resolve())}) + "\n")


def validate_replay(path):
    previous = -1.0
    with path.open() as stream:
        reader = csv.DictReader(stream)
        if not {"time_s", "throttle", "brake"}.issubset(reader.fieldnames or []):
            raise ValueError("Replay needs time_s, throttle and brake columns")
        rows = list(reader)
    for row in rows:
        time, throttle, brake = (float(row[key]) for key in ("time_s", "throttle", "brake"))
        if not previous < time or not 0 <= throttle <= 1 or not 0 <= brake <= 1:
            raise ValueError("Replay needs increasing finite times and controls in [0, 1]")
        previous = time
    if len(rows) < 2 or float(rows[0]["time_s"]) != 0 or not previous <= 1800:
        raise ValueError("Replay must cover a finite interval from zero, at most 1800 seconds")


def facade(source, destination):
    """Share bulk Content and isolate the three directories holding new files."""
    route = destination / "ROUTES/Chiltern"
    route.mkdir(parents=True)
    content = source.parent.parent
    for entry in content.iterdir():
        if entry.name.upper() != "ROUTES":
            (destination / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    for entry in source.iterdir():
        if entry.name.upper() in {"ACTIVITIES", "SERVICES", "PATHS"}:
            target = route / entry.name.upper()
            target.mkdir()
            for child in entry.iterdir():
                (target / child.name).symlink_to(child, target_is_directory=child.is_dir())
        else:
            (route / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    return route


def write_native(path, text):
    # An existing native name in the Content facade is a read-only symlink.
    # Exclusive creation prevents a capture from overwriting its target.
    with path.open("x", encoding="utf-16") as stream:
        stream.write(text)


def activity_files(source, route, preparation, scenario, provenance):
    """The native PAT places the *rear*, unlike the Bevy service's head anchor."""
    original = msts_text(source / "PATHS" / PAT)
    points = [list(map(float, block.split())) for block in msts_blocks(original, "TrackPDP")]
    nodes = [block.split() for block in msts_blocks(original, "TrPathNode")]
    if preparation["rear"]["node"] != provenance["approach_vector_id"]:
        raise ValueError("Native consist rear is not on the retained approach vector")
    if preparation["cars"] != 8:
        raise ValueError("Native reference must contain the same eight Pullman vehicles")
    graph = tomllib.loads((ROOT / "examples/chiltern_local/track.toml").read_text())
    ordered = []
    for begin, end in zip(scenario["route"]["waypoints"], scenario["route"]["waypoints"][1:]):
        candidates = [edge for edge in graph["edges"] if edge["from"] == begin and edge["to"] == end]
        if len(candidates) != 1:
            raise ValueError(f"Ambiguous native service segment {begin} -> {end}")
        ordered.append(candidates[0])
    chainages = {}
    accumulated = 0.0
    for edge in ordered:
        chainages[edge["id"]] = accumulated
        accumulated += edge["length_m"]
    distances = [preparation["length_m"] + chainages[marker["edge_id"]] + marker["chainage_m"]
        - provenance["start_chainage_m"] for marker in provenance["station_markers"]]
    extra = []
    for key, kind in [("rear", 1), ("junction", 2)]:
        p = preparation[key]
        extra.append([p["tile_x"], p["tile_z"], p["x"], p["y"], p["z"], kind, 0])
    path = "SIMISA@@@@@@@@@@JINX0P0t______\n\nSerial ( 1 )\nTrackPDPs (\n"
    for point in extra + points:
        path += "  TrackPDP ( " + " ".join(format(number, ".9g") for number in point) + " )\n"
    path += f')\nTrackPath (\n  TrPathName ( "{NAME}" )\n  TrPathFlags ( 00000020 )\n'
    path += f'  Name ( "{NAME}" )\n  TrPathStart ( "Northolt Park" )\n  TrPathEnd ( "West Ruislip" )\n'
    path += f"  TrPathNodes ( {len(nodes) + 2}\n"
    path += "    TrPathNode ( 00000000 1 4294967295 0 )\n    TrPathNode ( 00000000 2 4294967295 1 )\n"
    for flag, main, siding, point in nodes:
        shifted = [str(int(value) + 2) if value != "4294967295" else value for value in (main, siding)]
        path += f"    TrPathNode ( {flag} {shifted[0]} {shifted[1]} {int(point) + 2} )\n"
    path += "  )\n)\n"
    write_native(route / "PATHS" / f"{NAME}.pat", path)

    service = f'''SIMISA@@@@@@@@@@JINX0v0t______
Service_Definition (
  Serial ( 1 ) Name ( "{NAME}" ) Train_Config ( "Birmingham Pullman" )
  PathID ( "{NAME}" ) MaxWheelAcceleration ( 0 ) Efficiency ( 0.75 )
  TimeTable ( StartingSpeed ( 0 ) EndingSpeed ( 0 ) StartInWorld ( 0 ) EndInWorld ( 0 )
'''
    for index, marker in enumerate(provenance["station_markers"]):
        service += f'    StationStop ( PlatformStartID ( {marker["item_id"]} ) DistanceDownPath ( {distances[index]:.6f} ) SkipCount ( {index} ) )\n'
    service += "  )\n)\n"
    write_native(route / "SERVICES" / f"{NAME}.srv", service)
    start = scenario["scenario"]["start_time_s"]
    hour, minute, second = start // 3600, start % 3600 // 60, start % 60
    activity = f'''SIMISA@@@@@@@@@@JINX0a0t______
Tr_Activity (
  Serial ( 1 )
  Tr_Activity_Header (
    RouteID ( Chiltern ) Name ( "{NAME}" )
    Description ( "OpenRails 1.6.1 reference: same eight-car local service as Bevy." )
    Briefing ( "Northolt Park - South Ruislip - West Ruislip." )
    CompleteActivity ( 1 ) Type ( 0 ) Mode ( 2 ) StartTime ( {hour} {minute} {second} )
    Season ( 1 ) Weather ( 0 ) PathID ( "{NAME}" ) StartingSpeed ( 0 )
    Duration ( 0 30 ) Difficulty ( 0 ) FuelWater ( 100 ) FuelCoal ( 100 ) FuelDiesel ( 100 )
  )
  Tr_Activity_File (
    Player_Service_Definition ( "{NAME}"
      Player_Traffic_Definition ( {start}
'''
    for index, (stop, marker) in enumerate(zip(scenario["route"]["stops"], provenance["station_markers"], strict=True)):
        activity += f'        ArrivalTime ( {int(start + stop["arrive_s"])} ) DepartTime ( {int(start + stop["depart_s"])} ) SkipCount ( {index} ) DistanceDownPath ( {distances[index]:.6f} ) PlatformStartID ( {marker["item_id"]} )\n'
    activity += "      )\n      UiD ( 0 )\n"
    for index, marker in enumerate(provenance["station_markers"]):
        activity += f'      Efficiency ( 0.75 ) SkipCount ( {index} ) DistanceDownPath ( {distances[index]:.6f} ) PlatformStartID ( {marker["item_id"]} )\n'
    activity += "    )\n    NextServiceUID ( 1 ) NextActivityObjectUID ( 1 )\n  )\n)\n"
    act = route / "ACTIVITIES" / f"{NAME}.act"
    write_native(act, activity)
    return act


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", type=Path, default=Path.home() / "Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern")
    parser.add_argument("--installation-root", type=Path, default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wine-prefix", type=Path, default=ROOT / "tmp/or161-wine-prefix")
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--replay", type=Path, help="Diagnostic replay of the same normalized Bevy controls")
    args = parser.parse_args()
    out = args.out_dir.resolve()
    for protected in (args.route_root.parent.parent, args.installation_root, args.wine_source_prefix):
        if out.is_relative_to(protected.resolve()) or args.wine_prefix.resolve().is_relative_to(protected.resolve()):
            raise ValueError(f"Generated files must stay outside the original reference: {protected}")
    if out.exists():
        raise ValueError(f"Output already exists; choose a fresh capture directory: {out}")
    if args.replay:
        validate_replay(args.replay)
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    scenario = tomllib.loads((ROOT / "examples/chiltern_local/scenario.toml").read_text())
    provenance = json.loads((ROOT / "examples/chiltern_local/provenance.json").read_text())
    if digest(args.route_root / "Chiltern.tdb") != provenance["tdb_sha256"]:
        raise ValueError("Chiltern TDB differs from the selected Bevy service")
    if digest(args.route_root / "PATHS" / PAT) != provenance["pat_sha256"]:
        raise ValueError("Chiltern PAT differs from the selected Bevy service")
    out.mkdir(parents=True)
    runtime = out / "runtime"
    shutil.copytree(args.installation_root, runtime)
    shutil.copy2(HELPER, runtime / HELPER.name)
    shutil.copy2(HELPER, out / HELPER.name)
    env = os.environ.copy()
    if os.name == "nt":
        command = []
        compiler = str(Path(os.environ["WINDIR"]) / "Microsoft.NET/Framework64/v4.0.30319/csc.exe")
    else:
        clone_prefix(args.wine_source_prefix, args.wine_prefix)
        env.update(WINEPREFIX=str(args.wine_prefix.resolve()), WINEARCH="win64", WINEDEBUG="-all")
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        command = ["wine"]
        compiler = "C:\\windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe"
    with (out / "compile.log").open("w") as log:
        subprocess.run(command + [compiler, "/nologo", "/target:exe", "/out:HeadlessCapture.exe"] +
            ["/r:" + name for name in ["Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
                "Orts.Formats.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]] + [HELPER.name],
            cwd=runtime, env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    helper = command + [str(runtime / "HeadlessCapture.exe")]
    original_pat = args.route_root / "PATHS" / PAT
    consist = args.route_root.parent.parent / "TRAINS/CONSISTS/Birmingham Pullman.con"
    print("Running the original, pinned Open Rails geometry and consist loader...", flush=True)
    with (out / "prepare.log").open("w") as log:
        subprocess.run(helper + ["prepare", windows_path(original_pat), windows_path(consist), windows_path(out / "preparation.json")],
            cwd=runtime, env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    preparation = json.loads((out / "preparation.json").read_text())
    route = facade(args.route_root.resolve(), out / "content/Chiltern")
    act = activity_files(args.route_root, route, preparation, scenario, provenance)
    config = out / "config.json"
    config.write_text(json.dumps({"step_s": scenario["simulation"]["time_step"],
        "stops": scenario["route"]["stops"]}, indent=2) + "\n")
    mode = "replay" if args.replay else "service"
    capture_args = [mode, windows_path(act), windows_path(out / "capture"), windows_path(config)]
    if args.replay:
        capture_args.append(windows_path(args.replay))
    print("Capturing the complete service with the original simulation DLL...", flush=True)
    with (out / "capture.log").open("w") as log:
        subprocess.run(helper + capture_args, cwd=runtime, env=env, stdout=log,
            stderr=subprocess.STDOUT, check=True, timeout=600)
    driver_keyframes(out / "capture/trace.csv", out / "driver.csv", scenario["simulation"]["time_step"])
    native_files = {}
    content = args.route_root.parent.parent
    for filename in {vehicle["source"] for vehicle in preparation["vehicles"]}:
        path = content / "TRAINS/TRAINSET/RF_Blue_Pullman" / filename
        native_files[str(path.relative_to(content))] = digest(path)
    for path in [content / "GLOBAL/tsection.dat", args.route_root / "tsection.dat",
            args.route_root / "OpenRails/tsection.dat", args.route_root / "Chiltern.trk",
            args.route_root / "OpenRails/Chiltern.trk", args.route_root / "sigcfg.dat",
            args.route_root / "sigscr.dat"]:
        if path.is_file():
            native_files[str(path.relative_to(content))] = digest(path)
    manifest = {"version": pin["version"], "source_commit": pin["commit"], "mode": mode,
        "step_s": 0.05, "seed": 0, "helper_sha256": digest(runtime / HELPER.name),
        "inputs": {"tdb": digest(args.route_root / "Chiltern.tdb"), "original_pat": digest(original_pat),
            "consist": digest(consist), "scenario": digest(ROOT / "examples/chiltern_local/scenario.toml")},
        "binaries": {entry["name"]: digest(runtime / entry["name"]) for entry in pin["binaries"]},
        "generated": {name: digest(path) for name, path in [("activity", act),
            ("service", route / "SERVICES" / f"{NAME}.srv"), ("path", route / "PATHS" / f"{NAME}.pat")]},
        "native_files": native_files,
        "trace_sha256": digest(out / "capture/trace.csv"),
        "driver_sha256": digest(out / "driver.csv")}
    manifest["config_sha256"] = digest(config)
    manifest["runtime_assemblies"] = {path.name: digest(path) for path in sorted(runtime.glob("*.dll"))}
    generated_files = [act, route / "SERVICES" / f"{NAME}.srv", route / "PATHS" / f"{NAME}.pat"]
    captured_files = [out / "capture" / name for name in ("trace.csv", "initial.json", "outcome.json")]
    manifest["files"] = {str(path.relative_to(out)): digest(path) for path in generated_files + captured_files +
        [config, out / "driver.csv", out / "preparation.json", out / HELPER.name]}
    if args.replay:
        manifest["replay_sha256"] = digest(args.replay)
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    outcome = json.loads((out / "capture/outcome.json").read_text())
    print(json.dumps(outcome, indent=2))
    if mode == "service" and not outcome["success"]:
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"Original Open Rails capture failed: {error}", file=sys.stderr)
        sys.exit(1)

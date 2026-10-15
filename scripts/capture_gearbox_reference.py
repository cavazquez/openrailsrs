#!/usr/bin/env python3
"""Capture original OR 1.6.1 gearbox parsing, states, engine and route physics.

The source tree, installed DLLs and authored ENG/PAT are read-only. Runtime,
generated consist, logs and a private Wine prefix stay below --work-dir.
Nothing is copied over an approved baseline. --prepare-only registers inputs
and tolerances without starting Wine.

The two historical gearbox captures used the exact script archived alongside
their manifest as capture-script.py. This command now uses OracleWine's
disconnected desktop environment and guaranteed private-prefix cleanup. That
operational transition does not change the native helper, inputs or limits.
"""

import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import tomllib

from capture_chiltern_service_or import clone_prefix, windows_path
from prepare_chiltern_service import msts_blocks, msts_text
from run_oracles import verify
from oracle_wine import OracleWine

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools/openrails-reference/GearboxReference.cs"
SELF = Path(__file__).resolve()
DEFAULT_CONTENT = Path.home() / "Documentos/Open Rails/Content/Demo Model 1"
DEFAULT_VEHICLE = DEFAULT_CONTENT / "TRAINS/trainset/MT_DB_Class101_BlueGrey/MT_DB_Cl101_BlGr_DMBS_SC51451.eng"
DEFAULT_PATH = DEFAULT_CONTENT / "ROUTES/SCE/PATHS/MT_MT_0930 Edinburgh-Glasgow Queen Street(player).PAT"
REFERENCES = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
              "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]
TOLERANCES = {
    "declared_before_capture_and_rust_comparison": True,
    "gear_ratio_abs": 1e-6, "speed_mps_abs": 1e-5,
    "shaft_rpm_abs": .02, "rpm_abs": .02, "force_n_abs": .1,
    "clutch_pct_abs": .002, "flow_lps_abs": 1e-6,
    "state_and_gear": "exact",
    "native_drive": {"speed_mps_abs": .1, "distance_m_abs": 3,
                     "fuel_l_abs": .05, "manual_shift_time_s_abs": .1},
    "scope": "Subsystem tolerances compare equivalent native calculations; route tolerances are preregistered for a full native trace comparison, not an assertion that one has passed.",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def lexical_windows_path(path):
    """Keep our content facade in the argument instead of resolving its links."""
    absolute = str(path.absolute())
    return absolute if os.name == "nt" else "Z:" + absolute.replace("/", "\\")


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n")


def vehicle_profile(vehicle):
    text = msts_text(vehicle)
    engines = list(msts_blocks(text, "Engine"))
    if len(engines) != 1:
        raise ValueError("Expected one native Engine block")
    fields = []
    names = [
        "MaxPower", "MaxForce", "WheelRadius", "MaxTemperature", "MaxOilPressure",
        "MaxDieselLevel", "DieselUsedPerHourAtMaxPower", "DieselUsedPerHourAtIdle",
        "DieselSmokeEffectInitialMagnitude", "DieselSmokeEffectMaxMagnitude",
        "DieselSmokeEffectInitialSmokeRate", "DieselSmokeEffectMaxSmokeRate",
        "DieselEngineIdleRPM", "DieselEngineMaxRPM", "DieselEngineMaxRPMChangeRate",
        "ORTSDieselTransmissionType", "ORTSDieselEngineGovernorRPM", "ORTSDieselEngineMaxPower",
        "GearBoxNumberOfGears", "GearBoxDirectDriveGear", "ORTSReverseGearBoxIndication",
        "ORTSGearBoxFreeWheel", "ORTSGearBoxType", "ORTSMainClutchType",
        "GearBoxMaxSpeedForGears", "GearBoxMaxTractiveForceForGears", "ORTSGearBoxTractiveForceAtSpeed",
        "GearBoxOverspeedPercentageForFailure", "GearBoxBackLoadForce", "GearBoxCoastingForce",
        "GearBoxOperation", "GearBoxEngineBraking", "GearBoxUpGearProportion", "GearBoxDownGearProportion",
    ]
    for name in names:
        for raw in msts_blocks(engines[0], name):
            fields.append({"token": "engine(" + name.lower(), "raw": raw.strip()})
    return {"id": "class101_legacy_author", "family": "diesel_legacy_manual_gearbox",
            "source_vehicle": {"path": str(vehicle.resolve()), "sha256": digest(vehicle)},
            "derivation": "Selected numeric Engine blocks, preserving every repeated declaration in source order per token; values are parsed by MSTSDieselLocomotive.Parse, not by this extractor.",
            "fields": fields}


def replace_field(profile, token, raw):
    matches = [field for field in profile["fields"] if field["token"] == "engine(" + token.lower()]
    if not matches:
        raise ValueError(f"Missing authored {token}")
    matches[-1]["raw"] = raw


def prescribed_input(tick, *, speed=0, rpm=1900, throttle=100, clutch=100, next_gear=None,
                     direction="Forward", output_power=223710, traction=0):
    row = {"tick": tick, "speed_mps": speed, "throttle_pct": throttle,
           "direction": direction, "output_power_w": output_power, "traction_force_n": traction}
    if rpm is not None:
        row["rpm"] = rpm
    if clutch is not None:
        row["clutch_pct"] = clutch
    if next_gear is not None:
        row["next_gear_index"] = next_gear
    return row


def inputs(vehicle):
    authored = vehicle_profile(vehicle)
    explicit = copy.deepcopy(authored)
    explicit.update(id="class101_units_variant", family="diesel_legacy_unit_parser_variant",
                    derivation="Author-derived numeric variant with explicit mixed speed/force units, coasting=500 N and backload=1 kN; not a second authored vehicle.")
    replace_field(explicit, "GearBoxMaxSpeedForGears", "15mph 43.452288km/h 18.32864m/s 117.333333333ft/s")
    replace_field(explicit, "GearBoxMaxTractiveForceForGears", "35kN 25000N 17kN 12000N")
    replace_field(explicit, "GearBoxCoastingForce", "500N")
    replace_field(explicit, "GearBoxBackLoadForce", "1kN")
    rows = [prescribed_input(1, next_gear=-1, rpm=500, throttle=0, clutch=0)]
    for gear in range(4):
        for throttle in (0, 25, 60, 100):
            for rpm in (500, 920, 1480, 1900, 1901, 3230, 3231):
                for speed in (0, 2, 8, 20):
                    rows.append(prescribed_input(len(rows) + 1, next_gear=gear,
                                                 throttle=throttle, rpm=rpm, speed=speed))
    scenarios = [{"id": "manual_force_and_overspeed_grid", "profile_id": authored["id"],
                  "operation": "Manual", "method": "GearBox.Update", "step_s": .05, "inputs": rows}]
    rows = []
    for next_gear, clutch, speed, throttle, direction in [
        (0, 100, 0, 0, "Forward"), (0, 100, 2, 100, "Forward"),
        (1, 30, 7, 0, "Forward"), (1, 0, 7, 0, "Forward"),
        (2, 30, 12, 100, "Forward"), (2, 100, 12, 0, "Forward"),
        (3, 0, 18, 100, "Forward"), (1, 30, 12, 0, "Forward"),
        (1, 60, 12, 0, "Forward"), (-1, 30, 5, 0, "Forward"),
        (-1, 50, 5, 0, "Forward"), (0, 0, -2, 100, "Reverse"),
        (0, 100, -2, 100, "Forward"), (-1, 100, 0, 0, "Forward"),
    ]:
        rows.append(prescribed_input(len(rows) + 1, next_gear=next_gear, clutch=clutch,
                                     speed=speed, throttle=throttle, direction=direction))
    scenarios.append({"id": "manual_clutch_shift_and_neutral", "profile_id": authored["id"],
                      "operation": "Manual", "method": "GearBox.Update", "step_s": .05, "inputs": rows})
    rows = []
    for throttle, speed, clutch in [(0, 0, 0), (100, 0, 0), (100, 0, 0),
                                    (100, 6, 0), (100, 6, 0), (100, 6, 0),
                                    (100, 11, 0), (100, 11, 0), (100, 11, 0),
                                    (100, 17, 0), (100, 17, 0), (100, 17, 0),
                                    (100, 30, 0), (100, 30, 0),
                                    (100, 1, 0), (100, 1, 0), (100, 1, 0),
                                    (0, 1, 0), (0, 0, 0)]:
        rows.append(prescribed_input(len(rows) + 1, speed=speed, throttle=throttle, clutch=clutch))
    scenarios.append({"id": "automatic_up_down_and_neutral", "profile_id": authored["id"],
                      "operation": "Automatic", "method": "GearBox.Update", "step_s": .05, "inputs": rows})
    rows = [prescribed_input(i + 1, next_gear=0, speed=3, clutch=clutch)
            for i, clutch in enumerate((-120, -100, -60, -40, -20.001, -20, 0, 5, 50, 100, 120))]
    scenarios.append({"id": "explicit_units_coasting_and_clutch_clamp", "profile_id": explicit["id"],
                      "operation": "Manual", "method": "GearBox.Update", "step_s": .05, "inputs": rows})
    rows = []
    # Speed and load are prescribed here; the original DieselEngine updates RPM,
    # clutch, power and flow. The separate native drive prescribes only controls.
    for tick in range(800):
        gear = -1 if tick < 40 else 0 if tick < 280 else 1 if tick < 480 else 2 if tick < 640 else 3
        throttle = 0 if tick < 40 or 260 <= tick < 280 or 460 <= tick < 480 or 620 <= tick < 640 else 75
        speed = 0 if tick < 40 else (tick - 40) * .025
        rows.append(prescribed_input(tick + 1, speed=speed, rpm=None, throttle=throttle,
                                     clutch=None, next_gear=gear, traction=5000 if throttle else 0))
    scenarios.append({"id": "native_geared_engine_prescribed_speed", "profile_id": authored["id"],
                      "operation": "Manual", "method": "DieselEngine.Update", "step_s": .05, "inputs": rows})
    return {"schema_version": 1, "profiles": [authored, explicit], "scenarios": scenarios,
            "input_order": ["throttle", "direction", "speed_and_abs_speed", "rpm_if_present", "demanded_throttle",
                            "output_power_if_present", "clutch_if_present", "next_gear_if_present", "native_update"],
            "observer_order": ["ShaftRPM", "ClutchPercent", "IsClutchOn_if_in_gear", "TractiveForceN",
                               "IsOverspeedWarning", "IsOverspeedError", "GearIndication", "GearedUp", "GearedDown"]}


def drive_config():
    return {"schema_version": 1, "kind": "native_route_simulation", "step_s": .05,
            "ticks": 6000, "settle_ticks": 400,
            "control_units": {"throttle": "fraction", "brake": "native_handle_fraction", "gear": "0=neutral,1..4=gear"},
            "commands": [
                {"tick": 0, "throttle": 0, "brake": 1, "gear": 0},
                {"tick": 200, "throttle": 0, "brake": 0, "gear": 0},
                {"tick": 400, "throttle": 1, "brake": 0, "gear": 1},
                {"tick": 1200, "throttle": 0, "brake": 0, "gear": 1},
                {"tick": 1260, "throttle": 1, "brake": 0, "gear": 2},
                {"tick": 2200, "throttle": 0, "brake": 0, "gear": 2},
                {"tick": 2260, "throttle": 1, "brake": 0, "gear": 3},
                {"tick": 3200, "throttle": 0, "brake": 0, "gear": 3},
                {"tick": 3260, "throttle": 1, "brake": 0, "gear": 4},
                {"tick": 4300, "throttle": 0, "brake": 0, "gear": 4},
                {"tick": 4900, "throttle": 0, "brake": 1, "gear": 4},
                {"tick": 5600, "throttle": 0, "brake": 1, "gear": 0},
            ]}


def prepare_drive_facade(content, vehicle, path, destination):
    """Bulk authored content is linked read-only; only our small CON is new."""
    destination.mkdir()
    for entry in content.iterdir():
        if entry.name.upper() != "TRAINS":
            (destination / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    trains = destination / "TRAINS"
    trains.mkdir()
    for entry in (content / "TRAINS").iterdir():
        if entry.name.upper() != "CONSISTS":
            (trains / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    consists = trains / "CONSISTS"
    consists.mkdir()
    for entry in (content / "TRAINS/CONSISTS").iterdir():
        (consists / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
    consist = consists / "openrailsrs_gearbox_class101.con"
    with consist.open("x", encoding="utf-16") as stream:
        stream.write('SIMISA@@@@@@@@@@JINX0D0t______\n\nTrain (\n TrainCfg ( openrailsrs_gearbox_class101\n'
                     '  Name ( "OpenRailsRS native Class 101 gearbox" )\n Serial ( 1 )\n MaxVelocity ( 33.528 1 )\n'
                     '  NextWagonUID ( 1 ) Durability ( 1 )\n'
                     f'  Engine ( UiD ( 0 ) EngineData ( "{vehicle.stem}" "{vehicle.parent.name}" ) )\n )\n)\n')
    return destination / path.relative_to(content), consist


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vehicle", type=Path, default=DEFAULT_VEHICLE)
    parser.add_argument("--content-root", type=Path, default=DEFAULT_CONTENT)
    parser.add_argument("--drive-path", type=Path, default=DEFAULT_PATH)
    parser.add_argument("--skip-drive", action="store_true")
    parser.add_argument("--installation-root", type=Path,
                        default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wineserver", type=Path)
    parser.add_argument("--work-dir", type=Path, default=ROOT / "tmp/next-five-20261015/gearbox")
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    out = args.out_dir.resolve()
    work = args.work_dir.resolve()
    prefix = work / "wine-prefix"
    protected = [args.content_root.resolve(), args.installation_root.resolve(), args.wine_source_prefix.resolve(),
                 (ROOT.parent / "openrails").resolve()]
    for directory in (out, work):
        if any(directory.is_relative_to(original) for original in protected):
            raise ValueError("Capture runtime and outputs must stay outside original content/source/installation")
    if out.exists():
        raise ValueError("Choose a fresh capture directory; approved baselines cannot be overwritten")
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    out.mkdir(parents=True)
    write_json(out / "inputs.json", inputs(args.vehicle))
    write_json(out / "tolerances.json", TOLERANCES)
    write_json(out / "drive-config.json", drive_config())
    registration = {"reference_version": pin["version"], "reference_commit": pin["commit"],
                    "helper_sha256": digest(HELPER), "script_sha256": digest(SELF),
                    "wine_isolation_helper_sha256": digest(ROOT / "scripts/oracle_wine.py"),
                    "input_sha256": digest(out / "inputs.json"),
                    "tolerances_sha256": digest(out / "tolerances.json"),
                    "drive_config_sha256": digest(out / "drive-config.json"),
                    "source_vehicle": {"path": str(args.vehicle.resolve()), "sha256": digest(args.vehicle)},
                    "source_path": {"path": str(args.drive_path.resolve()), "sha256": digest(args.drive_path)},
                    "phase": "before_wine_capture_and_before_rust_comparison"}
    write_json(out / "registration.json", registration)
    if args.prepare_only:
        print(json.dumps(registration, indent=2))
        return
    clone_prefix(args.wine_source_prefix, prefix)
    runtime = out / "runtime"
    shutil.copytree(args.installation_root, runtime)
    files = [out / name for name in ("inputs.json", "tolerances.json", "registration.json", "reference.json", "drive-config.json")]
    with OracleWine(prefix, out / "wine-isolation", args.wineserver) as wine:
        wine.run(["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe",
                  "/nologo", "/out:GearboxReference.exe"] + ["/r:" + name for name in REFERENCES]
                 + [windows_path(HELPER)], cwd=runtime, log=out / "compile.log", timeout=90)
        wine.run(["wine", "GearboxReference.exe", "subsystems", windows_path(out / "inputs.json"),
                  windows_path(out / "reference.json")], cwd=runtime, log=out / "capture.log", timeout=90)
        if not args.skip_drive:
            path, consist = prepare_drive_facade(args.content_root.resolve(), args.vehicle.resolve(),
                                                args.drive_path.resolve(), out / "content")
            wine.run(["wine", "GearboxReference.exe", "drive", lexical_windows_path(path), windows_path(consist),
                      windows_path(out / "native-drive"), windows_path(out / "drive-config.json")],
                     cwd=runtime, log=out / "native-drive.log", timeout=240)
            files += [consist, *sorted((out / "native-drive").iterdir())]
    files += [out / "wine-isolation" / name for name in ("isolation.json", "release.json")]
    # Original binaries/source and all approved references must still match.
    verify(pin, ROOT.parent / "openrails", runtime)
    manifest = {**registration, "schema_version": 1,
                "capture_kind": "original_dll_calls_and_optional_native_route_simulation",
                "methods": json.loads((out / "reference.json").read_text())["methods"],
                "families": ["diesel_legacy_manual_gearbox", "diesel_legacy_automatic_subsystem", "vacuum_twin_pipe_native_drive"],
                "phases": ["parse_units", "initialize_ratios", "neutral", "clutch_shift_boundaries", "forward_reverse",
                           "manual_up_down", "automatic_up_down", "overspeed", "power_limit", "geared_rpm_and_fuel_flow"],
                "native_drive_captured": not args.skip_drive,
                "wine_isolation_release_sha256": digest(out / "wine-isolation/release.json"),
                "sha256": {name: digest(runtime / name) for name in REFERENCES},
                "files": {str(path.relative_to(out)): digest(path) for path in files},
                "scope": "Real OR 1.6.1 DLL results. Synthetic unit/coasting variants are labeled author-derived. Subsystem speed/RPM/power are prescribed; native-drive uses Simulator.Update with control commands only. No complete steam/electric or multi-engine certification claim.",
                "baseline_policy": "New capture only; previously approved baselines were verified and not regenerated."}
    write_json(out / "manifest.json", manifest)
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

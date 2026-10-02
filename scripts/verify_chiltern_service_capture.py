#!/usr/bin/env python3
"""Verify that the frozen OR service is a complete, reproducible native run.

This checks reference integrity and station contracts, not Bevy physics parity.
The latter has a separate strict oracle in oracles/chiltern-service.toml.
"""

import argparse
import csv
import hashlib
import json
import math
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / "examples/baselines/chiltern_local"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_capture(directory):
    manifest = json.loads((directory / "manifest.json").read_text())
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    require(manifest["version"] == pin["version"] and manifest["source_commit"] == pin["commit"],
        "Service capture does not use the pinned original")
    require(manifest["mode"] == "service", "A diagnostic replay cannot certify the reference service")
    for entry in pin["binaries"]:
        require(manifest["binaries"][entry["name"]] == entry["sha256"], "Different original binary")
    require(manifest["files"], "Missing capture file hashes")
    for filename, expected in manifest["files"].items():
        path = directory / filename
        require(path.resolve().is_relative_to(directory.resolve()), "Capture file escapes its directory")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, f"Capture changed: {filename}")
    require(manifest["files"]["capture/trace.csv"] == manifest["trace_sha256"], "Trace hash mismatch")
    require(manifest["files"]["driver.csv"] == manifest["driver_sha256"], "Driver hash mismatch")
    require(manifest["files"]["HeadlessCapture.cs"] == manifest["helper_sha256"], "Helper hash mismatch")
    initial = json.loads((directory / "capture/initial.json").read_text())
    outcome = json.loads((directory / "capture/outcome.json").read_text())
    config = json.loads((directory / "config.json").read_text())
    require(initial["cars"] == 8 and len(initial["vehicles"]) == 8, "Different original consist")
    require(0 <= initial["error_m"] <= 0.1, "Original head does not start at Northolt")
    require(outcome["success"] and outcome["failure"] is None, "Original service failed")
    require(len(outcome["native_tasks"]) == 3 and all(task["completed"] is True for task in outcome["native_tasks"]),
        "Original station tasks did not complete")
    require(len(outcome["stops"]) == len(config["stops"]) == 3, "Incomplete station service")
    step = manifest["step_s"]
    require(step == config["step_s"] == outcome["step_s"] == 0.05, "Wrong capture timestep")
    with (directory / "capture/trace.csv").open() as stream:
        rows = list(csv.DictReader(stream))
    require(len(rows) > 14000, "Short excerpt instead of a full service")
    numeric = ("time_s", "velocity_mps", "odometer_m", "throttle", "brake", "allowed_speed_mps",
        "native_brake_handle", "cylinder_psi", "brake_pipe_psi", "mass_kg", "pos_on_edge_m")
    for index, row in enumerate(rows):
        require(all(math.isfinite(float(row[name])) for name in numeric), "Non-finite native telemetry")
        require(abs(float(row["time_s"]) - index * step) < 1e-8, "Native trace has a missing physics tick")
        require(0 <= float(row["throttle"]) <= 1 and 0 <= float(row["brake"]) <= 1, "Invalid native controls")
        # Original couplers can creep a few millimetres in either direction at a stop.
        if row["door_state"] != "Closed":
            require(abs(float(row["velocity_mps"])) <= 0.1 and float(row["throttle"]) == 0,
                "Native train departed with doors open")
    require(float(rows[0]["time_s"]) == float(rows[0]["odometer_m"]) == 0, "Missing initial native sample")
    require(7000 <= outcome["distance_m"] <= 7100, "Original did not travel the selected route")
    require(abs(float(rows[-1]["odometer_m"]) - outcome["distance_m"]) < 0.01, "Native outcome differs from trace")
    require(rows[-1]["door_state"] == "Closed", "Terminal doors have not closed")
    for stop, schedule, task in zip(outcome["stops"], config["stops"], outcome["native_tasks"], strict=True):
        require(stop["station"] == schedule["name"] == task["station"], "Wrong station order")
        require(0 <= stop["position_error_m"] <= 10 and abs(stop["arrival_speed_mps"]) <= 0.1,
            "Native train missed a stopping target")
        require(stop["boarding_s"] + 1e-8 >= schedule["dwell_s"] and stop["depart_s"] >= schedule["depart_s"],
            "Native train departed before boarding or scheduled departure")
        first = math.ceil(stop["arrival_s"] / step - 1e-8)
        last = math.floor(stop["depart_s"] / step + 1e-8)
        open_time = sum(step for row in rows[first:last + 1] if row["door_state"] == "Open")
        require(open_time + step >= schedule["dwell_s"], "Native doors did not stay open for boarding")
    return {"reference": manifest["version"], "samples": len(rows), "distance_m": outcome["distance_m"],
        "stations": len(outcome["stops"])}


def verify_replay(reference, replay):
    """Check that exported controls really reproduce the original physics."""
    original = json.loads((reference / "manifest.json").read_text())
    candidate = json.loads((replay / "manifest.json").read_text())
    require(candidate["mode"] == "replay", "Expected a native control replay")
    for key in ("version", "source_commit", "step_s", "seed", "helper_sha256", "binaries",
            "runtime_assemblies", "native_files", "inputs", "generated", "config_sha256"):
        require(candidate[key] == original[key], f"Different native replay configuration: {key}")
    require(candidate["replay_sha256"] == original["driver_sha256"], "Different replay commands")
    for filename, expected in candidate["files"].items():
        path = replay / filename
        require(path.resolve().is_relative_to(replay.resolve()), "Replay file escapes its directory")
        require(hashlib.sha256(path.read_bytes()).hexdigest() == expected, f"Replay changed: {filename}")
    require(json.loads((replay / "capture/outcome.json").read_text())["failure"] is None, "Native replay failed")
    with (reference / "capture/trace.csv").open() as stream:
        expected = list(csv.DictReader(stream))
    with (replay / "capture/trace.csv").open() as stream:
        actual = list(csv.DictReader(stream))
    require(len(expected) == len(actual), "Replay does not cover the complete service")
    fields = ("time_s", "velocity_mps", "odometer_m", "pos_on_edge_m", "native_brake_handle",
        "cylinder_psi", "brake_pipe_psi", "allowed_speed_mps", "mass_kg")
    for row_a, row_b in zip(expected, actual, strict=True):
        require(row_a["edge_id"] == row_b["edge_id"], "Replay changed the routed path")
        require(all(float(row_a[name]) == float(row_b[name]) for name in fields),
            f"Native control replay differs at {row_a['time_s']} s")
    return {"exact_native_physics_replay": True, "samples": len(actual), "fields": fields,
        "reference_trace_sha256": original["trace_sha256"], "replay_trace_sha256": candidate["trace_sha256"],
        "driver_sha256": original["driver_sha256"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path, default=BASELINE)
    parser.add_argument("--replay", type=Path, help="Verify a second original-DLL run with the exported controls")
    args = parser.parse_args()
    report = verify_capture(args.directory)
    if args.replay:
        report["replay"] = verify_replay(args.directory, args.replay)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError) as error:
        print(f"Native service reference rejected: {error}", file=sys.stderr)
        sys.exit(1)

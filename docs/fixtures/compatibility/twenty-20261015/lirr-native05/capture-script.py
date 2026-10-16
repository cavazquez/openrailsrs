#!/usr/bin/env python3
"""Capture original Brooklyn--Hempstead and compare an exact Rust control replay.

Never builds Rust implicitly. All files are captured under a fresh output
directory and Wine uses a separate, private prefix with desktop drivers disabled.
"""
import argparse
import csv
import hashlib
import json
import math
from pathlib import Path
import shutil
import subprocess
import time
import tomllib

from capture_chiltern_service_or import clone_prefix, windows_path
from oracle_wine import OracleWine
from run_oracles import verify

ROOT = Path(__file__).resolve().parents[1]
PROTOCOL = ROOT / "oracles/lirr-full-service-protocol.json"
HELPER = ROOT / "tools/openrails-reference/ActivityTrajectory.cs"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read_trace(path):
    with Path(path).open() as stream:
        rows = [{k: float(v) for k, v in row.items()} for row in csv.DictReader(stream)]
    if not rows or any(not math.isfinite(v) for row in rows for v in row.values()):
        raise ValueError("Missing or nonfinite full trace")
    for i, row in enumerate(rows):
        if abs(row["time_s"] - i * 0.05) > 1e-5:
            raise ValueError("The full trace must start at zero with every20Hz sample")
        if not all(0 <= row[k] <= 1 for k in ("throttle", "brake")):
            raise ValueError("Invalid native handle")
    return rows


def compare(native, rust, protocol, outcome, initial):
    if len(native) != len(rust):
        raise ValueError("A truncated replay cannot pass")
    pairs = {"velocity_rms_mps": "velocity_mps", "brake_pipe_rms_psi": "brake_pipe_psi", "cylinder_rms_psi": "cylinder_psi", "motive_force_rms_n": "motive_force_n", "brake_force_rms_n": "brake_force_n"}
    errors = {name: math.sqrt(sum((a[column] - b[column])**2 for a, b in zip(native, rust))/len(native)) for name, column in pairs.items()}
    errors["maximum_odometer_error_m"] = max(abs(a["odometer_m"]-b["odometer_m"]) for a, b in zip(native, rust))
    controls_match = all(abs(a[k]-b[k]) <= 1e-8 for a, b in zip(native, rust) for k in ("time_s", "throttle", "brake", "doors_open"))
    coverage = {"native_reached_all_platforms": outcome["reached_all_platforms"],
                "original_platforms": len(outcome["arrivals"]) >= protocol["minimum_original_platforms"],
                "original_formation": initial["cars"] == protocol["cars"],
                "full_distance": native[-1]["odometer_m"] >= protocol["minimum_distance_m"],
                "full_duration": native[-1]["time_s"] >= protocol["minimum_duration_s"],
                "exact_controls": controls_match}
    checks = {name: math.isfinite(value) and value <= protocol["thresholds"][name] for name, value in errors.items()}
    return {"schema": 1, "reference_version": "1.6.1", "samples": len(native), "errors": errors,
            "thresholds_predeclared": protocol["thresholds"], "checks": checks, "coverage": coverage,
            "passed": all(checks.values()) and all(coverage.values()), "scope": protocol["controls"]}


def native_coverage(rows, protocol, outcome, initial):
    """Coverage is required even when the Rust replay has been deferred."""
    coverage = {
        "native_reached_all_platforms": outcome["reached_all_platforms"],
        "original_platforms": len(outcome["arrivals"]) >= protocol["minimum_original_platforms"],
        "all_original_tasks_visited": len(outcome["arrivals"]) == outcome["original_task_count"],
        "original_formation": initial["cars"] == protocol["cars"],
        "full_distance": rows[-1]["odometer_m"] >= protocol["minimum_distance_m"],
        "full_duration": rows[-1]["time_s"] >= protocol["minimum_duration_s"],
        "declared_duration_limit": rows[-1]["time_s"] <= protocol["maximum_duration_s"],
    }
    return {"schema": 1, "reference_version": protocol["reference_version"],
            "samples": len(rows), "step_s": protocol["step_s"],
            "duration_s": rows[-1]["time_s"], "distance_m": rows[-1]["odometer_m"],
            "coverage": coverage, "passed": all(coverage.values()),
            "comparison_pending": True,
            "scope": "Original DLL capture only; this does not certify Rust parity."}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--activity", type=Path, required=True)
    p.add_argument("--consist", type=Path, required=True)
    p.add_argument("--out-dir", type=Path, required=True)
    p.add_argument("--installation-root", type=Path, default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    p.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    p.add_argument("--wine-prefix", type=Path, help="Reuse one marked, private clone between fresh attempts")
    p.add_argument("--native-only", action="store_true", help="Capture and validate original coverage; defer Rust import and replay")
    p.add_argument("--binary", type=Path, default=ROOT / "target/debug/openrailsrs")
    p.add_argument("--replay-binary", type=Path, default=ROOT / "target/debug/examples/activity_trajectory")
    p.add_argument("--protocol", type=Path, default=PROTOCOL)
    args = p.parse_args()
    protocol = json.loads(args.protocol.read_text())
    if args.activity.name != protocol["activity"] or digest(args.activity) != protocol["activity_sha256"]:
        raise ValueError("The protocol selects the original identified LIRR activity")
    if not args.consist.is_file() or args.consist.stem.casefold() != protocol["consist"].casefold():
        raise ValueError("Select the original SFexpress-500 formation")
    out = args.out_dir.resolve()
    route = args.activity.resolve().parent.parent
    for original in (route.parent.parent, args.installation_root.resolve(), args.wine_source_prefix.resolve(), ROOT.parent / "openrails"):
        if out.is_relative_to(original) or original.is_relative_to(out):
            raise ValueError("Outputs must be separate from original files")
    if out.exists():
        raise ValueError("Use a fresh directory, retaining failed attempts")
    if not args.native_only and (not args.binary.is_file() or not args.replay_binary.is_file()):
        raise ValueError("Build workspace and activity_trajectory first; no implicit builds")
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    out.mkdir(parents=True)
    input_files = [args.activity.resolve(), args.consist.resolve(), args.protocol.resolve(), HELPER,
                   ROOT / "tools/openrails-reference/HeadlessCapture.cs", Path(__file__).resolve(),
                   ROOT / "scripts/oracle_wine.py", ROOT / "oracles/openrails-reference.toml"]
    if not args.native_only:
        input_files += [args.binary.resolve(), args.replay_binary.resolve()]
    input_files += [p for p in route.iterdir() if p.is_file() and p.suffix.lower() in (".tdb",".trk")]
    # Physics definitions are small; capture the original stock and native
    # service/path inputs before DLL loading rather than guessing their hashes
    # afterwards from filenames in the loaded consist.
    content = route.parent.parent
    input_files += [p for p in (content/"TRAINS").rglob("*") if p.is_file() and p.suffix.lower() in (".con", ".eng", ".wag", ".inc")]
    input_files += [p for folder in (route/"PATHS", route/"SERVICES") for p in folder.rglob("*") if p.is_file() and p.suffix.lower() in (".pat", ".srv")]
    input_files += [p for p in (route/"tsection.dat", route/"sigcfg.dat", route/"sigscr.dat", content/"GLOBAL/tsection.dat") if p.is_file()]
    input_files += [args.installation_root/e["name"] for e in [*pin["binaries"],*pin.get("scenery_binaries",[])]]
    sources = {str(p): digest(p) for p in input_files}
    receipt = {"schema": 1, "status": "declared", "mode": "native_only" if args.native_only else "comparison",
               "reference_commit": pin["commit"], "protocol": protocol,
               "input_sha256_before": sources, "commands": []}
    record = out / "receipt.json"
    record.write_text(json.dumps(receipt, indent=2)+"\n")
    runtime = out / "runtime"
    runtime.mkdir()
    refs = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Common.dll", "Orts.Settings.dll", "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]
    for file in args.installation_root.iterdir():
        if file.is_file() and (file.suffix.lower() == ".dll" or file.name in ("RunActivity.exe", "RunActivity.exe.config")):
            shutil.copy2(file, runtime/file.name)
    receipt["assemblies"] = {name: digest(runtime/name) for name in refs}
    prefix = args.wine_prefix.resolve() if args.wine_prefix else out / "private-prefix"
    started = time.monotonic()
    try:
        clone_prefix(args.wine_source_prefix, prefix)
        with OracleWine(prefix, out / "wine-isolation") as wine:
            compiler = ["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe", "/nologo", "/main:ActivityTrajectory", "/out:ActivityTrajectory.exe"] + ["/r:"+name for name in refs] + [windows_path(ROOT / "tools/openrails-reference/HeadlessCapture.cs"), windows_path(HELPER)]
            wine.run(compiler, cwd=runtime, log=out/"compile.log", timeout=90)
            wine.run(["wine", "ActivityTrajectory.exe", windows_path(args.activity), windows_path(out/"native")], cwd=runtime, log=out/"capture.log", timeout=900)
        native = read_trace(out/"native/trace.csv")
        outcome = json.loads((out/"native/outcome.json").read_text())
        initial = json.loads((out/"native/initial.json").read_text())
        coverage = native_coverage(native, protocol, outcome, initial)
        (out/"native-coverage.json").write_text(json.dumps(coverage, indent=2)+"\n")
        if not coverage["passed"]:
            receipt["status"] = "failed_native_coverage"
            raise RuntimeError("Native full-service coverage failed; preserve the attempt")
        if args.native_only:
            receipt["status"] = "native_capture_complete"
            receipt["comparison_pending"] = True
            return
        def run(command, label):
            receipt["commands"].append(list(map(str, command)))
            with (out/(label+".log")).open("w") as log:
                subprocess.run(list(map(str, command)), cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
        run([args.binary.resolve(), "import-msts", route, "--activity", args.activity.resolve(), "--consist", args.consist.resolve(), "--out-dir", out/"imported"], "import")
        run([args.replay_binary.resolve(), out/"imported/scenario.toml", out/"native/trace.csv", out/"rust.csv"], "replay")
        result = compare(native, read_trace(out/"rust.csv"), protocol, outcome, initial)
        (out/"comparison.json").write_text(json.dumps(result, indent=2)+"\n")
        receipt["status"] = "pass" if result["passed"] else "failed_tolerance_or_coverage"
        if not result["passed"]:
            raise RuntimeError("Full LIRR comparison failed; keep recorded thresholds and attempt")
    except BaseException as error:
        receipt.setdefault("error", str(error))
        if receipt["status"] == "declared": receipt["status"] = "failed_execution"
        raise
    finally:
        receipt["wall_time_s"] = time.monotonic() - started
        receipt["input_sha256_after"] = {p: digest(p) for p in sources}
        receipt["inputs_unchanged"] = receipt["input_sha256_after"] == sources
        if not receipt["inputs_unchanged"]: receipt["status"]="failed_inputs_changed"
        receipt["outputs_sha256"] = {str(p.relative_to(out)): digest(p) for p in (runtime/"ActivityTrajectory.exe", out/"native/trace.csv", out/"native/initial.json", out/"native/outcome.json", out/"native-coverage.json", out/"rust.csv", out/"comparison.json", out/"wine-isolation/isolation.json", out/"wine-isolation/release.json") if p.is_file()}
        record.write_text(json.dumps(receipt, indent=2)+"\n")
        if not receipt["inputs_unchanged"]:
            raise RuntimeError("Original or source input changed during capture")
    if not receipt["inputs_unchanged"]:
        raise RuntimeError("Original or source input changed during capture")


if __name__ == "__main__":
    main()

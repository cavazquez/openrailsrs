#!/usr/bin/env python3
"""Extract compact CPU regression outputs from genuine OR1.6.1 steam calls.

No native equation is evaluated here. Every expected number is selected from
an immutable captured JSON scalar; booleans become0/1 only. NativeSteamParams
comes from the authored-STF parse stored by the measured Rust example, never
from observed pressure, heat, mass, force, flow, or filter state.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil


COMMIT = "d16e670da333d26d2edfc97d5631a19dadf49ce5"
PROTOCOL = "c62fc972d0799c3fa7d32db8e6bcd19fde852b986b8ebe652bbb96de0e7e73cb"
MANIFESTS = {
    "hot_subsystem": "abd64cb3088dc54a0266df0c275d3953dfe8a4589fca92166d09dac8a4683e40",
    "cold_subsystem": "69c3e00601db4b00e79b77f07cd18de4c30e547481f8635588e5b02fdd999915",
}
COLUMNS = [
    "tick", "pressure_psi", "boiler_mass_lb", "boiler_heat_btu", "water_fraction",
    "evaporation_lb_s", "steam_usage_lb_s", "water_consumption_lb_s", "fire_burn_kg_s",
    "tender_coal_kg", "combined_tender_water_ukg", "water_lb_per_ukg", "motive_force_n",
    "injector1", "injector2", "injector1_fraction", "injector2_fraction",
]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load(path):
    return json.loads(path.read_bytes())


def write(path, value, compact=False):
    path.write_text(json.dumps(value, ensure_ascii=False, allow_nan=False,
                               separators=(",", ":") if compact else None,
                               indent=None if compact else 2) + "\n")


def verified_case(capture, measurement):
    manifest = load(capture / "manifest.json")
    case = manifest["case"]
    if (sha(capture / "manifest.json") != MANIFESTS[case]
            or manifest["reference_commit"] != COMMIT
            or manifest["reference_version"] != "1.6.1"
            or manifest["method"] != "MSTSSteamLocomotive.Update"
            or manifest["status"] != "captured_not_compared"
            or manifest["protocol_sha256"] != PROTOCOL):
        raise ValueError("Expected the fixed original subsystem capture")
    for name, expected in manifest["files"].items():
        p = Path(name)
        if p.is_absolute() or ".." in p.parts or sha(capture / p) != expected:
            raise ValueError(f"Captured file changed: {name}")
    receipt = load(measurement / "receipt.json")
    if receipt["capture_manifest_sha256"] != sha(capture / "manifest.json"):
        raise ValueError("Profile parse receipt refers to another capture")
    for name, expected in receipt["outputs"].items():
        p = Path(name)
        if p.is_absolute() or ".." in p.parts or sha(measurement / p) != expected:
            raise ValueError(f"Measured file changed: {name}")
    original = load(capture / "native/reference.json")
    programme = load(capture / "input.json")
    initial = load(capture / "native/initial.json")
    parsed = load(measurement / "initial.json")
    if (original["input"] != programme
            or programme["kind"] != "prescribed_speed_subsystem"
            or programme["ticks"] != 1600
            or len(original["rows"]) != 1601
            or parsed["profile"]["hot_start"] != programme["hot_start"]
            or parsed["original"] != initial):
        raise ValueError("Capture, parsed profile, and programme disagree")
    expected = []
    for tick, row in enumerate(original["rows"]):
        if row["tick"] != tick:
            raise ValueError("Incomplete native row sequence")
        expected.append([int(row[name]) if isinstance(row[name], bool) else row[name]
                         for name in COLUMNS])
    environment = {
        "world_position": initial["world_position"],
        "height_above_sea_level_m": initial["locomotive"]["TrainCar.CarHeightAboveSeaLevelM"],
        "season": "summer", "weather": "clear",
        "scope": "Static post-Start position, height and original SetExplore settings only; no observed outside temperature is included",
    }
    data = {
        "id": case, "hot_start": programme["hot_start"],
        "profile": parsed["profile"], "ticks": programme["ticks"], "dt_s": programme["dt_s"],
        "programme": programme["commands"], "environment": environment,
        "expected": expected,
    }
    proof = {
        "capture_manifest_sha256": sha(capture / "manifest.json"),
        "capture_status_preserved": manifest["status"],
        "raw_reference_sha256": manifest["files"]["native/reference.json"],
        "raw_initial_sha256": manifest["files"]["native/initial.json"],
        "raw_tables_sha256": manifest["files"]["native/tables.json"],
        "raw_input_sha256": manifest["files"]["input.json"],
        "raw_helper_sha256": manifest["helper_sha256"],
        "original_assets_sha256": {Path(p).name: h for p, h in manifest["originals"].items()},
        "original_assemblies_sha256": manifest["assemblies"],
        "original_source_sha256": manifest["source_files"],
        "profile_derivation": {
            "method": "Direct copy of measured initial.json#/profile, authored STF parsed by parse_native_steam161(read_vehicle_ast(originalENG)); hot_start comes from the predeclared case",
            "measurement_receipt_sha256": sha(measurement / "receipt.json"),
            "measurement_initial_sha256": sha(measurement / "initial.json"),
            "measured_parser_source_sha256": receipt["source_sha256"]["crates/openrailsrs-formats/src/typed/native_steam.rs"],
            "measured_example_source_sha256": receipt["source_sha256"]["crates/openrailsrs-sim/examples/steam_cycle_compare.rs"],
            "post_start_native_model_state_used_as_hardware": False,
        },
    }
    return data, proof, manifest


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--hot-capture", type=Path, required=True)
    ap.add_argument("--cold-capture", type=Path, required=True)
    ap.add_argument("--hot-measurement", type=Path, required=True)
    ap.add_argument("--cold-measurement", type=Path, required=True)
    ap.add_argument("--out-dir", type=Path, required=True)
    a = ap.parse_args()
    if a.out_dir.exists():
        raise ValueError("Extraction destination must be fresh")
    data = []
    proof = {}
    raw = []
    for capture, measurement in [(a.hot_capture, a.hot_measurement), (a.cold_capture, a.cold_measurement)]:
        case, provenance, manifest = verified_case(capture, measurement)
        data.append(case)
        proof[case["id"]] = provenance
        raw.append((capture, manifest))
    if [c["id"] for c in data] != ["hot_subsystem", "cold_subsystem"]:
        raise ValueError("Need one independently initialized hot and cold case")
    if sha(a.hot_capture / "native/tables.json") != sha(a.cold_capture / "native/tables.json"):
        raise ValueError("These original static table captures must be identical before deduplication")
    tables = load(a.hot_capture / "native/tables.json")
    if len(tables["points"]) != 250 or len(tables["grids"]) != 60:
        raise ValueError("Incomplete original DLL table coverage")
    protocol = load(a.hot_capture / "protocol.json")
    fixture = {
        "schema": 1, "reference_version": "1.6.1", "reference_commit": COMMIT,
        "scope": "Original-DLL prescribed-speed subsystem only; neither route-drive nor full steam compatibility",
        "protocol_sha256": PROTOCOL, "columns": COLUMNS,
        "limits": protocol["native_limits"], "table_limits": protocol["new_table_limits"],
        "tables": tables, "cases": data,
    }
    a.out_dir.mkdir(parents=True)
    write(a.out_dir / "reference.json", fixture, compact=True)
    shutil.copyfile(a.hot_capture / "protocol.json", a.out_dir / "protocol.json")
    shutil.copyfile(__file__, a.out_dir / "extract-fixture.py")
    (a.out_dir / "raw-manifests").mkdir()
    for capture, manifest in raw:
        shutil.copyfile(capture / "manifest.json", a.out_dir / "raw-manifests" / (manifest["case"] + ".json"))
    files = {str(p.relative_to(a.out_dir)): sha(p) for p in sorted(a.out_dir.rglob("*")) if p.is_file()}
    write(a.out_dir / "provenance.json", {
        "schema": 1, "kind": "lossy_projection_of_genuine_native_capture_for_cpu_regression",
        "reference_version": "1.6.1", "reference_commit": COMMIT,
        "extraction_algorithm": [
            "Verify the immutable raw capture manifest and every registered file hash.",
            "For every raw row0..1600 select the17 named scalar columns, preserving JSON decimal numbers; encode only injector booleans as0/1. No rounding, interpolation, downsampling, fitted constants or native equations.",
            "Copy predeclared programme inputs, independently parsed authored hardware profile and static world position/height; exclude all native evolving state from inputs.",
            "Copy all250 one-dimensional and60 two-dimensional original DLL table calls. Deduplicate only after hot/cold raw table hashes match exactly.",
            "Compact JSON whitespace only; this file has a new derived SHA and is not claimed byte-identical to the raw captures.",
        ],
        "original_resources_bundled": False, "post_start_observed_model_state_prescribed": False,
        "native_limits_changed": False, "rust_execution": "not_run_for_this_ci_target",
        "cases": proof, "files_sha256": files,
    })
    print(json.dumps({"status": "extracted_not_compared", "rows": [len(c["expected"]) for c in data],
                      "table_calls": len(tables["points"]) + len(tables["grids"]),
                      "reference_bytes": (a.out_dir / "reference.json").stat().st_size,
                      "reference_sha256": sha(a.out_dir / "reference.json")}))


if __name__ == "__main__":
    main()

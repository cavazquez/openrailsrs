#!/usr/bin/env python3
"""Derive a small original-DLL tunnel oracle; optionally publish capture08.

Expected forces are selected native CSV scalars. No tunnel force equation is
evaluated here. RowN uses the registered previous-row geometry/speed context.
"""
import argparse
import csv
import gzip
import hashlib
import json
from pathlib import Path
import re
import shutil
import struct


RAW08 = "362ecf741d06ae02e5bfb6ea67af88559d0f98adf631342d42e070c91f4eb13a"
RAW07 = "21212399f6289ff44b6d108d6312670008e6503abee0054df2d4dd150d6e2be2"
PROTOCOL = "49eed6db5e701f9e513a5f4b15091ba5f8c17a42435136da835a850df8877705"


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read(path):
    return json.loads(path.read_bytes())


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False, allow_nan=False) + "\n")


def single(value):
    return struct.unpack("f", struct.pack("f", value))[0]


def rows(path):
    with path.open(newline="") as stream:
        yield from csv.DictReader(stream)


def publish_capture(capture, output, manifest):
    if output.exists():
        raise ValueError("Published capture destination must be fresh")
    output.mkdir(parents=True)
    mapping = {}
    for name, expected in manifest["files_sha256"].items():
        source = capture / name
        if sha(source) != expected:
            raise ValueError(f"Immutable captured bytes changed: {name}")
        target_name = name + ".gz" if name == "native/trace.csv" else name
        target = output / target_name
        target.parent.mkdir(parents=True, exist_ok=True)
        if name == "native/trace.csv":
            with source.open("rb") as incoming, target.open("wb") as outgoing:
                with gzip.GzipFile(filename="", fileobj=outgoing, mode="wb", compresslevel=9, mtime=0) as zipped:
                    shutil.copyfileobj(incoming, zipped, 1024 * 1024)
            with gzip.open(target, "rb") as decoded:
                if hashlib.file_digest(decoded, "sha256").hexdigest() != expected:
                    raise ValueError("Lossless original CSV round-trip failed")
        else:
            shutil.copyfile(source, target)
        mapping[target_name] = {"sha256": sha(target), "raw_path": name,
                                "raw_sha256": expected,
                                "transformation": "lossless gzip mtime0" if name == "native/trace.csv" else "none; literal captured bytes"}
    shutil.copyfile(capture / "manifest.json", output / "manifest.json")
    mapping["manifest.json"] = {"sha256": sha(output / "manifest.json"), "transformation": "none; literal native manifest"}
    write(output / "publication.json", {
        "schema": 1, "kind": "lossless_publication_of_additive_original_tunnel_capture",
        "native_manifest_sha256": sha(capture / "manifest.json"), "rows": 32583,
        "previous_columns": 164, "columns": 306,
        "previous_columns_project_to07_byte_sha256": RAW07,
        "raw_trace_sha256": RAW08, "files": mapping,
        "rust_acceptance": "not_measured_by_this_capture", "original_assets_bundled": False,
        "excluded": ["runtime", "original ENG/WAG/CON/ACT/PAT/route/DLL assets", "Wine prefix", "human trace contents"],
        "scope": "Read-only original DLL fields. Prior164 columns are byte-identical in all32583 rows. Gzip hash differs from raw CSV; decompression reproduces RAW08. Raw JSON/source/receipts retain their captured host paths and identities.",
    })


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--capture-dir", type=Path, required=True)
    parser.add_argument("--protocol", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--publish-capture-dir", type=Path)
    args = parser.parse_args()
    capture = args.capture_dir.resolve()
    if args.out_dir.exists():
        raise ValueError("Fixture destination must be fresh")
    manifest = read(capture / "manifest.json")
    integrity = read(capture / "telemetry-integrity.json")
    receipt = read(capture / "receipt.json")
    protocol = read(args.protocol)
    raw = capture / "native/trace.csv"
    if sha(raw) != RAW08 or sha(args.protocol) != PROTOCOL:
        raise ValueError("Use fixed original08 and the predeclared protocol")
    if (integrity["projected_new_trace_sha256"] != RAW07
            or integrity["rows_compared"] != 32583
            or any(integrity["per_column_mismatch_counts"].values())
            or protocol["force_maximum_absolute_error_n"] != 0.01):
        raise ValueError("Original identity/tolerance contract changed")
    # The original TRK has no tunnel overrides. This input selection follows
    # TrainCar.Initialize's >=97.22m/s branch, never observed output area.
    trk = next(Path(p) for p in receipt["input_sha256_before"] if p.lower().endswith("/lirr.trk"))
    if sha(trk) != receipt["input_sha256_before"][str(trk)]:
        raise ValueError("Original TRK changed")
    trk_bytes = trk.read_bytes()
    trk_text = trk_bytes.decode("utf-16" if trk_bytes[:2] in (b"\xff\xfe", b"\xfe\xff") else "utf-8-sig", errors="strict")
    if re.search(r"orts(?:single|double)tunnel(?:area|perimeter)\s*\(", trk_text, re.I):
        raise ValueError("This source-default fixture requires absent TRK tunnel overrides")
    route_speed = float(re.search(r"\bSpeedLimit\s*\(\s*([0-9.]+)", trk_text, re.I).group(1))
    if route_speed != 223.52:
        raise ValueError("Original authored route speed changed")
    selected = {}
    def select(index, car, reason):
        if 1 <= index < 32583:
            selected.setdefault((index, car), set()).add(reason)
    previous = None
    maxima = [(-1., 1) for _ in range(7)]
    dimensions = {}
    for index, row in enumerate(rows(raw)):
        for car in range(7):
            prefix = f"car_{car}_"
            dims = (float(row[prefix + "width_m"]), float(row[prefix + "height_m"]))
            if car not in dimensions:
                dimensions[car] = dims
            if dimensions[car] != dims:
                raise ValueError("This bounded fixture requires constant authored body dimensions")
            area = single(single(dims[0]) * single(dims[1]))
            observed = float(row[prefix + "train_cross_section_area_m2"])
            if observed not in (0., area):
                raise ValueError("Native area history differs from static width*height; do not feed it back")
            state = (row[prefix + "tunnel_front_valid"], row[prefix + "tunnel_paths"])
            if previous is not None:
                old = (previous[prefix + "tunnel_front_valid"], previous[prefix + "tunnel_paths"])
                if state != old:
                    for offset in range(3):
                        select(index + offset, car, f"context_transition_at_{index}")
            force = float(row[prefix + "tunnel_force_n"])
            if force > maxima[car][0]:
                maxima[car] = (force, index)
            if float(row["time_s"]) in (400., 500., 600.):
                select(index, car, "fixed_time_" + row["time_s"])
            select(1, car, "first_computed_tick")
        previous = row
    for car, (_, index) in enumerate(maxima):
        select(index, car, "maximum_native_force")
    if not 100 <= len(selected) <= 300:
        raise ValueError(f"Unexpected bounded sample count: {len(selected)}")
    cases = []
    previous = None
    for index, row in enumerate(rows(raw)):
        if previous is not None:
            for car in range(7):
                if (index, car) not in selected:
                    continue
                p = f"car_{car}_"
                active = float(previous[p + "tunnel_front_valid"]) == 1.
                expected = float(row[p + "tunnel_force_n"])
                if not active and expected != 0.:
                    raise ValueError("Declared previous-row tunnel phase does not match DLL")
                if row[p + "mass_kg"] != previous[p + "mass_kg"]:
                    raise ValueError("SF static-mass context unexpectedly changed")
                cases.append({"force_row": index, "context_row": index - 1, "car": car,
                    "time_s": float(row["time_s"]), "reasons": sorted(selected[(index, car)]),
                    "active_previous": active,
                    "input": {"train_length_m": float(previous["train_length_m"]),
                        "train_mass_kg": float(previous["train_mass_kg"]),
                        "car_mass_kg": float(row[p + "mass_kg"]),
                        "width_m": dimensions[car][0], "height_m": dimensions[car][1],
                        "speed_mps": abs(float(previous[p + "velocity_local_mps"]))},
                    "tunnel_previous": {"ahead_valid": float(previous[p + "tunnel_ahead_valid"]) == 1.,
                        "behind_valid": float(previous[p + "tunnel_behind_valid"]) == 1.,
                        "length_ahead_m": float(previous[p + "tunnel_length_ahead_m"]),
                        "length_behind_m": float(previous[p + "tunnel_length_behind_m"]),
                        "num_paths": int(float(previous[p + "tunnel_paths"]))},
                    "expected_force_n": expected})
        previous = row
    args.out_dir.mkdir(parents=True)
    fixture = {"schema": 1, "reference_version": "1.6.1", "reference_commit": manifest["reference_commit"],
        "protocol_sha256": PROTOCOL, "tolerance_abs_n": 0.01,
        "route_hardware": {"authored_route_speed_limit_mps": route_speed,
            "authored_tunnel_overrides": False,
            "single": {"area_m2": 70., "perimeter_m": 32.},
            "double": {"area_m2": 100., "perimeter_m": 37.5},
            "origin": "OR1.6.1 TrainCar.Initialize779–858; source-default >=97.22m/s branch, original TRK hash recorded"},
        "scope": "Isolated prescribed geometry/speed/mass context, not a closed-loop route acceptance",
        "cases": cases}
    write(args.out_dir / "reference.json", fixture)
    shutil.copyfile(args.protocol, args.out_dir / "protocol.json")
    shutil.copyfile(__file__, args.out_dir / "extract-fixture.py")
    shutil.copyfile(capture / "manifest.json", args.out_dir / "native08-manifest.json")
    write(args.out_dir / "provenance.json", {"schema": 1,
        "kind": "selected_genuine_native08_tunnel_subsystem_outputs",
        "raw_trace_sha256": RAW08, "native07_projection_sha256": RAW07,
        "native08_manifest_sha256": sha(capture / "manifest.json"),
        "telemetry_integrity_sha256": sha(capture / "telemetry-integrity.json"),
        "original_trk_sha256": sha(trk), "native08_helper_sha256": sha(capture / "capture-sources/ActivityTrajectoryDiagnostics.cs"),
        "source_files_sha256": protocol["source_sha256"],
        "projection": "Select actual forceRowN outputs only; prescribed context is rowN−1; derive cross-section with Single(width*height), not observed TrainCrossSectionAreaM2. Store no native force formula.",
        "selection": protocol["selection"], "cases": len(cases),
        "geometry_calls": sum(c["active_previous"] for c in cases),
        "inactive_transition_records": sum(not c["active_previous"] for c in cases),
        "original_resources_bundled": False, "rust_comparison": "not_run",
        "files_sha256": {p.name: sha(p) for p in sorted(args.out_dir.iterdir()) if p.is_file()}})
    if args.publish_capture_dir:
        publish_capture(capture, args.publish_capture_dir, manifest)
    print(json.dumps({"status": "derived_not_compared", "cases": len(cases),
                      "geometry_calls": sum(c["active_previous"] for c in cases),
                      "fixture_sha256": sha(args.out_dir / "reference.json")}))


if __name__ == "__main__":
    main()

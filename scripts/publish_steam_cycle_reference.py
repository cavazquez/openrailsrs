#!/usr/bin/env python3
"""Publish bounded original-DLL steam evidence without bundling original content.

The captured manifest and its files remain immutable. Only reference.json is
losslessly gzip-compressed; publication.json records both byte identities.
Rust measurements retain their original executable/source hashes and result.
This command never runs Wine, Cargo, a comparator, or an original asset.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil


CAPTURE_FILES = {
    "registration.json", "input.json", "protocol.json", "capture-script.py",
    "SteamCycleReference.cs", "compile.log", "capture.log",
    "native/reference.json", "native/initial.json", "native/tables.json",
    "wine-isolation/isolation.json", "wine-isolation/release.json",
}
MEASUREMENT_FILES = {"comparison.json", "initial.json", "trace.csv", "receipt.json"}
COMMIT = "d16e670da333d26d2edfc97d5631a19dadf49ce5"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load(path):
    return json.loads(path.read_bytes())


def write(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n")


def relative(name):
    p = PurePosixPath(name)
    if p.is_absolute() or ".." in p.parts or "\\" in name:
        raise ValueError(f"Invalid evidence-relative path: {name}")
    return Path(*p.parts)


def verify_capture(capture):
    manifest_bytes = (capture / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    if (manifest.get("reference_version") != "1.6.1"
            or manifest.get("reference_commit") != COMMIT
            or manifest.get("status") != "captured_not_compared"
            or manifest.get("rust_acceptance") != "not_run"
            or set(manifest.get("files", {})) != CAPTURE_FILES):
        raise ValueError("Expected a complete, unchanged SteamCycleReference capture")
    for name, expected in manifest["files"].items():
        if digest((capture / relative(name)).read_bytes()) != expected:
            raise ValueError(f"Captured bytes changed: {name}")
    protocol = load(capture / "protocol.json")
    if digest((capture / "protocol.json").read_bytes()) != manifest["protocol_sha256"]:
        raise ValueError("Registered protocol changed")
    if not protocol.get("limits_declared_before_capture"):
        raise ValueError("Missing predeclared comparison limits")
    for name, expected in protocol["assemblies_sha256"].items():
        if manifest["assemblies"].get(name) != expected:
            raise ValueError(f"Original assembly identity changed: {name}")
    for name, expected in protocol["source_sha256"].items():
        if manifest["source_files"].get(name) != expected:
            raise ValueError(f"Original source identity changed: {name}")
    initial = load(capture / "native/initial.json")
    reference = load(capture / "native/reference.json")
    programme = load(capture / "input.json")
    if (len(reference["rows"]) != programme["ticks"] + 1
            or len(reference["rows"]) != manifest["rows"]
            or manifest["case"] != programme["case"]
            or reference["method"] != manifest["method"]):
        raise ValueError("Incomplete or inconsistent native capture")
    if "world_position" not in initial:
        raise ValueError("This publication requires the registered static centre position")
    if {r["phase"] for r in reference["rows"][1:]} != {c["phase"] for c in programme["commands"]}:
        raise ValueError("Native programme phase coverage is incomplete")
    isolation = load(capture / "wine-isolation/isolation.json")
    release = load(capture / "wine-isolation/release.json")
    if (isolation.get("registry_verified") is not True
            or isolation.get("graphics_driver") != "null"
            or isolation.get("show_crash_dialog") != 0
            or release.get("remaining_processes") != []
            or release.get("forbidden_drivers_seen") != []):
        raise ValueError("Missing verified private Wine isolation and clean release")
    return manifest_bytes, manifest, programme


def publish(capture, output, measurements):
    if output.exists():
        raise ValueError("Publication directory must be fresh; preserve prior evidence")
    manifest_bytes, manifest, programme = verify_capture(capture)
    checked_measurements = []
    for directory in measurements:
        receipt = load(directory / "receipt.json")
        if receipt.get("capture_manifest_sha256") != digest(manifest_bytes):
            raise ValueError(f"Rust receipt refers to a different capture: {directory}")
        # Source hashes belong to the executable that produced this receipt;
        # later source formatting must not rewrite or invalidate that record.
        outputs = receipt.get("outputs", {})
        if set(outputs) != MEASUREMENT_FILES - {"receipt.json"}:
            raise ValueError(f"Incomplete subsystem measurement receipt: {directory}")
        for name, expected in outputs.items():
            if digest((directory / relative(name)).read_bytes()) != expected:
                raise ValueError(f"Measured bytes changed: {directory}/{name}")
        comparison = load(directory / "comparison.json")
        if receipt.get("result") != comparison.get("status"):
            raise ValueError(f"Rust receipt/result mismatch: {directory}")
        checked_measurements.append((directory, receipt))
    output.mkdir(parents=True)
    published = {}
    (output / "manifest.json").write_bytes(manifest_bytes)
    published["manifest.json"] = {
        "sha256": digest(manifest_bytes),
        "transformation": "none; immutable captured manifest, including original captured status and local provenance paths",
    }
    for name in sorted(CAPTURE_FILES):
        source = capture / relative(name)
        data = source.read_bytes()
        compressed = name == "native/reference.json"
        target_name = name + ".gz" if compressed else name
        target = output / relative(target_name)
        target.parent.mkdir(parents=True, exist_ok=True)
        if compressed:
            payload = gzip.compress(data, compresslevel=9, mtime=0)
            if gzip.decompress(payload) != data:
                raise ValueError("Lossless compression round-trip failed")
            target.write_bytes(payload)
        else:
            shutil.copyfile(source, target)
        published[target_name] = {
            "sha256": digest(target.read_bytes()),
            "raw_capture_path": name,
            "raw_capture_sha256": manifest["files"][name],
            "raw_bytes": len(data),
            "transformation": "gzip level9 mtime0; decompressed bytes exactly equal captured bytes" if compressed else "none; byte-for-byte copy",
        }
    rust = []
    for index, (directory, receipt) in enumerate(checked_measurements, 1):
        prefix = f"measurements/{index:02d}"
        for name in sorted(MEASUREMENT_FILES):
            data = (directory / name).read_bytes()
            target_name = f"{prefix}/{name}"
            target = output / target_name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
            published[target_name] = {
                "sha256": digest(data),
                "transformation": "none; immutable Rust measurement bytes",
            }
        rust.append({
            "directory": prefix,
            "result": receipt["result"],
            "executable_sha256": receipt["executable_sha256"],
            "receipt_sha256": digest((directory / "receipt.json").read_bytes()),
            "claim": "This executable and these recorded sources only; no workspace-wide or route-drive acceptance follows from a subsystem result",
        })
    publication = {
        "schema": 1,
        "kind": "derived_publication_of_immutable_original_dll_capture",
        "reference_version": manifest["reference_version"],
        "reference_commit": manifest["reference_commit"],
        "captured_manifest_sha256": digest(manifest_bytes),
        "captured_status_preserved": manifest["status"],
        "case": manifest["case"],
        "rows": manifest["rows"],
        "scope": programme["kind"],
        "publication_script_sha256": digest(Path(__file__).read_bytes()),
        "original_resources_bundled": False,
        "post_start_observed_state_supplied_to_rust": False,
        "original_resource_hashes": {Path(p).name: h for p, h in manifest["originals"].items()},
        "files": published,
        "rust_measurements": rust,
        "recovery": "Read immutable manifest.json. For native/reference.json only, decompress native/reference.json.gz and verify the decompressed SHA256 against manifest.files. All other captured files are byte-for-byte copies. Original ENG/PAT/DLL/runtime/content directories are deliberately absent.",
    }
    write(output / "publication.json", publication)
    return publication


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--capture-dir", type=Path, required=True)
    ap.add_argument("--out-dir", type=Path, required=True)
    ap.add_argument("--measurement-dir", type=Path, action="append", default=[])
    a = ap.parse_args()
    result = publish(a.capture_dir.resolve(), a.out_dir.resolve(), [p.resolve() for p in a.measurement_dir])
    print(json.dumps({"status": "published", "case": result["case"], "rows": result["rows"],
                      "publication": str(a.out_dir / "publication.json"),
                      "rust_results": [m["result"] for m in result["rust_measurements"]]}))


if __name__ == "__main__":
    main()

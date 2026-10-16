#!/usr/bin/env python3
"""Replay capture04 with explicitly supplied fresh binaries; never builds or runs Wine."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from capture_lirr_full_service import compare, digest, native_coverage, read_trace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native-dir", type=Path, default=ROOT / "tmp/twenty-20261015/lirr-native-04")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--replay-binary", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--captured-pin", type=Path, help="Exact historical pin, for separately recorded catalog-only additions")
    args = parser.parse_args()
    native_dir = args.native_dir.resolve()
    capture_receipt = native_dir / "receipt.json"
    source_receipt = json.loads(capture_receipt.read_text())
    if source_receipt["status"] != "native_capture_complete" or not source_receipt["inputs_unchanged"]:
        raise ValueError("Requires the complete, unchanged original capture")
    for name, sha in source_receipt["outputs_sha256"].items():
        if digest(native_dir / name) != sha:
            raise ValueError(f"Native evidence changed: {name}")
    catalog_change = None
    for name, sha in source_receipt["input_sha256_after"].items():
        if digest(Path(name)) != sha:
            if Path(name) != ROOT / "oracles/openrails-reference.toml" or args.captured_pin is None:
                raise ValueError(f"Original/capture input changed: {name}")
            historical = args.captured_pin.resolve()
            if digest(historical) != sha:
                raise ValueError("The historical pin must match the exact captured hash")
            old = tomllib.loads(historical.read_text())
            new = tomllib.loads(Path(name).read_text())
            if {k: v for k, v in old.items() if k != "files"} != {k: v for k, v in new.items() if k != "files"}:
                raise ValueError("Original version/source/assembly pin changed")
            old_files = {r["path"]: r["sha256"] for r in old["files"]}
            new_files = {r["path"]: r["sha256"] for r in new["files"]}
            updated = {p: {"old": h, "new": new_files.get(p)} for p, h in old_files.items() if new_files.get(p) != h}
            if set(old_files) - set(new_files) or set(updated) - {"oracles/coupling-reference-provenance.json"}:
                raise ValueError("A previous reference or unrelated catalog record changed")
            catalog_change = {"historical_pin": str(historical), "historical_sha256": sha,
                              "current_sha256": digest(Path(name)), "unchanged_native_pin": True,
                              "updated_catalog_records": updated,
                              "added_catalog_records": {p: h for p, h in new_files.items() if p not in old_files},
                              "scope": "Root appended physical05 and the current coupling replay; no original capture/DLL/source/threshold changed."}
    protocol = source_receipt["protocol"]
    native = read_trace(native_dir / "native/trace.csv")
    outcome = json.loads((native_dir / "native/outcome.json").read_text())
    initial = json.loads((native_dir / "native/initial.json").read_text())
    if not native_coverage(native, protocol, outcome, initial)["passed"]:
        raise ValueError("Native coverage is incomplete")
    binary, replay_binary = args.binary.resolve(), args.replay_binary.resolve()
    if not binary.is_file() or not replay_binary.is_file():
        raise ValueError("Supply the newly built binaries explicitly; this helper does not build")
    out = args.out_dir.resolve()
    if out.exists() or not out.is_relative_to(ROOT / "tmp"):
        raise ValueError("Use a fresh task directory under tmp")
    out.mkdir(parents=True)
    activity = next(Path(p) for p in source_receipt["input_sha256_before"] if Path(p).name == protocol["activity"])
    consist = next(Path(p) for p in source_receipt["input_sha256_before"] if Path(p).suffix.casefold() == ".con")
    route = activity.parent.parent
    inputs = {str(p): digest(p) for p in (capture_receipt, binary, replay_binary, Path(__file__).resolve())}
    inputs.update({name: digest(Path(name)) for name in source_receipt["input_sha256_after"]})
    if args.captured_pin:
        inputs[str(args.captured_pin.resolve())] = digest(args.captured_pin)
    receipt = {"schema": 1, "status": "declared", "native_capture_receipt": str(capture_receipt),
               "native_trace_sha256": digest(native_dir / "native/trace.csv"),
               "protocol_predeclared": protocol, "input_sha256_before": inputs,
               "reference_catalog_change": catalog_change, "commands": []}
    record = out / "receipt.json"
    record.write_text(json.dumps(receipt, indent=2) + "\n")
    started = time.monotonic()
    try:
        for label, command in (
            ("import", [binary, "import-msts", route, "--activity", activity, "--consist", consist, "--out-dir", out / "imported"]),
            ("replay", [replay_binary, out / "imported/scenario.toml", native_dir / "native/trace.csv", out / "rust.csv"]),
        ):
            command = list(map(str, command))
            receipt["commands"].append({"name": label, "args": command})
            record.write_text(json.dumps(receipt, indent=2) + "\n")
            begin = time.monotonic()
            with (out / (label + ".log")).open("w") as log:
                result = subprocess.run(command, cwd=ROOT, stdin=subprocess.DEVNULL,
                                        stdout=log, stderr=subprocess.STDOUT, check=False)
            receipt["commands"][-1].update(returncode=result.returncode, wall_time_s=time.monotonic() - begin)
            if result.returncode:
                raise RuntimeError(f"{label} failed: {result.returncode}; preserve its log")
        result = compare(native, read_trace(out / "rust.csv"), protocol, outcome, initial)
        (out / "comparison.json").write_text(json.dumps(result, indent=2) + "\n")
        receipt["status"] = "pass" if result["passed"] else "failed_tolerance_or_coverage"
        if not result["passed"]:
            raise RuntimeError("Full native/Rust comparison failed at unchanged thresholds")
    except BaseException as error:
        receipt["error"] = str(error)
        if receipt["status"] == "declared":
            receipt["status"] = "failed_execution"
        raise
    finally:
        receipt["wall_time_s"] = time.monotonic() - started
        receipt["input_sha256_after"] = {p: digest(p) for p in inputs}
        receipt["inputs_unchanged"] = inputs == receipt["input_sha256_after"]
        if not receipt["inputs_unchanged"]:
            receipt["status"] = "failed_inputs_changed"
        outputs = [out / "rust.csv", out / "comparison.json", out / "import.log", out / "replay.log"]
        outputs.extend((out / "imported").rglob("*.toml"))
        outputs.extend((out / "imported").rglob("*.json"))
        receipt["outputs_sha256"] = {str(p.relative_to(out)): digest(p) for p in outputs if p.is_file()}
        record.write_text(json.dumps(receipt, indent=2) + "\n")
        if not receipt["inputs_unchanged"]:
            raise RuntimeError("Replay input changed during the attempt")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Run the coupling/depot QA clients against already-built Rust libraries.

First build coherent libraries normally. This script invokes rustc directly,
never Cargo; original ENG/WAG bytes are copied only to a fresh private QA folder.
"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from verify_coupling_reference import validate_comparison, validate_reference

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, default=ROOT / "oracles/coupling-reference.json")
    parser.add_argument("--stock", type=Path, required=True,
                        help="Original RF_Blue_Pullman trainset directory")
    parser.add_argument("--deps-dir", type=Path, default=ROOT / "target/release/deps")
    parser.add_argument("--rustc", default="rustc")
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    stock = args.stock.resolve(strict=True)
    source_root = args.source_root.resolve(strict=True)
    if out.exists() or out.is_relative_to(stock):
        raise ValueError("Use a fresh output directory outside original content")
    if out.is_relative_to(ROOT) and not out.is_relative_to(ROOT / "tmp"):
        raise ValueError("Original assets copied for QA must stay under ignored tmp/ or outside the repository")
    reference = json.loads(args.reference.read_text())
    validate_reference(reference)
    out.mkdir(parents=True)
    originals = out / "originals"
    originals.mkdir()
    vehicles = reference["coupling"]["after"]["vehicles"]
    hashes = {}
    for vehicle in vehicles:
        name = vehicle["file"]
        if Path(name).name != name or Path(name).suffix.lower() not in (".eng", ".wag"):
            raise ValueError("The reference must identify a single native vehicle filename")
        original = (stock / name).resolve(strict=True)
        if not original.is_relative_to(stock):
            raise ValueError("Original vehicle path escapes the chosen trainset")
        shutil.copy2(original, originals / name)
        hashes[name] = digest(original)
    for name, half in (("player.con", vehicles[:4]), ("donor.con", vehicles[4:])):
        lines = ["(Train"]
        for vehicle in half:
            path = (originals / vehicle["file"]).as_posix()
            if '"' in path:
                raise ValueError("QA paths containing a quotation mark are not supported")
            kind = "Engine" if vehicle["file"].lower().endswith(".eng") else "Wagon"
            lines.append(f'  ({kind} "{path}")')
        (out / name).write_text("\n".join([*lines, ")", ""]))
    route = json.dumps(str(source_root / "examples/yard_training/route"))
    scenario = f'''[scenario]
name="Original Pullman coupling comparison"
[route]
path={route}
start="a"
destination="destination"
start_offset_m=260.0
assume_signals_clear=true
[train]
consist="player.con"
[simulation]
duration=300.0
time_step=0.05
seed=42
multi_body=true
[gameplay]
objective="arrive"
difficulty="normal"
[output]
csv="run.csv"
metadata="run.json"
[[extra_trains]]
id="donor"
consist="donor.con"
start="a"
destination="destination"
start_offset_m=170.0
parked=true
output_csv="donor.csv"
'''
    (out / "scenario.toml").write_text(scenario)
    dependencies = {}
    command = [args.rustc, "--edition=2024", "-C", "opt-level=1", "-C", "debuginfo=0",
               "-C", "strip=symbols", "-L", "dependency=" + str(args.deps_dir.resolve())]
    for crate in ("openrailsrs_sim", "openrailsrs_scenarios", "serde_json"):
        candidates = list(args.deps_dir.glob("lib" + crate + "-*.rlib"))
        if not candidates:
            raise ValueError("Missing already-built dependency: " + crate)
        dependency = max(candidates, key=lambda path: path.stat().st_mtime)
        dependencies[crate] = {"file": dependency.name, "sha256": digest(dependency)}
        command += ["--extern", crate + "=" + str(dependency.resolve())]
    for helper, runner, inputs, output in (
        ("CouplingCompare.rs", "coupling-compare", [str(out / "scenario.toml"), str(args.reference.resolve())], "rust.json"),
        ("DepotStateCompare.rs", "depot-compare", [str(source_root)], "depot.json"),
    ):
        with (out / (runner + "-compile.log")).open("w") as log:
            subprocess.run([*command, str(ROOT / "tools/openrails-reference" / helper), "-o", str(out / runner)],
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
        with (out / (runner + "-capture.log")).open("w") as log:
            subprocess.run([str(out / runner), *inputs, str(out / output)],
                           stdout=log, stderr=subprocess.STDOUT, check=True, timeout=120)
    comparison = validate_comparison(reference, json.loads((out / "rust.json").read_text()))
    receipt = {"cargo_invoked": False, "runtime_modified": False, "original_assets_versioned": False,
               "dependencies": dependencies, "original_files": hashes, "comparison": comparison,
               "reference_sha256": digest(args.reference), "rust_output_sha256": digest(out / "rust.json"),
               "depot_output_sha256": digest(out / "depot.json")}
    (out / "receipt.json").write_text(json.dumps(receipt, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(comparison, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()

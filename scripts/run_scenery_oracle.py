#!/usr/bin/env python3
"""Compare Chiltern building composition with the pinned native OR reader.

Counts and primitive identities must match exactly; LOD distance tolerance is
1 mm. This checks the importer, not pixel parity or GPU placement.
"""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tomllib

from run_oracles import ROOT, verify


def differences(expected, actual, path="", distance_tolerance=0.001):
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return [f"{path}: expected object"]
        return [error for key, value in expected.items() for error in differences(
            value, actual.get(key), f"{path}.{key}", distance_tolerance)]
    if isinstance(expected, list):
        if not isinstance(actual, list) or len(expected) != len(actual):
            return [f"{path}: expected {len(expected)} entries"]
        return [error for index, (a, b) in enumerate(zip(expected, actual))
                for error in differences(a, b, f"{path}[{index}]", distance_tolerance)]
    if path.endswith(".selection_m") and isinstance(actual, (int, float)):
        if abs(expected - actual) <= distance_tolerance:
            return []
    elif expected == actual:
        return []
    return [f"{path}: expected {expected!r}, got {actual!r}"]


def compare_model(model, route_root, binary):
    shape = route_root / "SHAPES" / model["file"]
    if hashlib.sha256(shape.read_bytes()).hexdigest() != model["sha256"]:
        raise ValueError(f"Content changed: {shape}; recapture explicitly")
    actual = json.loads(subprocess.check_output(
        [str(binary.resolve()), "shape-dump", str(shape), "--json"], text=True))
    errors = differences(model["expected"], actual, model["file"])
    if errors:
        raise ValueError("\n".join(errors))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--route-root", required=True, type=Path)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/openrailsrs")
    args = parser.parse_args()
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    reference = json.loads((ROOT / "oracles/chiltern-scenery.json").read_text())
    source = ROOT.parent / "openrails"
    verify(pin, source if source.is_dir() else None)
    if (reference["version"], reference["commit"]) != (pin["version"], pin["commit"]):
        raise ValueError("Scenery oracle does not match the pinned Open Rails version")
    for model in reference["models"]:
        compare_model(model, args.route_root, args.binary)
        print(f"PASS native model composition: {model['file']}")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Scenery oracle FAIL: {error}", file=sys.stderr)
        sys.exit(1)

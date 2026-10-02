#!/usr/bin/env python3
"""Verify the immutable Open Rails reference, then run its acceptance suite."""

import argparse
import hashlib
from pathlib import Path
import subprocess
import sys
import tomllib

from verify_chiltern_service_capture import BASELINE, verify_capture

ROOT = Path(__file__).resolve().parents[1]


def verify(pin, source_root=None, installation_root=None):
    for entry in pin["files"]:
        path = ROOT / entry["path"]
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != entry["sha256"]:
            raise ValueError(f"Reference changed: {entry['path']}; recapture and review explicitly")
    verify_capture(BASELINE)
    log = (ROOT / pin["version_evidence"]).read_text()
    if f"Version    = {pin['version']}" not in log:
        raise ValueError("Capture log does not match the pinned Open Rails version")
    if source_root is not None:
        commit = subprocess.check_output(
            ["git", "-C", str(source_root), "-c", "core.fsmonitor=false", "rev-parse", "HEAD"], text=True
        ).strip()
        if commit != pin["commit"]:
            raise ValueError(f"Open Rails checkout is {commit}, expected {pin['commit']}")
        dirty = subprocess.check_output(
            ["git", "-C", str(source_root), "-c", "core.fsmonitor=false", "diff", "HEAD", "--", "Source"], text=True
        )
        if dirty:
            raise ValueError("Open Rails reference source has uncommitted modifications")
    if installation_root is not None:
        for entry in pin["binaries"]:
            path = installation_root / entry["name"]
            if hashlib.sha256(path.read_bytes()).hexdigest() != entry["sha256"]:
                raise ValueError(f"Installed Open Rails binary differs: {path}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path)
    parser.add_argument("--installation-root", type=Path)
    parser.add_argument("--verify-only", action="store_true")
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/openrailsrs")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "tmp/oracles")
    parser.add_argument("--suite", choices=("acceptance", "service"), default="acceptance",
        help="The new full-service physics target remains diagnostic and currently returns FAIL")
    args = parser.parse_args()
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    source = args.source_root
    if source is None and (ROOT.parent / "openrails/Source").is_dir():
        source = ROOT.parent / "openrails"
    verify(pin, source, args.installation_root)
    print(f"Reference OK: Open Rails {pin['version']} · {pin['commit']}", flush=True)
    if args.verify_only:
        return 0
    return subprocess.run([
        str(args.binary.resolve()), "oracle-suite", "--manifest",
        str(ROOT / "oracles" / ("chiltern-service.toml" if args.suite == "service" else "chiltern.toml")),
        "--out-dir", str(args.out_dir.resolve()),
    ], cwd=ROOT).returncode


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Oracle reference rejected: {error}", file=sys.stderr)
        sys.exit(1)

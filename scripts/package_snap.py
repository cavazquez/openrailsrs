#!/usr/bin/env python3
"""Prepare a small Snap build tree without downloads or previous build outputs.

Run snapcraft pack --use-lxd in the printed directory. A clean clone can also
run Snapcraft directly; this helper is for a working checkout with native data.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
ADDITIONS = (
    "crates/openrailsrs-bevy-scenery/src/texture_cache.rs",
    "packaging/resources.json", "scripts/package_linux.py", "scripts/package_snap.py",
    "snap/snapcraft.yaml", "snap/local/desktop-launch", "snap/local/display-launch.py",
)


def prepare(destination, refresh=False):
    destination = destination.resolve()
    if not refresh and destination.exists() and any(destination.iterdir()):
        raise ValueError("Choose an empty source directory")
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    # Include newly authored source files before the first commit, while keeping
    # ignored downloads and arbitrary untracked private data out of the tree.
    untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=ROOT).decode().split("\0")
    additions = {p for p in untracked if p.startswith("crates/") and Path(p).suffix in (".rs", ".wgsl")}
    manifest = ROOT / "packaging/resources.json"
    if manifest.is_file():
        additions.update(json.loads(manifest.read_text()))
    count = 0
    hashes = {}
    for relative in sorted(set(filter(None, paths)) | set(ADDITIONS) | additions):
        path = Path(relative)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError(f"Invalid source path: {relative}")
        if path.parts[0] in ("target", "tmp", "player-data", "official-content", ".git"):
            continue
        source = ROOT / path
        if not source.is_file() or source.is_symlink():
            continue
        if not source.resolve().is_relative_to(ROOT):
            raise ValueError(f"Source escapes the checkout: {relative}")
        target = destination / path
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        hashes[relative] = hashlib.sha256(target.read_bytes()).hexdigest()
        count += 1
    if not (destination / "snap/snapcraft.yaml").is_file():
        raise ValueError("Snap recipe is missing")
    try:
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                         text=True, stderr=subprocess.DEVNULL).strip()
        if not isinstance(commit, str) or len(commit) != 40 or any(c not in "0123456789abcdef" for c in commit):
            commit = None
        dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT,
                                             stderr=subprocess.DEVNULL))
    except (OSError, subprocess.CalledProcessError):
        commit, dirty = None, None
    provenance = destination / "packaging/source-build.json"
    provenance.parent.mkdir(parents=True, exist_ok=True)
    provenance.write_text(json.dumps({"version": 1, "source_commit": commit,
        "source_dirty": dirty, "files": hashes}, sort_keys=True) + "\n")
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, default=ROOT / "tmp/dist/snap-source")
    parser.add_argument("--refresh", action="store_true", help="refresh the prepared sources for a build retry")
    args = parser.parse_args()
    count = prepare(args.source_dir, args.refresh)
    print(f"{args.source_dir.resolve()} ({count} source files)")


if __name__ == "__main__":
    main()

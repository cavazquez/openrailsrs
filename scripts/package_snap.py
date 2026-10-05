#!/usr/bin/env python3
"""Prepare a small Snap build tree without downloads or previous build outputs.

Run snapcraft pack --use-lxd in the printed directory. A clean clone can also
run Snapcraft directly; this helper is for a working checkout with native data.
"""
import argparse
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
ADDITIONS = (
    "crates/openrailsrs-bevy-scenery/src/texture_cache.rs",
    "packaging/resources.json", "scripts/package_linux.py", "scripts/package_snap.py",
    "snap/snapcraft.yaml", "snap/local/desktop-launch",
)


def prepare(destination, refresh=False):
    destination = destination.resolve()
    if not refresh and destination.exists() and any(destination.iterdir()):
        raise ValueError("Choose an empty source directory")
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0")
    count = 0
    for relative in sorted(set(filter(None, paths)) | set(ADDITIONS)):
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
        count += 1
    if not (destination / "snap/snapcraft.yaml").is_file():
        raise ValueError("Snap recipe is missing")
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

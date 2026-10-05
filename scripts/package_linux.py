#!/usr/bin/env python3
"""Assemble a moved-binary distribution from source resources, never downloaded content."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tarfile

ROOT = Path(__file__).resolve().parents[1]
BINS = ("openrailsrs", "openrailsrs-viewer3d", "openrailsrs-prepare-content")
RESOURCE_PREFIXES = (
    "examples/",
    "crates/openrailsrs-bevy-scenery/assets/",
    "crates/openrailsrs-viewer3d/assets/",
)


def assemble(binaries, destination):
    destination = destination.resolve()
    if destination.exists() and any(destination.iterdir()):
        raise ValueError("Choose an empty output directory")
    shared = destination / "share/openrailsrs"
    (destination / "bin").mkdir(parents=True, exist_ok=True)
    for name in BINS:
        source = binaries / name
        if not source.is_file():
            raise ValueError(f"Missing compiled binary: {source}")
        shutil.copy2(source, destination / "bin" / name)
    # Git's allowlist excludes ignored native routes, downloads and local data.
    paths = json.loads((ROOT / "packaging/resources.json").read_text())
    for relative in filter(None, paths):
        if not relative.startswith(RESOURCE_PREFIXES) or ".." in Path(relative).parts:
            raise ValueError(f"Invalid resource manifest path: {relative}")
        source = ROOT / relative
        if source.is_symlink():
            continue
        if not source.resolve().is_relative_to(ROOT.resolve()):
            raise ValueError("Resource escapes the source tree")
        if source.name.startswith(("run.", "run_", "outcome.")) or source.suffix in (
            ".csv", ".webm", ".mp4"
        ):
            continue
        if not source.is_file():
            continue
        if relative.startswith(RESOURCE_PREFIXES[1]):
            target = shared / "assets" / source.relative_to(ROOT / RESOURCE_PREFIXES[1])
        elif relative.startswith(RESOURCE_PREFIXES[2]):
            target = shared / "viewer-assets" / source.relative_to(ROOT / RESOURCE_PREFIXES[2])
        else:
            target = shared / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    for name in ("LICENSE", "README.md"):
        if (ROOT / name).is_file():
            shutil.copy2(ROOT / name, destination / name)
    (destination / "openrailsrs.desktop").write_text(
        "[Desktop Entry]\nType=Application\nName=openrailsrs\n"
        "Comment=Simulador ferroviario\nExec=openrailsrs-viewer3d --menu\n"
        "Terminal=false\nCategories=Game;Simulation;\n"
    )
    manifest = {
        str(path.relative_to(destination)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(destination.rglob("*")) if path.is_file()
    }
    (destination / "manifest.json").write_text(json.dumps({
        "resources": "share/openrailsrs",
        "downloaded_content_included": False,
        "files": manifest,
    }, indent=2) + "\n")
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path, default=ROOT / "target/release")
    parser.add_argument("--output", type=Path, default=ROOT / "tmp/dist/openrailsrs-linux")
    parser.add_argument("--archive", type=Path)
    args = parser.parse_args()
    result = assemble(args.binaries.resolve(), args.output)
    if args.archive:
        args.archive.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(args.archive, "w:gz") as archive:
            archive.add(result, arcname="openrailsrs")
    print(result)


if __name__ == "__main__":
    main()

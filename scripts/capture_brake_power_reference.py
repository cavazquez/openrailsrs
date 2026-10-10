#!/usr/bin/env python3
"""Capture brake/power subsystems from the unmodified pinned OR 1.6.1 DLLs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

from capture_chiltern_service_or import clone_prefix, windows_path
from run_oracles import verify

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools/openrails-reference/BrakePowerReference.cs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installation-root", type=Path, default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wine-prefix", type=Path, default=ROOT / "tmp/or161-wine-prefix")
    parser.add_argument("--profiles", type=Path, default=ROOT / "examples/brake_supply_native/profiles.json")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    for protected in (args.installation_root.resolve(), args.wine_source_prefix.resolve(), ROOT.parent / "openrails", args.profiles.parent.resolve()):
        if out.is_relative_to(protected) or args.wine_prefix.resolve().is_relative_to(protected):
            raise ValueError("Capture output must stay outside original files and fixtures")
    if out.exists():
        raise ValueError("Choose a fresh output directory")
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    clone_prefix(args.wine_source_prefix, args.wine_prefix)
    out.mkdir(parents=True)
    runtime = out / "runtime"
    shutil.copytree(args.installation_root, runtime)
    env = dict(os.environ, WINEPREFIX=str(args.wine_prefix.resolve()), WINEARCH="win64", WINEDEBUG="-all")
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    references = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
                  "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]
    with (out / "compile.log").open("w") as log:
        subprocess.run(["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe", "/nologo", "/out:BrakePowerReference.exe"]
                       + ["/r:" + r for r in references] + [windows_path(HELPER)], cwd=runtime,
                       env=env, stdout=log, stderr=subprocess.STDOUT, timeout=60, check=True)
    reference = out / "reference.json"
    with (out / "capture.log").open("w") as log:
        subprocess.run(["wine", "BrakePowerReference.exe", windows_path(args.profiles), windows_path(reference)],
                       cwd=runtime, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=60, check=True)
    manifest = {"reference_version": pin["version"], "reference_commit": pin["commit"],
                "sha256": {n: hashlib.sha256((runtime / n).read_bytes()).hexdigest() for n in references},
                "helper_sha256": hashlib.sha256(HELPER.read_bytes()).hexdigest(),
                "profiles_sha256": hashlib.sha256(args.profiles.read_bytes()).hexdigest(),
                "output_sha256": hashlib.sha256(reference.read_bytes()).hexdigest()}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

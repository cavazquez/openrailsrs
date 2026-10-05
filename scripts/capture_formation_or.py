#!/usr/bin/env python3
"""Class 47 controls against the pinned original OR 1.6.1 simulation DLL.

Native content and installation remain read-only. The runtime, Wine prefix,
control trace and manifest are private, outside the downloaded package.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

from capture_chiltern_service_or import clone_prefix, driver_keyframes, windows_path
from run_oracles import verify

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools/openrails-reference/FormationCapture.cs"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--activity", type=Path, required=True)
    parser.add_argument("--installation-root", type=Path,
                        default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wine-prefix", type=Path, default=ROOT / "tmp/or161-wine-prefix")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    activity = args.activity.resolve(strict=True)
    content = activity.parents[3]
    for protected in (content, args.installation_root.resolve(), args.wine_source_prefix.resolve()):
        if out.is_relative_to(protected) or args.wine_prefix.resolve().is_relative_to(protected):
            raise ValueError(f"Capture files must stay outside the original files: {protected}")
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
    references = ["Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
                  "Orts.Formats.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]
    with (out / "compile.log").open("w") as log:
        subprocess.run(["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe",
                        "/nologo", "/out:FormationCapture.exe"] + ["/r:" + r for r in references]
                       + [windows_path(HELPER)], cwd=runtime, env=env, stdout=log,
                       stderr=subprocess.STDOUT, timeout=90, check=True)
    with (out / "capture.log").open("w") as log:
        subprocess.run(["wine", str(runtime / "FormationCapture.exe"), windows_path(activity),
                        windows_path(out / "capture")], cwd=runtime, env=env, stdout=log,
                       stderr=subprocess.STDOUT, timeout=240, check=True)
    driver_keyframes(out / "capture/trace.csv", out / "driver.csv", .05)
    verify(pin, ROOT.parent / "openrails", runtime)
    manifest = {
        "reference_version": "1.6.1", "reference_commit": pin["commit"],
        "activity_name": activity.name, "activity_sha256": hashlib.sha256(activity.read_bytes()).hexdigest(),
        "step_s": .05, "duration_s": 250, "seed": 0, "full_service": False,
        "helper_sha256": hashlib.sha256(HELPER.read_bytes()).hexdigest(),
        "binaries": {b["name"]: hashlib.sha256((runtime / b["name"]).read_bytes()).hexdigest()
                     for b in pin["binaries"]},
        "files": {str(p.relative_to(out)): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in [out / "driver.csv", *sorted((out / "capture").iterdir())]},
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

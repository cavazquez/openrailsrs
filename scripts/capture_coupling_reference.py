#!/usr/bin/env python3
"""Capture real coupling/track transfer from the unmodified pinned OR 1.6.1 DLL.

Requires a locally installed original Chiltern/Pullman and the existing private
Wine QA prefix. Generated inputs, helper and outputs stay in a fresh QA folder.
"""

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
HELPER = ROOT / "tools/openrails-reference/CouplingReference.cs"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installation-root", type=Path,
                        default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wine-prefix", type=Path, default=ROOT / "tmp/or161-wine-prefix")
    parser.add_argument("--content", type=Path,
                        default=Path.home() / "Documentos/Open Rails/Content/Chiltern")
    parser.add_argument("--native-path", default="RS_Maryleb-WRuislip0955.pat")
    parser.add_argument("--native-consist", default="Birmingham Pullman.con")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    out = args.out_dir.resolve()
    native_path = args.content / "ROUTES/Chiltern/PATHS" / args.native_path
    native_consist = args.content / "TRAINS/CONSISTS" / args.native_consist
    for protected in (args.installation_root.resolve(), args.wine_source_prefix.resolve(),
                      (ROOT.parent / "openrails").resolve(), args.content.resolve()):
        if out.is_relative_to(protected) or args.wine_prefix.resolve().is_relative_to(protected):
            raise ValueError("Capture outputs must stay outside the original sources and content")
    if out.exists():
        raise ValueError("Choose a fresh output directory")
    if not native_path.is_file() or not native_consist.is_file():
        raise ValueError("The original native PAT and CON must already be installed")
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    clone_prefix(args.wine_source_prefix, args.wine_prefix)
    out.mkdir(parents=True)
    runtime = out / "runtime"
    runtime.mkdir()
    for source in args.installation_root.iterdir():
        if source.is_file() and (source.suffix.lower() == ".dll" or
                                source.name in ("RunActivity.exe", "RunActivity.exe.config")):
            shutil.copy2(source, runtime / source.name)
    config = out / "config.json"
    config.write_text(json.dumps({"native_path": windows_path(native_path),
                                 "native_consist": windows_path(native_consist)}, indent=2) + "\n")
    env = dict(os.environ, WINEPREFIX=str(args.wine_prefix.resolve()), WINEARCH="win64", WINEDEBUG="-all")
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    references = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
                  "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]
    with (out / "compile.log").open("w") as log:
        subprocess.run(["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe", "/nologo",
                        "/out:CouplingReference.exe"] + ["/r:" + item for item in references] +
                       [windows_path(HELPER)], cwd=runtime, env=env,
                       stdout=log, stderr=subprocess.STDOUT, timeout=60, check=True)
    reference = out / "reference.json"
    with (out / "capture.log").open("w") as log:
        subprocess.run(["wine", "CouplingReference.exe", windows_path(config), windows_path(reference)],
                       cwd=runtime, env=env, stdout=log, stderr=subprocess.STDOUT,
                       timeout=300, check=True)
    manifest = {"reference_version": pin["version"], "reference_commit": pin["commit"],
                "assemblies": {item: digest(runtime / item) for item in references},
                "source_files": {name: digest(ROOT.parent / "openrails" / name) for name in (
                    "Source/Orts.Simulation/Simulation/Simulator.cs",
                    "Source/Orts.Simulation/Simulation/Physics/Train.cs",
                    "Source/Orts.Simulation/Simulation/Traveller.cs")},
                "helper_sha256": digest(HELPER), "capture_script_sha256": digest(Path(__file__)),
                "config_sha256": digest(config), "native_pat_sha256": digest(native_path),
                "native_consist_sha256": digest(native_consist), "output_sha256": digest(reference),
                "reference_source_and_dlls_unmodified": True,
                "scope": "Real coupling/state and occupancy handoff at prescribed contact; not full driven-route or moving-impact parity"}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Capture independent vacuum from the unmodified pinned Open Rails DLL.

Owned numeric fixtures declare controller inputs and tolerances before capture.
No original content, DLL or source file is changed. No viewer is started.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tomllib

from capture_chiltern_service_or import clone_prefix, windows_path
from run_oracles import verify
from oracle_wine import OracleWine

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / "tools/openrails-reference/VacuumIndependentReference.cs"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installation-root", type=Path,
                        default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wine-prefix", type=Path,
                        default=ROOT / "tmp/next-five-20261015/vacuum/wine-prefix")
    parser.add_argument("--content", type=Path,
                        default=Path.home() / "Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET")
    parser.add_argument("--input", type=Path, default=ROOT / "examples/vacuum_independent/reference-input.json")
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()
    args.input = args.input.resolve()
    out = args.out_dir.resolve()
    for protected in (args.installation_root.resolve(), args.wine_source_prefix.resolve(),
                      (ROOT.parent / "openrails").resolve(), args.content.resolve()):
        if out.is_relative_to(protected) or args.wine_prefix.resolve().is_relative_to(protected):
            raise ValueError("Outputs and Wine prefix must remain outside original content and sources")
    if out.exists():
        raise ValueError("Choose a fresh capture directory; failed captures are preserved")
    config = json.loads(args.input.read_text())
    source_files = {}
    for case in config["cases"]:
        for vehicle in case["vehicles"]:
            rel = Path(vehicle["source_original"])
            if rel.is_absolute() or ".." in rel.parts:
                raise ValueError("Original source must be relative to the installed trainset")
            actual = digest(args.content / rel)
            if actual != vehicle["source_original_sha256"]:
                raise ValueError(f"Original source changed: {rel}")
            source_files[str(rel)] = actual
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    out.mkdir(parents=True)
    before = {"input_sha256": digest(args.input), "helper_sha256": digest(HELPER),
              "wine_isolation_helper_sha256": digest(ROOT / "scripts/oracle_wine.py"),
              "tolerances_declared_before_capture": config["tolerances_declared_before_capture"],
              "fixture_sha256": {str((args.input.parent / vehicle["fixture"]).relative_to(ROOT)):
                                 digest(args.input.parent / vehicle["fixture"])
                                 for case in config["cases"] for vehicle in case["vehicles"]},
              "original_content_sha256": source_files}
    (out / "protocol-before.json").write_text(json.dumps(before, indent=2) + "\n")
    clone_prefix(args.wine_source_prefix, args.wine_prefix)
    runtime = out / "runtime"
    runtime.mkdir()
    for source in args.installation_root.iterdir():
        if source.is_file() and (source.suffix.lower() == ".dll" or
                                source.name in ("RunActivity.exe", "RunActivity.exe.config")):
            shutil.copy2(source, runtime / source.name)
    references = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Settings.dll", "Orts.Common.dll",
                  "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll", "GNU.Gettext.dll"]
    reference = out / "reference.json"
    assemblies_before = {name: digest(args.installation_root / name) for name in references}
    with OracleWine(args.wine_prefix.resolve(), out / "wine-isolation") as wine:
        wine.run(["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe", "/nologo",
                  "/out:VacuumIndependentReference.exe"] + ["/r:" + r for r in references] +
                 [windows_path(HELPER)], cwd=runtime, log=out / "compile.log", timeout=90)
        wine.run(["wine", "VacuumIndependentReference.exe", windows_path(args.input), windows_path(reference)],
                 cwd=runtime, log=out / "capture.log", timeout=120)
    if any(digest(args.installation_root / name) != expected or digest(runtime / name) != expected
           for name, expected in assemblies_before.items()):
        raise ValueError("Original or runtime assemblies changed during capture")
    for relative, expected in source_files.items():
        if digest(args.content / relative) != expected:
            raise ValueError(f"Original source changed during capture: {relative}")
    if digest(args.input) != before["input_sha256"] or digest(HELPER) != before["helper_sha256"]:
        raise ValueError("Capture inputs changed while running")
    captured = json.loads(reference.read_text())
    for case, spec in zip(captured["cases"], config["cases"], strict=True):
        if case["name"] != spec["name"] or len(case["points"]) != sum(p["ticks"] for p in spec["phases"]):
            raise ValueError("Native case/phase coverage differs from the protocol")
        if max(p["independent_command"] for p in case["points"]) <= 0:
            raise ValueError("Native controller did not traverse the prescribed independent commands")
        for phase in spec["phases"]:
            points = [p for p in case["points"] if p["phase"] == phase["name"]]
            if len(points) != phase["ticks"]:
                raise ValueError("Missing native phase points")
    methods = sorted({method for case in captured["cases"] for method in case["methods"]})
    manifest = dict(before, schema=1, reference_version=pin["version"], reference_commit=pin["commit"],
                    assemblies={name: digest(runtime / name) for name in references},
                    original_or_source_sha256={name: digest(ROOT.parent / "openrails" / name) for name in (
                        "Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/Brakes/MSTS/VacuumSinglePipe.cs",
                        "Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/Controllers/BrakeController.cs",
                        "Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/Controllers/MSTSNotchController.cs")},
                    capture_script_sha256=digest(Path(__file__)), output_sha256=digest(reference),
                    wine_isolation_release_sha256=digest(out / "wine-isolation/release.json"),
                    families=[case["name"] for case in config["cases"]],
                    phases={case["name"]: [p["name"] for p in case["phases"]] for case in config["cases"]},
                    called_native_methods=methods, original_content_and_dlls_unmodified=True,
                    scope=config["scope"])
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"reference": str(reference), "manifest": str(out / "manifest.json"),
                      "output_sha256": manifest["output_sha256"], "families": manifest["families"]}, indent=2))


if __name__ == "__main__":
    main()

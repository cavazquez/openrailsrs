#!/usr/bin/env python3
"""Capture original OR 1.6.1 lateral-risk diagnostics with prescribed inputs."""
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
HELPER = ROOT / "tools/openrails-reference/OperationalSignalsReference.cs"
INPUT = ROOT / "docs/fixtures/compatibility/operational-signals-or161/input.json"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installation-root", type=Path,
                        default=Path.home() / "wine64-OpenRails/drive_c/Program Files/Open Rails")
    parser.add_argument("--wine-source-prefix", type=Path, default=Path.home() / "wine64-OpenRails")
    parser.add_argument("--wine-prefix", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--wineserver", type=Path)
    args = parser.parse_args()
    out, prefix = args.out_dir.resolve(), args.wine_prefix.resolve()
    for protected in (args.installation_root.resolve(), args.wine_source_prefix.resolve(),
                      (ROOT.parent / "openrails").resolve()):
        if out.is_relative_to(protected) or prefix.is_relative_to(protected):
            raise ValueError("Outputs must remain outside original sources and installation")
    if out.exists() or prefix.exists():
        raise ValueError("Use fresh output and prefix directories; preserve failed captures")
    pin = tomllib.loads((ROOT / "oracles/openrails-reference.toml").read_text())
    verify(pin, ROOT.parent / "openrails", args.installation_root)
    out.mkdir(parents=True)
    refs = ["RunActivity.exe", "Orts.Simulation.dll", "Orts.Common.dll", "Orts.Settings.dll",
            "Orts.Formats.Msts.dll", "Orts.Parsers.Msts.dll", "MonoGame.Framework.dll", "Newtonsoft.Json.dll"]
    protocol = {"status": "DECLARED_BEFORE_EXECUTION", "input_sha256": digest(INPUT),
                "helper_sha256": digest(HELPER), "capture_script_sha256": digest(Path(__file__)),
                "input": json.loads(INPUT.read_text()), "reference_version": pin["version"],
                "reference_commit": pin["commit"],
                "assemblies": {name: digest(args.installation_root / name) for name in refs},
                "no_renderer_or_audio": True}
    (out / "protocol.json").write_text(json.dumps(protocol, indent=2) + "\n")
    clone_prefix(args.wine_source_prefix, prefix)
    runtime = out / "runtime"
    runtime.mkdir()
    for source in args.installation_root.iterdir():
        if source.is_file() and (source.suffix.lower() == ".dll" or
                                source.name in ("RunActivity.exe", "RunActivity.exe.config")):
            shutil.copy2(source, runtime / source.name)
    with OracleWine(prefix, out / "wine-isolation", args.wineserver) as wine:
        wine.run(["wine", r"C:\windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe", "/nologo",
                  "/out:OperationalSignalsReference.exe"] + ["/r:" + name for name in refs] +
                 [windows_path(HELPER)], cwd=runtime, log=out / "compile.log", timeout=90)
        output = out / "reference.json"
        wine.run(["wine", "OperationalSignalsReference.exe", windows_path(INPUT), windows_path(output)],
                 cwd=runtime, log=out / "capture.log", timeout=90)
    source_names = ["Source/Orts.Simulation/Simulation/Signalling/SignalObject.cs",
                    "Source/Orts.Simulation/Simulation/Signalling/SignalHead.cs",
                    "Source/Orts.Formats.Msts/TrackDatabaseFile.cs"]
    manifest = {"schema": 1, "reference_version": pin["version"], "reference_commit": pin["commit"],
                "protocol_sha256": digest(out / "protocol.json"), "input_sha256": digest(INPUT),
                "helper_sha256": digest(HELPER), "capture_script_sha256": digest(Path(__file__)),
                "assemblies": protocol["assemblies"],
                "source_files": {name: digest(ROOT.parent / "openrails" / name) for name in source_names},
                "output_sha256": digest(output), "reference_source_and_dlls_unmodified": True,
                "wine_isolation_helper_sha256": digest(ROOT / "scripts/oracle_wine.py"),
                "wine_isolation_release_sha256": digest(out / "wine-isolation/release.json"),
                "native_methods": protocol["input"]["native_methods"],
                "tolerances_predeclared": protocol["input"]["tolerances_predeclared"],
                "scope": protocol["input"]["scope"]}
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"rows": sum(len(value) for value in json.loads(output.read_text()).values() if isinstance(value,list)),
                      "output_sha256": manifest["output_sha256"]}))


if __name__ == "__main__":
    main()

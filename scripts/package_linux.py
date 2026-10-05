#!/usr/bin/env python3
"""Assemble a moved-binary distribution from source resources, never downloaded content."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
BINS = ("openrailsrs", "openrailsrs-viewer3d", "openrailsrs-prepare-content")
RESOURCE_PREFIXES = (
    "examples/",
    "crates/openrailsrs-bevy-scenery/assets/",
    "crates/openrailsrs-viewer3d/assets/",
)

LAUNCHER = r'''#!/bin/sh
set -eu
OPENRAILSRS_BUNDLE_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
case "${1:-}" in
  --check)
    shift
    failed=0
    for program in openrailsrs openrailsrs-viewer3d openrailsrs-prepare-content; do
      if [ ! -x "$OPENRAILSRS_BUNDLE_DIR/bin/$program" ]; then
        echo "Falta el ejecutable: bin/$program" >&2; failed=1
      elif command -v ldd >/dev/null 2>&1; then
        if libraries=$(ldd "$OPENRAILSRS_BUNDLE_DIR/bin/$program" 2>&1); then
          missing=$(printf '%s\n' "$libraries" | sed -n '/not found/p')
          if [ -n "$missing" ]; then echo "$program: $missing" >&2; failed=1; fi
        else
          echo "No se pudo comprobar $program: $libraries" >&2; failed=1
        fi
      fi
    done
    if ! command -v python3 >/dev/null 2>&1; then
      echo "Falta Python 3 para preparar y descargar el contenido original." >&2; failed=1
    fi
    if [ ! -f "$OPENRAILSRS_BUNDLE_DIR/share/openrailsrs/assets/shaders/or_cab.wgsl" ] ||
       [ ! -f "$OPENRAILSRS_BUNDLE_DIR/share/openrailsrs/examples/smoke/scenario.toml" ]; then
      echo "Faltan recursos del paquete. Volvé a extraer el archivo completo." >&2; failed=1
    fi
    [ "$failed" -eq 0 ] || exit 1
    "$OPENRAILSRS_BUNDLE_DIR/bin/openrailsrs" content --list
    echo "Paquete listo. Ejecutá ./Jugar.sh para abrir el menú."
    exit 0 ;;
  --cpu) export OPENRAILSRS_RENDERER=cpu; shift ;;
  --gpu) export OPENRAILSRS_RENDERER=gpu; shift ;;
  --auto) export OPENRAILSRS_RENDERER=auto; shift ;;
esac
exec "$OPENRAILSRS_BUNDLE_DIR/bin/openrailsrs-viewer3d" --menu "$@"
'''
QUICK_START = """openrailsrs — Linux x86_64

1. Extraé todo el archivo y conservá bin/ y share/ junto a Jugar.sh.
2. Abrí una terminal en esa carpeta y ejecutá ./Jugar.sh --check.
3. Ejecutá ./Jugar.sh. No necesitás Rust, Cargo ni compilar.
4. En el menú, Biblioteca muestra el contenido original y su carpeta de instalación.
   Las rutas y trenes originales se descargan por separado y quedan en tus datos de usuario.

./Jugar.sh --cpu utiliza el renderizador por software (Mesa/lavapipe).
./Jugar.sh --gpu exige una GPU compatible; --auto elige el dispositivo disponible.
CPU y GPU siguen compartiendo tareas de simulación, lectura y preparación de recursos.
El renderizador por software puede ser lento en rutas grandes.

Dependencias: Linux x86_64, Python 3, controladores Vulkan/Mesa, X11 o Wayland y
bibliotecas del sistema enumeradas por --check. La versión mínima de glibc depende
de los binarios; BUILD.json registra required_glibc y el sistema de construcción.
La prueba gráfica se hizo en el host de construcción. Este paquete no es AppImage.

Desde cualquier carpeta también podés ejecutar /ruta/bin/openrailsrs content --list.
No ejecutes como root. Guardados, ajustes y descargas usan XDG, fuera de este paquete.
Los scripts C# opcionales requieren un host .NET aparte; el TCS Rust está disponible.
"""



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
    launcher = destination / "Jugar.sh"
    launcher.write_text(LAUNCHER)
    launcher.chmod(0o755)
    (destination / "LEEME.txt").write_text(QUICK_START)
    import platform
    try:
        commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        commit = None
    try:
        source_dirty = bool(subprocess.check_output([
            "git", "status", "--porcelain", "--", "Cargo.toml", "Cargo.lock", "crates", "scripts", "packaging"
        ], cwd=ROOT, stderr=subprocess.DEVNULL))
    except (OSError, subprocess.CalledProcessError):
        source_dirty = None
    glibc_versions = []
    for name in BINS:
        try:
            versions = subprocess.check_output(
                ["readelf", "--version-info", str(destination / "bin" / name)],
                text=True, stderr=subprocess.DEVNULL,
            )
            glibc_versions.extend(tuple(map(int, value.split(".")))
                                  for value in re.findall(r"Name: GLIBC_([\d.]+)", versions))
        except (OSError, subprocess.CalledProcessError):
            pass
    required_glibc = ".".join(map(str, max(glibc_versions))) if glibc_versions else None
    if required_glibc:
        with (destination / "LEEME.txt").open("a") as instructions:
            instructions.write(f"\nEstos binarios requieren glibc {required_glibc} o superior.\n")
    (destination / "BUILD.json").write_text(json.dumps({
        "source_commit": commit, "source_dirty": source_dirty, "architecture": platform.machine(),
        "build_libc": list(platform.libc_ver()), "required_glibc": required_glibc,
        "downloaded_content_included": False,
        "binary_sha256": {name: hashlib.sha256((destination / "bin" / name).read_bytes()).hexdigest()
                          for name in BINS},
    }, indent=2) + "\n")
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

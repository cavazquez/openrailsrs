#!/usr/bin/env bash
# BNSF Scenic Subdivision — starter gratis TrainSimulations (texturas .dds, NA).
#
# Instalación:
#   https://www.trainsimulations.net/starter
#   Instalá en una carpeta OR separada (GLOBAL + ROUTES + TRAINS + SOUND).
#   export BNSF_SCENIC_ROOT="$HOME/Documentos/Open Rails/Content/TrainSimulations"
#
# Uso:
#   ./scripts/run_render3d_bnsf_scenic.sh
#   ./scripts/run_render3d_bnsf_scenic.sh -- --radius 1

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/render3d_common.sh
source "$SCRIPT_DIR/lib/render3d_common.sh"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  render3d_usage_header "$0" "BNSF_SCENIC_ROOT   raíz OR con la ruta instalada
  BNSF_SCENIC_ROUTE  carpeta de ruta (opcional; auto-detecta ROUTES/*Scenic*)"
  exit 0
fi

MSTS_ROOT="${BNSF_SCENIC_ROOT:-}"
if [[ -z "$MSTS_ROOT" ]]; then
  MSTS_ROOT="$(render3d_first_existing_dir \
    "$HOME/Documentos/Open Rails/Content/TrainSimulations" \
    "$HOME/Documentos/Open Rails/Content/BNSF" \
    "$HOME/Documentos/Open Rails/Content/BNSFScenic" \
    "$HOME/routes/TrainSimulations" \
    "$HOME/routes/BNSFScenic" || true)"
fi

if [[ -z "$MSTS_ROOT" ]]; then
  echo "error: no se encontró instalación BNSF Scenic." >&2
  echo "Descargá el starter en https://www.trainsimulations.net/starter" >&2
  echo "y exportá BNSF_SCENIC_ROOT=/carpeta/con/GLOBAL+ROUTES+TRAINS" >&2
  exit 1
fi

MSTS_ROOT="$(render3d_resolve_msts_root "$MSTS_ROOT")"

ROUTE_DIR="${BNSF_SCENIC_ROUTE:-}"
if [[ -z "$ROUTE_DIR" ]]; then
  ROUTE_DIR="$(render3d_find_route_dir "$MSTS_ROOT" \
    "BNSF Scenic Sub" \
    BNSFScenic \
    BNSF_Scenic \
    "BNSF Scenic" \
    Scenic \
    Demo)"
fi

EXTRA=()
if [[ "${1:-}" == "--" ]]; then
  shift
  EXTRA=("$@")
fi

RADIUS="${RENDER3D_RADIUS:-0}"
if [[ ${#EXTRA[@]} -eq 0 ]]; then
  EXTRA=(--radius "$RADIUS")
fi

render3d_run "$MSTS_ROOT" "$ROUTE_DIR" "${EXTRA[@]}"

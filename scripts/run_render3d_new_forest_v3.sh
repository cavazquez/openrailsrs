#!/usr/bin/env bash
# New Forest Route V3 (Rick Loader) — Bournemouth UK, OR-only, ~20 GB, TERRTEX fotorreal.
#
# Instalación (no uses el ZIP — cloná con Git):
#   git clone https://github.com/rickloader/NewForestRouteV3.git
#   export NEW_FOREST_V3_ROOT="$HOME/routes/NewForestRouteV3"
#
# Uso:
#   ./scripts/run_render3d_new_forest_v3.sh
#   ./scripts/run_render3d_new_forest_v3.sh -- --radius 0
#   NEW_FOREST_V3_ROOT=/otra ./scripts/run_render3d_new_forest_v3.sh -- --tile-x -6096 --tile-z 14916

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/render3d_common.sh
source "$SCRIPT_DIR/lib/render3d_common.sh"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  render3d_usage_header "$0" "NEW_FOREST_V3_ROOT   raíz del clon (GLOBAL/, Routes/, TRAINS/)
  NEW_FOREST_V3_ROUTE  carpeta de ruta (default: Routes/Watersnake)"
  exit 0
fi

MSTS_ROOT="${NEW_FOREST_V3_ROOT:-}"
if [[ -z "$MSTS_ROOT" ]]; then
  MSTS_ROOT="$(render3d_first_existing_dir \
    "$HOME/Documentos/Open Rails/Content/NewForestRouteV3" \
    "$HOME/Documentos/Open Rails/Content/NewForestRoute" \
    "$HOME/routes/NewForestRouteV3" \
    "$HOME/routes/NewForestRoute" \
    "$HOME/NewForestRouteV3" || true)"
fi

if [[ -z "$MSTS_ROOT" ]]; then
  echo "error: no se encontró New Forest V3." >&2
  echo "Cloná https://github.com/rickloader/NewForestRouteV3 (Git, no ZIP) y exportá NEW_FOREST_V3_ROOT=/ruta/al/clon" >&2
  exit 1
fi

MSTS_ROOT="$(render3d_resolve_msts_root "$MSTS_ROOT")"

ROUTE_DIR="${NEW_FOREST_V3_ROUTE:-}"
if [[ -z "$ROUTE_DIR" ]]; then
  ROUTE_DIR="$(render3d_find_route_dir "$MSTS_ROOT" Watersnake)"
fi

EXTRA=()
if [[ "${1:-}" == "--" ]]; then
  shift
  EXTRA=("$@")
fi

# Cámara: sin .act (New Forest es OR-only) se usa la vía del tile central.
# Para un path concreto: RENDER3D_PLAYER_PATH=PATHS/foo.pat ./scripts/run_render3d_new_forest_v3.sh
#   o: ./scripts/run_render3d_new_forest_v3.sh -- --player-path PATHS/foo.pat --path-offset-m 0

# Ruta pesada: 1 tile por defecto; subí radius solo si tenés RAM de sobra.
RADIUS="${RENDER3D_RADIUS:-0}"
if [[ ${#EXTRA[@]} -eq 0 ]]; then
  EXTRA=(--radius "$RADIUS")
fi

if [[ -n "${RENDER3D_PLAYER_PATH:-}" ]]; then
  EXTRA+=(--player-path "$RENDER3D_PLAYER_PATH")
  if [[ -n "${RENDER3D_PATH_OFFSET_M:-}" ]]; then
    EXTRA+=(--path-offset-m "$RENDER3D_PATH_OFFSET_M")
  fi
fi

render3d_run "$MSTS_ROOT" "$ROUTE_DIR" "${EXTRA[@]}"

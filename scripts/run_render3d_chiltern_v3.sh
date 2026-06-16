#!/usr/bin/env bash
# Chiltern Route v3 (DocMartin) — Open Rails only, remaster UK.
#
# Instalación:
#   git clone https://github.com/DocMartin7644/Chiltern-Route-v3.git
#   export CHILTERN_V3_ROOT="$HOME/routes/Chiltern-Route-v3"
#
# Uso:
#   ./scripts/run_render3d_chiltern_v3.sh
#   ./scripts/run_render3d_chiltern_v3.sh -- --radius 0
#   CHILTERN_V3_ROOT=/otra/ruta ./scripts/run_render3d_chiltern_v3.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/render3d_common.sh
source "$SCRIPT_DIR/lib/render3d_common.sh"

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  render3d_usage_header "$0" "CHILTERN_V3_ROOT   raíz con GLOBAL/, ROUTES/, TRAINS/ (default: varias rutas típicas)"
  exit 0
fi

MSTS_ROOT="${CHILTERN_V3_ROOT:-}"
if [[ -z "$MSTS_ROOT" ]]; then
  MSTS_ROOT="$(render3d_first_existing_dir \
    "$HOME/Documentos/Open Rails/Content/Chiltern-Route-v3" \
    "$HOME/Documentos/Open Rails/Content/Chiltern-Route" \
    "$HOME/routes/Chiltern-Route-v3" \
    "$HOME/routes/Chiltern-Route" \
    "$HOME/Chiltern-Route-v3" || true)"
fi

if [[ -z "$MSTS_ROOT" ]]; then
  echo "error: no se encontró Chiltern v3." >&2
  echo "Cloná https://github.com/DocMartin7644/Chiltern-Route-v3 y exportá CHILTERN_V3_ROOT=/ruta/al/clon" >&2
  exit 1
fi

MSTS_ROOT="$(render3d_resolve_msts_root "$MSTS_ROOT")"

ROUTE_DIR="${CHILTERN_V3_ROUTE:-}"
if [[ -z "$ROUTE_DIR" ]]; then
  ROUTE_DIR="$(render3d_find_route_dir "$MSTS_ROOT" Chiltern)"
fi

EXTRA=()
if [[ "${1:-}" == "--" ]]; then
  shift
  EXTRA=("$@")
fi

RADIUS="${RENDER3D_RADIUS:-1}"
if [[ ${#EXTRA[@]} -eq 0 ]]; then
  EXTRA=(--radius "$RADIUS")
fi

render3d_run "$MSTS_ROOT" "$ROUTE_DIR" "${EXTRA[@]}"

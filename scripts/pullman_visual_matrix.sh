#!/usr/bin/env bash
# Matriz visual Pullman — capturas PNG automáticas (OPENRAILSRS_SCREENSHOT).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/tmp/pullman_matrix}"
mkdir -p "$OUT"

export OPENRAILSRS_MSTS_CONTENT="${OPENRAILSRS_MSTS_CONTENT:-$HOME/Documentos/Open Rails/Content}"
CHILTERN_ROUTE="${CHILTERN_ROUTE:-$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern}"
SCENARIO="$ROOT/examples/chiltern/scenario.toml"

# Corredor mínimo: tren Pullman visible en ~5 s; cámara chase por defecto.
export OPENRAILSRS_SCREENSHOT_DELAY_S="${OPENRAILSRS_SCREENSHOT_DELAY_S:-14}"
export OPENRAILSRS_VISIBLE_RADIUS_M=400

VIEWER=(cargo run --release -q -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" "$SCENARIO")

run_mode() {
  local id="$1"
  local label="$2"
  shift 2
  local png="$OUT/${id}_${label}.png"
  echo ""
  echo "════════════════════════════════════════"
  echo "  [$id] $label"
  echo "  → $png"
  echo "════════════════════════════════════════"
  env "$@" OPENRAILSRS_SCREENSHOT="$png" "${VIEWER[@]}" 2>&1 | \
    grep -E 'spawn|Pullman|consist|screenshot|train:live|ERROR|warning: could not' || true
  if [[ -f "$png" ]]; then
    echo "  ✓ $(du -h "$png" | cut -f1)"
  else
    echo "  ✗ CAPTURA FALLIDA" >&2
    return 1
  fi
}

echo "Salida: $OUT"
echo "Route:  $CHILTERN_ROUTE"
echo "Delay:  ${OPENRAILSRS_SCREENSHOT_DELAY_S}s por captura"

run_mode "A1" "baseline_produccion"
run_mode "A2" "force_double_sided" OPENRAILSRS_DEBUG_FORCE_DOUBLE_SIDED=1
run_mode "A3" "cull_normal_single_sided" OPENRAILSRS_DEBUG_CULL_NORMAL=1
run_mode "A4" "flip_winding" OPENRAILSRS_DEBUG_FLIP_WINDING=1
run_mode "A5" "cull_normal_plus_flip_winding" \
  OPENRAILSRS_DEBUG_CULL_NORMAL=1 OPENRAILSRS_DEBUG_FLIP_WINDING=1
run_mode "B1" "flip_u" OPENRAILSRS_DEBUG_FLIP_U=1
run_mode "C1" "face_colors_back_rojo" OPENRAILSRS_DEBUG_FACE_COLORS=back
run_mode "C2" "face_colors_front_verde" OPENRAILSRS_DEBUG_FACE_COLORS=front
run_mode "A6" "cull_front_solo_backfaces" OPENRAILSRS_DEBUG_CULL_FRONT=1

echo ""
echo "Listo. Capturas en: $OUT"
ls -la "$OUT"/*.png 2>/dev/null || true

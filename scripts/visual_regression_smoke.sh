#!/usr/bin/env bash
# Visual regression smoke (#43): fixed camera → PNG → compare vs golden.
#
# Usage:
#   ./scripts/visual_regression_smoke.sh
#   UPDATE_GOLDEN=1 ./scripts/visual_regression_smoke.sh
#
# Exit codes:
#   0 — OK (or golden updated)
#   1 — capture/compare failed
#   3 — render environment unavailable (no PNG produced; CI may treat specially)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ROUTE="${OPENRAILSRS_VISUAL_ROUTE:-examples/smoke/routes/test}"
GOLDEN="${OPENRAILSRS_VISUAL_GOLDEN:-docs/fixtures/visual/smoke_orbit.png}"
OUT_DIR="${OPENRAILSRS_VISUAL_OUT:-tmp/visual_smoke}"
ACTUAL="${OUT_DIR}/actual.png"
DIFF="${OUT_DIR}/diff.png"

mkdir -p "$OUT_DIR" "$(dirname "$GOLDEN")"

# Deterministic orbit + window (matches golden 640×360).
export OPENRAILSRS_WINDOW_WIDTH="${OPENRAILSRS_WINDOW_WIDTH:-640}"
export OPENRAILSRS_WINDOW_HEIGHT="${OPENRAILSRS_WINDOW_HEIGHT:-360}"
export OPENRAILSRS_CAM_YAW="${OPENRAILSRS_CAM_YAW:-0.85}"
export OPENRAILSRS_CAM_PITCH="${OPENRAILSRS_CAM_PITCH:--0.35}"
export OPENRAILSRS_CAM_DIST="${OPENRAILSRS_CAM_DIST:-180}"
export OPENRAILSRS_VIEW_RADIUS_M="${OPENRAILSRS_VIEW_RADIUS_M:-400}"
export OPENRAILSRS_SCREENSHOT_AFTER_READY="${OPENRAILSRS_SCREENSHOT_AFTER_READY:-1}"
export OPENRAILSRS_SCREENSHOT_READY_FRAMES="${OPENRAILSRS_SCREENSHOT_READY_FRAMES:-45}"
export OPENRAILSRS_SCREENSHOT_DELAY_S="${OPENRAILSRS_SCREENSHOT_DELAY_S:-90}"
export OPENRAILSRS_SCREENSHOT="$ACTUAL"

TOL="${OPENRAILSRS_VISUAL_TOL:-16}"
MAX_HOT_PCT="${OPENRAILSRS_VISUAL_MAX_HOT_PCT:-2}"

rm -f "$ACTUAL" "$DIFF"

echo "=== visual regression smoke ==="
echo "route:   $ROUTE"
echo "actual:  $ACTUAL"
echo "golden:  $GOLDEN"
echo "window:  ${OPENRAILSRS_WINDOW_WIDTH}x${OPENRAILSRS_WINDOW_HEIGHT}"
echo "cam:     yaw=$OPENRAILSRS_CAM_YAW pitch=$OPENRAILSRS_CAM_PITCH dist=$OPENRAILSRS_CAM_DIST"
echo "ready:   AFTER_READY=$OPENRAILSRS_SCREENSHOT_AFTER_READY frames=$OPENRAILSRS_SCREENSHOT_READY_FRAMES max_delay=${OPENRAILSRS_SCREENSHOT_DELAY_S}s"

set +e
cargo run --release -q -p openrailsrs-viewer3d --bin openrailsrs-viewer3d -- "$ROUTE"
viewer_rc=$?
set -e

if [[ ! -f "$ACTUAL" ]]; then
  echo "error: screenshot not written (viewer exit=$viewer_rc)" >&2
  exit 3
fi

echo "captured: $(du -h "$ACTUAL" | cut -f1) ($ACTUAL)"

if [[ "${UPDATE_GOLDEN:-}" == "1" ]]; then
  cp -f "$ACTUAL" "$GOLDEN"
  echo "updated golden → $GOLDEN"
  exit 0
fi

if [[ ! -f "$GOLDEN" ]]; then
  echo "error: golden missing: $GOLDEN" >&2
  echo "hint: UPDATE_GOLDEN=1 $0" >&2
  exit 1
fi

set +e
cargo run --release -q -p openrailsrs-viewer3d --bin openrailsrs-visual-diff -- \
  "$ACTUAL" "$GOLDEN" --diff "$DIFF" --tol "$TOL" --max-hot-pct "$MAX_HOT_PCT"
diff_rc=$?
set -e

if [[ "$diff_rc" -ne 0 ]]; then
  echo "visual-diff failed (rc=$diff_rc); see $ACTUAL and $DIFF" >&2
  exit 1
fi

echo "OK — within thresholds (tol=$TOL, max hot %=$MAX_HOT_PCT)"
exit 0

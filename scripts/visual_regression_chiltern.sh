#!/usr/bin/env bash
# Visual regression Chiltern Birmingham — exterior + cab multi-view (#71 / #170).
#
# Captures fixed views against baseline openrailsrs goldens (or regenerates
# them). Optional OR side-by-side: place cabina.png / desdeafuera.png under
# docs/fixtures/visual/or_reference/ for manual checklist (not CI).
#
# Usage:
#   ./scripts/visual_regression_chiltern.sh
#   UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh
#
# Clean capture env (recommended for goldens):
#   env -i HOME="$HOME" USER="$USER" PATH="$PATH" DISPLAY="${DISPLAY:-}" \
#     WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-}" \
#     OPENRAILSRS_MSTS_CONTENT="$OPENRAILSRS_MSTS_CONTENT" \
#     ./scripts/visual_regression_chiltern.sh
#
# Requires:
#   OPENRAILSRS_MSTS_CONTENT → Chiltern route on disk
#   GPU / xvfb for capture (same as visual_regression_smoke.sh)
#
# Exit codes:
#   0 — all views within thresholds (or goldens updated)
#   1 — capture/compare failed
#   2 — Chiltern content missing
#   3 — screenshot not written (GPU/env)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CONTENT="${OPENRAILSRS_MSTS_CONTENT:-}"
if [[ -z "$CONTENT" ]]; then
  echo "error: set OPENRAILSRS_MSTS_CONTENT to your MSTS/OR Content root" >&2
  exit 2
fi

CHILTERN_ROUTE="${OPENRAILSRS_CHILTERN_ROUTE:-$CONTENT/Chiltern/ROUTES/Chiltern}"
SCENARIO="${OPENRAILSRS_CHILTERN_SCENARIO:-examples/chiltern/scenario.toml}"
if [[ ! -d "$CHILTERN_ROUTE" ]]; then
  echo "error: Chiltern route missing: $CHILTERN_ROUTE" >&2
  exit 2
fi
if [[ ! -f "$SCENARIO" ]]; then
  echo "error: scenario missing: $SCENARIO" >&2
  exit 2
fi

OUT_DIR="${OPENRAILSRS_VISUAL_OUT:-tmp/visual_chiltern}"
GOLDEN_DIR="${OPENRAILSRS_VISUAL_GOLDEN_DIR:-docs/fixtures/visual/chiltern}"
mkdir -p "$OUT_DIR" "$GOLDEN_DIR"

export OPENRAILSRS_WINDOW_WIDTH="${OPENRAILSRS_WINDOW_WIDTH:-640}"
export OPENRAILSRS_WINDOW_HEIGHT="${OPENRAILSRS_WINDOW_HEIGHT:-360}"
export OPENRAILSRS_VIEW_RADIUS_M="${OPENRAILSRS_VIEW_RADIUS_M:-400}"
export OPENRAILSRS_SCREENSHOT_AFTER_READY="${OPENRAILSRS_SCREENSHOT_AFTER_READY:-1}"
export OPENRAILSRS_SCREENSHOT_READY_FRAMES="${OPENRAILSRS_SCREENSHOT_READY_FRAMES:-60}"
export OPENRAILSRS_SCREENSHOT_DELAY_S="${OPENRAILSRS_SCREENSHOT_DELAY_S:-180}"
# Fog stays at viewer default (on, #39) for stable baselines.

TOL="${OPENRAILSRS_VISUAL_TOL:-24}"
MAX_HOT_PCT="${OPENRAILSRS_VISUAL_MAX_HOT_PCT:-3}"

capture_view() {
  local name="$1"
  local actual="$OUT_DIR/${name}.png"
  local golden="$GOLDEN_DIR/${name}.png"
  shift
  # Remaining args are env assignments applied for this capture.
  (
    export OPENRAILSRS_SCREENSHOT="$actual"
    for kv in "$@"; do
      export "$kv"
    done
    rm -f "$actual"
    echo "=== capture $name ==="
    echo "  screenshot: $actual"
    set +e
    cargo run --release -q -p openrailsrs-viewer3d --bin openrailsrs-viewer3d -- \
      --live --route-root "$CHILTERN_ROUTE" "$SCENARIO"
    local rc=$?
    set -e
    if [[ ! -f "$actual" ]]; then
      echo "error: screenshot not written for $name (viewer exit=$rc)" >&2
      exit 3
    fi
    if [[ "${UPDATE_GOLDEN:-}" == "1" ]]; then
      cp -f "$actual" "$golden"
      echo "updated golden → $golden"
      return 0
    fi
    if [[ ! -f "$golden" ]]; then
      echo "error: golden missing: $golden" >&2
      echo "hint: UPDATE_GOLDEN=1 $0" >&2
      exit 1
    fi
    cargo run --release -q -p openrailsrs-viewer3d --bin openrailsrs-visual-diff -- \
      "$actual" "$golden" --diff "$OUT_DIR/${name}_diff.png" \
      --tol "$TOL" --max-hot-pct "$MAX_HOT_PCT"
  )
}

echo "=== visual regression Chiltern (#71 / #170 cab slice) ==="
echo "route:    $CHILTERN_ROUTE"
echo "scenario: $SCENARIO"
echo "out:      $OUT_DIR"
echo "goldens:  $GOLDEN_DIR"

# Exterior: fixed orbit near Birmingham station (tile ≈ -6080 / 14925).
# Poses tuned for marquesina / platform framing; regenerate if scenery LOD changes.
capture_view "birmingham_exterior" \
  "OPENRAILSRS_FOLLOW=orbit" \
  "OPENRAILSRS_CAM_YAW=0.95" \
  "OPENRAILSRS_CAM_PITCH=-0.28" \
  "OPENRAILSRS_CAM_DIST=220"

# Cabina: first-person driver views (cab mesh + CVF). LOOK_* in radians (#170).
capture_view "birmingham_cabina" \
  "OPENRAILSRS_FOLLOW=driver" \
  "OPENRAILSRS_LOOK_YAW=0" \
  "OPENRAILSRS_LOOK_PITCH=0"

capture_view "birmingham_cabina_up" \
  "OPENRAILSRS_FOLLOW=driver" \
  "OPENRAILSRS_LOOK_YAW=0" \
  "OPENRAILSRS_LOOK_PITCH=0.55"

capture_view "birmingham_cabina_left" \
  "OPENRAILSRS_FOLLOW=driver" \
  "OPENRAILSRS_LOOK_YAW=0.7" \
  "OPENRAILSRS_LOOK_PITCH=0"

capture_view "birmingham_cabina_right" \
  "OPENRAILSRS_FOLLOW=driver" \
  "OPENRAILSRS_LOOK_YAW=-0.7" \
  "OPENRAILSRS_LOOK_PITCH=0"

echo "OK — Chiltern exterior + cab multi-view within thresholds (tol=$TOL, max hot %=$MAX_HOT_PCT)"
echo "OR reference (manual): docs/fixtures/visual/or_reference/{desdeafuera,cabina}.png"
exit 0

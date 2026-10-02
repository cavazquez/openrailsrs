#!/usr/bin/env bash
# Verificación local y en CI: formato, lints, tests y build del workspace.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

export OPENRAILSRS_DISABLE_AUDIO=1
# Clear session/visual overrides that leak into unit tests (view radius, camera, screenshots).
unset OPENRAILSRS_FOLLOW OPENRAILSRS_CAM_YAW OPENRAILSRS_CAM_PITCH OPENRAILSRS_CAM_DIST
unset OPENRAILSRS_VIEW_RADIUS_M OPENRAILSRS_VISIBLE_RADIUS_M
unset OPENRAILSRS_SCREENSHOT OPENRAILSRS_SCREENSHOT_DELAY_S OPENRAILSRS_SCREENSHOT_READY_FRAMES
unset OPENRAILSRS_SCREENSHOT_AFTER_READY OPENRAILSRS_WINDOW_WIDTH OPENRAILSRS_WINDOW_HEIGHT

echo "==> rustfmt (cargo fmt --check)"
cargo fmt --all -- --check

echo "==> clippy (-D warnings)"
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

echo "==> tests"
# Serial: several suites share process-global counters and environment variables.
# All focused regressions are already included here; keep one Bevy feature set.
cargo test --locked --workspace --all-features -- --test-threads=1

echo "==> build"
cargo build --locked --workspace --all-features

echo "==> pinned Open Rails acceptance oracles"
python3 scripts/run_oracles.py

echo "==> complete station service"
target/debug/openrailsrs play-service examples/chiltern_local/scenario.toml --out-dir tmp/service-check

echo "OK: check.sh completó sin errores."

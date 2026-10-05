#!/usr/bin/env bash
# Verificación local y en CI: formato, lints, tests y build del workspace.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

# Match the playable launcher and share one compact Bevy build across checks.
export CARGO_PROFILE_DEV_OPT_LEVEL="${CARGO_PROFILE_DEV_OPT_LEVEL:-1}"
export CARGO_PROFILE_TEST_OPT_LEVEL="${CARGO_PROFILE_TEST_OPT_LEVEL:-1}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_PROFILE_TEST_DEBUG="${CARGO_PROFILE_TEST_DEBUG:-0}"
export CARGO_PROFILE_DEV_STRIP="${CARGO_PROFILE_DEV_STRIP:-symbols}"
export CARGO_PROFILE_TEST_STRIP="${CARGO_PROFILE_TEST_STRIP:-symbols}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export OPENRAILSRS_DISABLE_AUDIO=1
# Clear session/visual overrides that leak into unit tests (view radius, camera, screenshots).
unset OPENRAILSRS_FOLLOW OPENRAILSRS_CAM_YAW OPENRAILSRS_CAM_PITCH OPENRAILSRS_CAM_DIST
unset OPENRAILSRS_VIEW_RADIUS_M OPENRAILSRS_VISIBLE_RADIUS_M
unset OPENRAILSRS_SCREENSHOT OPENRAILSRS_SCREENSHOT_DELAY_S OPENRAILSRS_SCREENSHOT_READY_FRAMES
unset OPENRAILSRS_SCREENSHOT_AFTER_READY OPENRAILSRS_WINDOW_WIDTH OPENRAILSRS_WINDOW_HEIGHT
unset OPENRAILSRS_SCREENSHOT_MIN_ODOMETER_M OPENRAILSRS_SCREENSHOT_PAUSE_AT_TARGET
unset OPENRAILSRS_SCREENSHOT_AFTER_SERVICE
unset OPENRAILSRS_FOG_VISIBILITY_M
unset OPENRAILSRS_WEATHER
unset OPENRAILSRS_WEATHER_EXECUTION OPENRAILSRS_WEATHER_PARTICLE_BUDGET OPENRAILSRS_RENDERER
unset OPENRAILSRS_TCS_SCRIPT OPENRAILSRS_TCS_HOST_DLL OPENRAILSRS_TCS_TYPE OPENRAILSRS_VISUAL_FAULT
unset OPENRAILSRS_CAPTURE_OR_FOCUS OPENRAILSRS_LOOK_YAW OPENRAILSRS_LOOK_PITCH

echo "==> rustfmt (cargo fmt --check)"
cargo fmt --all -- --check

echo "==> clippy (-D warnings)"
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings

echo "==> tests"
# Serial: several suites share process-global counters and environment variables.
# All focused regressions are already included here; keep one Bevy feature set.
cargo test --locked --workspace --all-features -- --test-threads=1

echo "==> native service capture integrity and replay regressions"
python3 -m unittest discover -s scripts -p test_service_capture.py
python3 -m unittest discover -s scripts -p test_scenery_oracle.py
python3 -m unittest discover -s scripts -p test_weather_execution.py
python3 -m unittest discover -s scripts -p test_native_pilot.py
python3 -m unittest discover -s scripts -p test_website.py
python3 -m unittest discover -s scripts -p test_visual_goldens.py
python3 scripts/build_website.py --check

if [[ -n "${OPENRAILSRS_NATIVE_ROUTE:-}" ]]; then
    echo "==> native platform geometry and continuous train motion"
    cargo test --locked --workspace --all-features native_service_ -- --ignored --nocapture --test-threads=1
fi

echo "==> build"
cargo build --locked --workspace --all-features
if [[ -n "${OPENRAILSRS_NATIVE_ROUTE:-}" ]]; then
    python3 scripts/run_scenery_oracle.py --route-root "$OPENRAILSRS_NATIVE_ROUTE"
fi

echo "==> pinned Open Rails acceptance oracles"
python3 scripts/run_oracles.py

echo "==> complete station service"
target/debug/openrailsrs play-service examples/chiltern_local/scenario.toml --out-dir tmp/service-check

echo "OK: check.sh completó sin errores."

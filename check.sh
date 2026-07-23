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
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "==> focused MSTS/Open Rails regressions"
# Serial: some suites share process-global counters (e.g. shape parse count).
cargo test -p openrailsrs-formats terrain -- --test-threads=1
cargo test -p openrailsrs-formats parse_compressed_binary_shape_from_open_rails_content -- --test-threads=1
cargo test -p openrailsrs-formats cvf -- --test-threads=1
cargo test -p openrailsrs-or-shader -- --test-threads=1
cargo test -p openrailsrs-bevy-scenery -- --test-threads=1
cargo test -p openrailsrs-viewer3d shapes -- --test-threads=1
cargo test -p openrailsrs-viewer3d camera -- --test-threads=1
cargo test -p openrailsrs-viewer3d cab_cvf -- --test-threads=1
cargo test -p openrailsrs-viewer3d cab_cvf_overlay -- --test-threads=1
cargo test -p openrailsrs-viewer3d pullman -- --test-threads=1
cargo test -p openrailsrs-viewer3d cab_view -- --test-threads=1
cargo test -p openrailsrs-viewer3d floating_origin -- --test-threads=1

echo "==> tests"
cargo test --workspace --all-features -- --test-threads=1

echo "==> build"
cargo build --workspace --all-features

echo "OK: check.sh completó sin errores."

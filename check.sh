#!/usr/bin/env bash
# Verificación local y en CI: formato, lints, tests y build del workspace.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

export OPENRAILSRS_DISABLE_AUDIO=1
unset OPENRAILSRS_FOLLOW OPENRAILSRS_CAM_YAW OPENRAILSRS_CAM_PITCH OPENRAILSRS_CAM_DIST

echo "==> rustfmt (cargo fmt --check)"
cargo fmt --all -- --check

echo "==> clippy (-D warnings)"
cargo clippy --workspace --all-targets --all-features -- -D warnings

echo "==> focused MSTS/Open Rails regressions"
cargo test -p openrailsrs-formats terrain
cargo test -p openrailsrs-formats parse_compressed_binary_shape_from_open_rails_content
cargo test -p openrailsrs-formats cvf
cargo test -p openrailsrs-or-shader
cargo test -p openrailsrs-bevy-scenery
cargo test -p openrailsrs-viewer3d shapes
cargo test -p openrailsrs-viewer3d camera
cargo test -p openrailsrs-viewer3d cab_cvf
cargo test -p openrailsrs-viewer3d cab_cvf_overlay
cargo test -p openrailsrs-viewer3d pullman
cargo test -p openrailsrs-viewer3d cab_view
cargo test -p openrailsrs-viewer3d floating_origin

echo "==> tests"
cargo test --workspace --all-features -- --test-threads=1

echo "==> build"
cargo build --workspace --all-features

echo "OK: check.sh completó sin errores."

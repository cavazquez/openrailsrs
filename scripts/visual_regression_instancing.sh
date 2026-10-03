#!/usr/bin/env bash
# Actual GPU draws: individual meshes and custom instances must share silhouettes.
# Run inside Xvfb on CI. Captures are sequential and need no original route assets.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_PROFILE_DEV_OPT_LEVEL="${CARGO_PROFILE_DEV_OPT_LEVEL:-1}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_PROFILE_DEV_STRIP="${CARGO_PROFILE_DEV_STRIP:-symbols}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export OPENRAILSRS_DISABLE_AUDIO=1
cargo build --locked --workspace --all-features
OUT="${OPENRAILSRS_INSTANCING_ORACLE_OUT:-tmp/instancing_oracle}"
mkdir -p "$OUT"
timeout --kill-after=5 60 target/debug/scenery_oracle "$OUT/individual.png" --individual
timeout --kill-after=5 60 target/debug/scenery_oracle "$OUT/instanced.png"
target/debug/scenery_oracle --verify "$OUT/individual.png" "$OUT/instanced.png"

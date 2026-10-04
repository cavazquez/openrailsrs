#!/usr/bin/env bash
# Play the complete local service using the original Chiltern scenery/trainset.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
ROUTE="${CHILTERN_ROUTE:-${OPENRAILSRS_MSTS_CONTENT:-$HOME/Documentos/Open Rails/Content}/Chiltern/ROUTES/Chiltern}"
SERVICE="${CHILTERN_SERVICE:-examples/chiltern_traffic/scenario.toml}"
if [[ ! -d "$ROUTE/WORLD" ]]; then
  echo "Chiltern no encontrado en $ROUTE; definir CHILTERN_ROUTE con la ruta del Content." >&2
  exit 1
fi
if [[ "${1:-}" == "--autodrive" ]]; then
  export OPENRAILSRS_AUTODRIVE=0.75
  shift
fi
if [[ "${1:-}" == "--cab" ]]; then
  export OPENRAILSRS_FOLLOW=driver
  shift
fi
export CARGO_PROFILE_DEV_OPT_LEVEL="${CARGO_PROFILE_DEV_OPT_LEVEL:-1}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_PROFILE_DEV_STRIP="${CARGO_PROFILE_DEV_STRIP:-symbols}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
cargo build --locked --workspace --all-features
if [[ "${1:-}" == "--direct" ]]; then
  shift
  exec target/debug/openrailsrs-viewer3d --live --route-root "$ROUTE" "$SERVICE" "$@"
fi
if [[ -n "${OPENRAILSRS_AUTODRIVE:-}" || -n "${OPENRAILSRS_FOLLOW:-}" ]]; then
  exec target/debug/openrailsrs-viewer3d --live --route-root "$ROUTE" "$SERVICE" "$@"
fi
exec target/debug/openrailsrs-viewer3d --menu --route-root "$ROUTE" "$@"

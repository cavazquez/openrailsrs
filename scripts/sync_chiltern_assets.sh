#!/usr/bin/env bash
# Sync RF_Blue_Pullman physics-only eng/wag into examples/chiltern.
# Optional: ./scripts/sync_chiltern_assets.sh --with-shapes [trainset_dir] [route_dir]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:-$HOME/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman}"
ROUTE="${2:-$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern}"
if [[ "${1:-}" == "--with-shapes" ]]; then
  python3 "$ROOT/scripts/sync_chiltern_assets.py" "${2:-$SRC}" --with-shapes --route-content "${3:-$ROUTE}"
else
  python3 "$ROOT/scripts/sync_chiltern_assets.py" "$SRC" ${2:+--route-content "$2"}
fi

#!/usr/bin/env bash
# Sync RF_Blue_Pullman physics-only eng/wag into examples/chiltern.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:-$HOME/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman}"
python3 "$ROOT/scripts/sync_chiltern_assets.py" "$SRC"

#!/usr/bin/env bash
# Experimento C — Open Rails (Explorer) para capturar baseline throttle 75 % / 60 s.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib/backup_or_speed_csv.sh
source "$SCRIPT_DIR/lib/backup_or_speed_csv.sh"

WINEPREFIX="${WINEPREFIX:-$HOME/wine64-OpenRails}"
export WINEPREFIX WINEARCH="${WINEARCH:-win64}"
export PATH="/usr/lib/x86_64-linux-gnu/wine:${PATH:-}"
export DISPLAY="${DISPLAY:-:0}"

RUNACT="$WINEPREFIX/drive_c/Program Files/Open Rails/RunActivity.exe"
PAT='C:\users\cristian\Documents\Open Rails\Content\Chiltern\ROUTES\Chiltern\PATHS\RS_Let'"'"'s go to Birmingham.pat'
CON='C:\users\cristian\Documents\Open Rails\Content\Chiltern\TRAINS\CONSISTS\Birmingham Pullman.con'

if [[ ! -f "$RUNACT" ]]; then
  echo "error: no se encontró RunActivity.exe en $RUNACT" >&2
  exit 1
fi

echo "=== Open Rails — Experimento C (Explorer, 75 % / 60 s) ==="
echo "WINEPREFIX=$WINEPREFIX"
echo "DISPLAY=$DISPLAY"
echo ""
echo "Options → Evaluation: train speed ON; Performance/Physics OFF."
echo ""
echo "En cabina:"
echo "  1. Pausa (P) al cargar"
echo "  2. Reverser adelante (W) si hace falta"
echo "  3. Freno suelto: ; hasta BRAKEPRESSURE -001"
echo "  4. Throttle 75 %: D hasta THROTTLEPERC ~075 (notch 6/8)"
echo "  5. Despausa y corré 60 s simulados"
echo "  6. Salí de OR"
echo ""
ROAM="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming"
echo "=== Respaldo automático de capturas OR previas ==="
backup_or_speed_csv "$ROAM" "Open Rails_explorerSpeed*.csv"
echo ""
echo "Luego:"
echo "  ./scripts/install_chiltern_throttle75_baseline.sh"
echo ""
read -r -p "Enter para lanzar OR Explorer (Ctrl+C cancela)…"

exec wine "$RUNACT" -start -explorer "$PAT" "$CON" 10:00 1 0

#!/usr/bin/env bash
# Experimento A — Open Rails (Explorer): acelerar → frenada fuerte → costa libre (180 s).
#
# NO uses RunActivity.exe -help  → no existe y puede crashear DXVK/Wine.
#
# Requisitos: WINEPREFIX con OR 1.6.1, content Chiltern, DISPLAY=:0

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

echo "=== Open Rails — Experimento A (frenada + costa libre, 180 s) ==="
echo "WINEPREFIX=$WINEPREFIX"
echo "DISPLAY=$DISPLAY"
echo ""
echo "Antes de arrancar, en OR → Options → Evaluation:"
echo "  ✓ Train speed logging ON"
echo "  ✓ Performance/Physics logging OFF (evita crash pdh.dll en Wine)"
echo ""
echo "=== Freno del Blue Pullman (importante) ==="
echo "El .eng define 5 DETENTES de freno de tren, no un eje continuo:"
echo "  1. Release (suelto)   2. Lap   3. Suppression"
echo "  4. Full service (~0.9) ← USÁ ESTE para el experimento"
echo "  5. Emergency (~0.95) ← la última posición del mouse a menudo NO frena más"
echo ""
echo "En tu captura anterior el freno SÍ funcionó con MOUSE (BRAKE→45), no con teclado."
echo "El replay OR no registró pulsaciones de freno por tecla — probable teclado Wine/layout."
echo ""
echo "Verificá teclas ANTES de correr:"
echo "  • En cabina: F1 → pestaña Key Commands → buscá Train Brake Increase/Decrease"
echo "  • O en menú OR: Options → Keyboard (defaults US: ' = aplicar, ; = soltar)"
echo "  • Teclado Linux latam + Wine: ';' y ''' suelen fallar. Remapeá a algo simple, p. ej.:"
echo "      Train Brake Increase  →  PageDown"
echo "      Train Brake Decrease  →  PageUp"
echo "  • NO confundas con Dynamic Brake (coma ,) ni Engine Brake ([ ])"
echo ""
echo "Con MOUSE (palanca TRAIN_BRAKE, abajo a la izquierda en cabina 2D):"
echo "  • Arrastrá hasta la ANTEÚLTIMA posición útil (full service), no al tope final"
echo "  • Mirá el HUD: BRAKEPRESSURE debe subir (p. ej. 030–045), no quedarse en 000"
echo ""
echo "Secuencia (alineada a driver_brake_coast.csv / openrailsrs):"
echo "  t=0–100 s   throttle 100 % (D), freno suelto"
echo "  t=100–105 s throttle 0 (A), freno full service (mouse o tecla Increase)"
echo "  t=105–180 s throttle 0, freno suelto (Decrease / palanca a release) — costa libre"
echo ""
echo "En cabina (Explorer):"
echo "  P — pausa al cargar"
echo "  W — reverser adelante"
echo "  Shift+/ — Initialize brakes (soltar todo, parado)"
echo "  D — subir throttle hasta ~100 %  →  este es t=0 (despausa y cronometrá con reloj OR)"
echo "  A — bajar throttle a 0 a los 100 s simulados"
echo "  Freno ON 100–105 s (PageDown repetido, o mouse a full service — NO emergency)"
echo "  Freno OFF a los 105 s (PageUp repetido, o palanca a release) y costear hasta ≥180 s"
echo ""
ROAM="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming"
echo "=== Respaldo automático de capturas OR previas ==="
backup_or_speed_csv "$ROAM" "Open Rails_explorerSpeed*.csv"
echo ""
echo "Luego instalá el baseline:"
echo "  ./scripts/install_chiltern_brake_coast_baseline.sh"
echo ""
if [[ "${NONINTERACTIVE:-}" != "1" ]]; then
  read -r -p "Enter para lanzar OR Explorer (Ctrl+C cancela)…"
fi

exec wine "$RUNACT" -start -explorer "$PAT" "$CON" 10:00 1 0

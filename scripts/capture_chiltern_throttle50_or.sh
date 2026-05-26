#!/usr/bin/env bash
# Experimento E — lanzar Open Rails (Explorer) para capturar baseline throttle 50 % / 30 s.
#
# NO uses RunActivity.exe -help  → no existe y puede crashear DXVK/Wine.
# Usá este script o el menú OpenRails.exe.
#
# Requisitos: WINEPREFIX con OR 1.6.1, content Chiltern, DISPLAY=:0

set -euo pipefail

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

echo "=== Open Rails — Experimento E (Explorer) ==="
echo "WINEPREFIX=$WINEPREFIX"
echo "DISPLAY=$DISPLAY"
echo ""
echo "Antes de arrancar, en OR → Options → Evaluation:"
echo "  ✓ Train speed logging ON"
echo "  ✓ Performance/Physics logging OFF (evita crash pdh.dll en Wine)"
echo ""
echo "En cabina (modo Explorer) — teclas OR/MSTS por defecto (verificá con F1):"
echo "  W/S = reverser adelante/atrás   D/A = subir/bajar throttle"
echo "  Freno de tren: ' (apóstrofo) aplicar, ; (punto y coma) soltar"
echo "  (La coma , es freno dinámico, NO el freno de tren.)"
echo ""
echo "  1. Pausa (P) inmediatamente al cargar"
echo "  2. Reverser adelante (W) si hace falta"
echo "  3. Freno de tren completamente suelto: ; repetido hasta BRAKEPRESSURE -001"
echo "     (parado: Shift+/ = Initialize brakes = soltar todo de golpe)"
echo "  4. Throttle 50 %: D repetida hasta THROTTLEPERC ~050 en el HUD"
echo "     (Pullman 8 notches → suele ser notch 4; NO te quedes en ~012)"
echo "  5. Despausa y dejá correr 30 s de tiempo simulado"
echo "  6. Salí de OR (Alt+F4)"
echo ""
SRC_CSV="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_explorerSpeed.csv"
if [[ -f "$SRC_CSV" ]]; then
  echo "Aviso: ya existe $SRC_CSV"
  echo "  Si es una captura vieja, renombrala antes de correr OR:"
  echo "  mv \"$SRC_CSV\" \"${SRC_CSV}.bak.$(date +%Y%m%d%H%M%S)\""
  echo ""
fi
echo "Luego instalá el baseline:"
echo "  ./scripts/install_chiltern_throttle50_baseline.sh"
echo ""
read -r -p "Enter para lanzar OR Explorer (Ctrl+C cancela)…"

exec wine "$RUNACT" -start -explorer "$PAT" "$CON" 10:00 1 0

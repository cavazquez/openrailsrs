#!/usr/bin/env bash
# Capturar baseline OR largo — Chiltern / RS_Let's go to Birmingham (AUTO_SIGNAL).
#
# Genera: %APPDATA%/Open Rails_RS_Let's go to BirminghamSpeed.csv
# Instalá después con: ./scripts/install_chiltern_birmingham_baseline.sh
#
# NO uses RunActivity.exe -help  → no existe y puede crashear DXVK/Wine.

set -euo pipefail

WINEPREFIX="${WINEPREFIX:-$HOME/wine64-OpenRails}"
export WINEPREFIX WINEARCH="${WINEARCH:-win64}"
export PATH="/usr/lib/x86_64-linux-gnu/wine:${PATH:-}"
export DISPLAY="${DISPLAY:-:0}"

for candidate in \
  "$WINEPREFIX/drive_c/Program Files/Open Rails/RunActivity.exe" \
  "$WINEPREFIX/drive_c/Program Files (x86)/Open Rails/RunActivity.exe"
do
  if [[ -f "$candidate" ]]; then
    RUNACT="$candidate"
    break
  fi
done

if [[ -z "${RUNACT:-}" ]]; then
  echo "error: no se encontró RunActivity.exe en el WINEPREFIX" >&2
  exit 1
fi

ACT='C:\users\cristian\Documents\Open Rails\Content\Chiltern\ROUTES\Chiltern\ACTIVITIES\RS_Let'"'"'s go to Birmingham.act'
SRC_CSV="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_RS_Let's go to BirminghamSpeed.csv"
TARGET_S="${CAPTURE_DURATION_S:-120}"

echo "=== Open Rails — Birmingham baseline (actividad AUTO_SIGNAL) ==="
echo "WINEPREFIX=$WINEPREFIX"
echo "DISPLAY=$DISPLAY"
echo "Duración objetivo: ${TARGET_S} s de tiempo simulado (mínimo recomendado; podés correr más)."
echo ""
echo "Antes de arrancar, en OR → Options → Evaluation:"
echo "  ✓ Train speed logging ON"
echo "  ✓ Performance/Physics logging OFF (evita crash pdh.dll en Wine)"
echo ""
echo "En cabina — mismos controles que el baseline corto (~80 % throttle):"
echo "  W/S = reverser adelante/atrás   D/A = subir/bajar throttle"
echo "  Freno de tren: ' (aplicar), ; (soltar)   (, = freno dinámico, no tren)"
echo ""
echo "Flujo sugerido:"
echo "  1. Al cargar la actividad, soltá freno de tren (; hasta BRAKEPRESSURE -001)"
echo "  2. Reverser adelante (W) si hace falta"
echo "  3. Throttle ~80 % (D hasta THROTTLEPERC ~080 en el HUD)"
echo "  4. Dejá correr ≥ ${TARGET_S} s (ideal 120–180 s) sin salir de OR"
echo "  5. Alt+F4 para cerrar OR (el CSV se escribe al salir)"
echo ""
if [[ -f "$SRC_CSV" ]]; then
  echo "Aviso: ya existe $SRC_CSV"
  echo "  Renombralo antes de capturar para no mezclar sesiones:"
  echo "  mv \"$SRC_CSV\" \"${SRC_CSV}.bak.\$(date +%Y%m%d%H%M%S)\""
  echo ""
fi
echo "Después instalá en el repo:"
echo "  ./scripts/install_chiltern_birmingham_baseline.sh"
echo ""
read -r -p "Enter para lanzar OR (Ctrl+C cancela)…"

exec wine "$RUNACT" -start -activity "$ACT"

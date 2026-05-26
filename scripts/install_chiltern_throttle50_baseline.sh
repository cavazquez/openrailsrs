#!/usr/bin/env bash
# Copia el *Speed.csv de OR (Explorer) al baseline versionado del Experimento E.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WINEPREFIX="${WINEPREFIX:-$HOME/wine64-OpenRails}"
SRC="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_explorerSpeed.csv"
DEST="$REPO_ROOT/examples/baselines/chiltern_throttle50/or_evaluation_speed.csv"
MIN_THR="${MIN_THROTTLE_AVG:-45}"
MAX_THR="${MAX_THROTTLE_AVG:-55}"
WINDOW_S="${BASELINE_WINDOW_S:-30}"

if [[ ! -f "$SRC" ]]; then
  echo "error: no existe $SRC" >&2
  echo "Corré primero: ./scripts/capture_chiltern_throttle50_or.sh" >&2
  exit 1
fi

python3 - "$SRC" "$DEST" "$MIN_THR" "$MAX_THR" "$WINDOW_S" <<'PY'
import sys
from datetime import datetime, timedelta

src, dest, min_thr, max_thr, window_s = sys.argv[1:6]
min_thr = int(min_thr)
max_thr = int(max_thr)
window_s = int(window_s)

lines = [l.strip() for l in open(src, encoding="utf-8", errors="replace") if l.strip()]
if len(lines) < 10:
    sys.exit("CSV demasiado corto")
header = lines[0]
rows = []
for line in lines[1:]:
    parts = line.split(",")
    if "EXPLORER" not in parts:
        continue
    i = parts.index("EXPLORER")
    if i + 2 >= len(parts):
        continue
    try:
        t = datetime.strptime(parts[0], "%H:%M:%S")
        throttle = int(parts[i + 2])
        brake = int(parts[i + 3]) if i + 3 < len(parts) and parts[i + 3].lstrip("-").isdigit() else 999
    except ValueError:
        continue
    rows.append((t, line, throttle, brake))

if len(rows) < 5:
    sys.exit("no se encontraron filas EXPLORER con THROTTLEPERC")

steady = [(t, line, thr, brk) for t, line, thr, brk in rows if thr >= 48]
if len(steady) < window_s:
    sys.exit(f"muy pocas filas con throttle >= 48 % ({len(steady)}); recapturá con D hasta ~050")

avg_steady = sum(r[2] for r in steady) / len(steady)
max_thr_val = max(r[2] for r in rows)
print(f"THROTTLEPERC en régimen (>=48 %): avg={avg_steady:.0f} min={min(r[2] for r in steady)} max={max(r[2] for r in steady)} n={len(steady)}", file=sys.stderr)
print(f"THROTTLEPERC global: min={min(r[2] for r in rows)} max={max_thr_val} n={len(rows)}", file=sys.stderr)

if avg_steady < min_thr or avg_steady > max_thr:
    sys.exit(
        f"error: régimen ~{avg_steady:.0f} % fuera de {min_thr}–{max_thr} %.\n"
        "  → Pausá (P), D hasta ~050, freno suelto con ;, despausa y corré 30 s."
    )

# Ventana del experimento: primer instante con throttle ~50 % y freno casi suelto.
start_idx = next(
    (i for i, (_, _, thr, brk) in enumerate(rows) if thr >= 48 and brk == 0),
    None,
)
if start_idx is None:
    start_idx = next(
        (i for i, (_, _, thr, brk) in enumerate(rows) if thr >= 48 and brk <= 1),
        None,
    )
if start_idx is None:
    start_idx = next(i for i, (_, _, thr, _) in enumerate(rows) if thr >= 48)
start_t = rows[start_idx][0]
end_t = start_t + timedelta(seconds=window_s)
window = [(t, line) for t, line, _, _ in rows if start_t <= t <= end_t]
if len(window) < window_s // 2:
    sys.exit(f"ventana recortada demasiado corta ({len(window)} filas)")

out = [header]
base = window[0][0]
for t, line in window:
    parts = line.split(",")
    parts[0] = (base + (t - base)).strftime("%H:%M:%S")
    out.append(",".join(parts))

open(dest, "w", encoding="utf-8").write("\n".join(out) + "\n")
print(f"Instalado: {dest} ({len(window)} filas, t=0 en {start_t.strftime('%H:%M:%S')} OR original)")
PY

echo ""
echo "Comparar:"
echo "  cd examples/chiltern"
echo "  openrailsrs sim scenario_throttle50.toml --driver driver_throttle50.csv"
echo "  openrailsrs compare-or ../baselines/chiltern_throttle50/or_evaluation_speed.csv run_throttle50.csv"
echo "  cargo test -p openrailsrs-cli --test chiltern_throttle50"

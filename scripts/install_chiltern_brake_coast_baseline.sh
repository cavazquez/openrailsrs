#!/usr/bin/env bash
# Copia el *Speed.csv de OR (Explorer) al baseline versionado del Experimento A.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WINEPREFIX="${WINEPREFIX:-$HOME/wine64-OpenRails}"
ROAM="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming"
DEST="$REPO_ROOT/examples/baselines/chiltern_brake_coast/or_evaluation_speed.csv"
WINDOW_S="${BASELINE_WINDOW_S:-180}"
ACCEL_END_S="${ACCEL_END_S:-100}"
BRAKE_END_S="${BRAKE_END_S:-105}"
MIN_THR_START="${MIN_THROTTLE_START:-95}"
BRAKE_RELEASED_MAX="${BRAKE_RELEASED_MAX:-0}"
MIN_BRAKE_PEAK="${MIN_BRAKE_PEAK:-20}"

SRC="${OR_EXPLORER_SPEED_CSV:-}"
if [[ -z "$SRC" ]]; then
  mapfile -t _candidates < <(find "$ROAM" -maxdepth 1 -name 'Open Rails_explorerSpeed*.csv' ! -name '*.bak*' -printf '%T@ %p\n' 2>/dev/null | sort -rn | cut -d' ' -f2-)
  if ((${#_candidates[@]} == 0)); then
    SRC="$ROAM/Open Rails_explorerSpeed.csv"
  else
    SRC="${_candidates[0]}"
    if ((${#_candidates[@]} > 1)); then
      echo "Aviso: varios explorerSpeed.csv; usando el más reciente:" >&2
      printf '  %s\n' "${_candidates[@]}" >&2
    fi
  fi
fi

if [[ ! -f "$SRC" ]]; then
  echo "error: no existe $SRC" >&2
  echo "Corré primero: ./scripts/capture_chiltern_brake_coast_or.sh" >&2
  exit 1
fi
echo "Fuente OR: $SRC" >&2

python3 - "$SRC" "$DEST" "$WINDOW_S" "$ACCEL_END_S" "$BRAKE_END_S" "$MIN_THR_START" "$BRAKE_RELEASED_MAX" "$MIN_BRAKE_PEAK" <<'PY'
import sys
from datetime import datetime, timedelta

(
    src,
    dest,
    window_s,
    accel_end_s,
    brake_end_s,
    min_thr_start,
    brake_released_max,
    min_brake_peak,
) = sys.argv[1:9]
window_s = int(window_s)
accel_end_s = int(accel_end_s)
brake_end_s = int(brake_end_s)
min_thr_start = int(min_thr_start)
brake_released_max = int(brake_released_max)
min_brake_peak = int(min_brake_peak)

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

if len(rows) < window_s // 2:
    sys.exit(f"muy pocas filas EXPLORER ({len(rows)})")

try:
    start_idx = next(
        i
        for i, (_, _, thr, brk) in enumerate(rows)
        if thr >= min_thr_start and brk <= brake_released_max
    )
except StopIteration:
    sys.exit(
        f"no hay filas con throttle>={min_thr_start} % y BRAKEPRESSURE<={brake_released_max}; "
        "empezá con freno suelto y throttle pleno (t=0 del experimento)"
    )

start_t = rows[start_idx][0]
need_until = start_t + timedelta(seconds=window_s)
end_t = need_until
window = [(t, line, thr, brk) for t, line, thr, brk in rows if start_t <= t <= end_t]
if len(window) < window_s // 2:
    sys.exit(f"ventana recortada demasiado corta ({len(window)} filas; necesitás ~{window_s} s)")

installed_dur = (window[-1][0] - window[0][0]).total_seconds()
if installed_dur + 0.5 < window_s:
    sys.exit(
        f"error: ventana disponible {installed_dur:.0f} s < {window_s} s.\n"
        f"  Dejá correr hasta ≥ {need_until.strftime('%H:%M:%S')} en reloj OR."
    )

# Comprobar frenada en fase 100–105 s (relativa a t=0).
brake_phase = [
    brk
    for t, _, thr, brk in window
    if accel_end_s <= (t - start_t).total_seconds() < brake_end_s
]
if not brake_phase:
    sys.exit("no hay muestras en fase de frenada (100–105 s)")
max_brake = max(brake_phase)
if max_brake < min_brake_peak:
    sys.exit(
        f"error: BRAKEPRESSURE máximo en frenada = {max_brake} (< {min_brake_peak}).\n"
        "  Aplicá freno pleno con ' durante ~5 s en t=100–105."
    )

coast_phase = [
    (t - start_t).total_seconds()
    for t, _, thr, brk in window
    if (t - start_t).total_seconds() >= brake_end_s and thr <= 5 and brk <= brake_released_max
]
if len(coast_phase) < (window_s - brake_end_s) // 2:
    sys.exit(
        "muy pocas filas en costa libre (throttle bajo, freno suelto tras t=105); "
        "soltá freno con ; y no toques throttle"
    )

print(
    f"t=0 OR original: {start_t.strftime('%H:%M:%S')} "
    f"(throttle={rows[start_idx][2]} %, brake={rows[start_idx][3]})",
    file=sys.stderr,
)
print(
    f"Frenada: BRAKEPRESSURE max={max_brake} en [{accel_end_s},{brake_end_s}) s; "
    f"costa libre: {len(coast_phase)} muestras",
    file=sys.stderr,
)

out = [header]
base = window[0][0]
for t, line, _, _ in window:
    parts = line.split(",")
    parts[0] = (base + (t - base)).strftime("%H:%M:%S")
    out.append(",".join(parts))

open(dest, "w", encoding="utf-8").write("\n".join(out) + "\n")
print(f"Instalado: {dest} ({len(window)} filas, {installed_dur:.0f} s)")
PY

echo ""
echo "Comparar:"
echo "  cd examples/chiltern"
echo "  openrailsrs sim scenario_brake_coast.toml --driver driver_brake_coast.csv"
echo "  openrailsrs compare-or ../baselines/chiltern_brake_coast/or_evaluation_speed.csv run_brake_coast.csv"
echo "  cargo test -p openrailsrs-cli --test chiltern_brake_coast"

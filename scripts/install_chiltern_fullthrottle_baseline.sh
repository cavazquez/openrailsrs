#!/usr/bin/env bash
# Copia el *Speed.csv de OR (Explorer) al baseline versionado del Experimento B.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WINEPREFIX="${WINEPREFIX:-$HOME/wine64-OpenRails}"
ROAM="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming"
DEST="$REPO_ROOT/examples/baselines/chiltern_fullthrottle/or_evaluation_speed.csv"
MIN_THR="${MIN_THROTTLE_AVG:-95}"
MAX_THR="${MAX_THROTTLE_AVG:-100}"
WINDOW_S="${BASELINE_WINDOW_S:-120}"
STEADY_MIN_THR="${STEADY_MIN_THR:-95}"
BRAKE_RELEASED_MAX="${BRAKE_RELEASED_MAX:-0}"

# OR rota: Open Rails_explorerSpeed.csv, _01.csv, _02.csv, … — tomar el más reciente.
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
  echo "Corré primero: ./scripts/capture_chiltern_fullthrottle_or.sh" >&2
  echo "(Roaming limpio — OR creará Open Rails_explorerSpeed.csv al salir)" >&2
  exit 1
fi
echo "Fuente OR: $SRC" >&2

python3 - "$SRC" "$DEST" "$MIN_THR" "$MAX_THR" "$WINDOW_S" "$STEADY_MIN_THR" "$BRAKE_RELEASED_MAX" <<'PY'
import sys
from datetime import datetime, timedelta

src, dest, min_thr, max_thr, window_s, steady_min, brake_max = sys.argv[1:8]
min_thr = int(min_thr)
max_thr = int(max_thr)
window_s = int(window_s)
steady_min = int(steady_min)
brake_max = int(brake_max)

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

steady = [(t, line, thr, brk) for t, line, thr, brk in rows if thr >= steady_min]
if len(steady) < window_s // 2:
    sys.exit(
        f"muy pocas filas con throttle >= {steady_min} % ({len(steady)}); "
        "recapturá con D hasta ~100"
    )

avg_steady = sum(r[2] for r in steady) / len(steady)
max_thr_val = max(r[2] for r in rows)
print(
    f"THROTTLEPERC en régimen (>={steady_min} %): avg={avg_steady:.0f} "
    f"min={min(r[2] for r in steady)} max={max(r[2] for r in steady)} n={len(steady)}",
    file=sys.stderr,
)
print(
    f"THROTTLEPERC global: min={min(r[2] for r in rows)} max={max_thr_val} n={len(rows)}",
    file=sys.stderr,
)

if avg_steady < min_thr or avg_steady > max_thr:
    sys.exit(
        f"error: régimen ~{avg_steady:.0f} % fuera de {min_thr}–{max_thr} %.\n"
        "  → Pausá (P), D hasta ~100, freno suelto con ;, despausa y corré 120 s."
    )

csv_first, csv_last = rows[0][0], rows[-1][0]
csv_span = (csv_last - csv_first).total_seconds()
steady_span = (steady[-1][0] - steady[0][0]).total_seconds()
print(
    f"Tiempo sim en CSV: {csv_first.strftime('%H:%M:%S')} → {csv_last.strftime('%H:%M:%S')} "
    f"({csv_span:.0f} s, {len(rows)} muestras ≈ 1 Hz)",
    file=sys.stderr,
)
print(
    f"Régimen throttle>={steady_min}%: {steady[0][0].strftime('%H:%M:%S')} → "
    f"{steady[-1][0].strftime('%H:%M:%S')} ({steady_span:.0f} s)",
    file=sys.stderr,
)

# t=0 = throttle pleno y freno ya soltado (openrailsrs arranca con brake=0).
try:
    start_idx = next(
        i
        for i, (_, _, thr, brk) in enumerate(rows)
        if thr >= steady_min and brk <= brake_max
    )
except StopIteration:
    sys.exit(
        f"no hay filas con throttle>={steady_min} % y BRAKEPRESSURE<={brake_max}; "
        "esperá a que el freno OR baje a 0 antes de salir"
    )
start_t = rows[start_idx][0]
if rows[start_idx][3] > 0:
    print(
        f"t=0 en {start_t.strftime('%H:%M:%S')} OR (throttle={rows[start_idx][2]} %, "
        f"brake={rows[start_idx][3]})",
        file=sys.stderr,
    )
need_until = start_t + timedelta(seconds=window_s)
end_t = need_until
window = [(t, line) for t, line, _, _ in rows if start_t <= t <= end_t]
if len(window) < window_s // 2:
    sys.exit(f"ventana recortada demasiado corta ({len(window)} filas; necesitás ~{window_s} s)")

installed_dur = (window[-1][0] - window[0][0]).total_seconds()
if installed_dur + 0.5 < window_s:
    sys.exit(
        f"error: ventana disponible {installed_dur:.0f} s < {window_s} s pedidos.\n"
        f"  CSV termina a las {csv_last.strftime('%H:%M:%S')}; hace falta ≥ {need_until.strftime('%H:%M:%S')} "
        f"(reloj OR en pantalla, no cronómetro del sistema).\n"
        f"  OR registra ~1 fila/s solo con sim despausada: {csv_span:.0f} s en archivo = "
        f"{csv_span:.0f} s simulados.\n"
        f"  → Tras soltar freno y D al 100 %, dejá correr hasta que el reloj OR pase "
        f"{need_until.strftime('%H:%M:%S')} (~{window_s + 5} s desde t=0)."
    )

out = [header]
base = window[0][0]
for t, line in window:
    parts = line.split(",")
    parts[0] = (base + (t - base)).strftime("%H:%M:%S")
    out.append(",".join(parts))

open(dest, "w", encoding="utf-8").write("\n".join(out) + "\n")
print(f"Instalado: {dest} ({len(window)} filas, {installed_dur:.0f} s, t=0 en {start_t.strftime('%H:%M:%S')} OR original)")
PY

echo ""
echo "Comparar:"
echo "  cd examples/chiltern"
echo "  openrailsrs sim scenario_throttle100.toml --driver driver_throttle100.csv"
echo "  openrailsrs compare-or ../baselines/chiltern_fullthrottle/or_evaluation_speed.csv run_throttle100.csv"
echo "  cargo test -p openrailsrs-cli --test chiltern_fullthrottle"

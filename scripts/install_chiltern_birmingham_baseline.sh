#!/usr/bin/env bash
# Valida e instala el *Speed.csv de OR (actividad Birmingham) en el baseline versionado.
#
# Acepta el CSV en Wine (tras capturar) o ya copiado en examples/…/or_evaluation_speed.csv.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WINEPREFIX="${WINEPREFIX:-$HOME/wine64-OpenRails}"
SRC="$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_RS_Let's go to BirminghamSpeed.csv"
DEST="$REPO_ROOT/examples/baselines/chiltern_birmingham/or_evaluation_speed.csv"
MIN_DURATION_S="${MIN_DURATION_S:-120}"
MIN_THR="${MIN_THROTTLE_AVG:-70}"
MAX_THR="${MAX_THROTTLE_AVG:-90}"

INPUT="${BASELINE_SRC:-}"
if [[ -z "$INPUT" ]]; then
  if [[ -f "$SRC" ]]; then
    INPUT="$SRC"
  elif [[ -f "$DEST" ]]; then
    INPUT="$DEST"
    echo "Usando baseline ya copiado: $DEST" >&2
  else
    echo "error: no existe $SRC ni $DEST" >&2
    echo "Corré: ./scripts/capture_chiltern_birmingham_or.sh" >&2
    echo "  o copiá el CSV a $DEST" >&2
    exit 1
  fi
fi

python3 - "$INPUT" "$DEST" "$MIN_DURATION_S" "$MIN_THR" "$MAX_THR" <<'PY'
import sys
from datetime import datetime

src, dest, min_duration_s, min_thr, max_thr = sys.argv[1:6]
min_duration_s = int(min_duration_s)
min_thr = int(min_thr)
max_thr = int(max_thr)


def parse_or_int(token: str) -> int:
    token = token.strip()
    if not token:
        return 0
    if token.lstrip("-").isdigit():
        return int(token)
    # OR a veces escribe distancias en notación científica en otras columnas; ignorar.
    return 0


def parse_eval_row(parts: list[str]) -> tuple[datetime, int, int] | None:
    if "AUTO_SIGNAL" not in parts:
        return None
    try:
        t = datetime.strptime(parts[0], "%H:%M:%S")
    except ValueError:
        return None
    try:
        anchor = parts.index("-001")
    except ValueError:
        return None
    if anchor < 2:
        return None
    throttle = parse_or_int(parts[anchor - 2])
    brake = parse_or_int(parts[anchor - 1])
    return t, throttle, brake


lines = [l.strip() for l in open(src, encoding="utf-8", errors="replace") if l.strip()]
if len(lines) < 10:
    sys.exit("CSV demasiado corto")
header = lines[0].upper()
if "TIME" not in header or "TRAINSPEED" not in header:
    sys.exit("header inesperado; ¿Train speed logging activo en Evaluation?")

rows: list[tuple[datetime, str, int, int]] = []
for line in lines[1:]:
    parts = line.split(",")
    parsed = parse_eval_row(parts)
    if parsed is None:
        continue
    t, throttle, brake = parsed
    rows.append((t, line, throttle, brake))

if len(rows) < min_duration_s // 2:
    sys.exit(f"muy pocas filas AUTO_SIGNAL ({len(rows)}); ¿corriste la actividad completa?")

t0 = rows[0][0]
t_last = rows[-1][0]
span = (t_last - t0).total_seconds()
if span < min_duration_s - 5:
    sys.exit(
        f"error: duración sim {span:.0f} s < mínimo {min_duration_s} s.\n"
        "  → En OR mirá el reloj del juego (no el reloj de pared): dejá correr más tiempo sim.\n"
        f"  → Tu CSV va de {t0.strftime('%H:%M:%S')} a {t_last.strftime('%H:%M:%S')}.\n"
        f"  → Para esta captura probá: MIN_DURATION_S={max(60, int(span) - 10)} ./scripts/install_chiltern_birmingham_baseline.sh"
    )

steady = [
    (t, thr)
    for t, _, thr, brk in rows
    if (t - t0).total_seconds() >= 30 and brk <= 1 and thr >= 60
]
if len(steady) < 20:
    sys.exit("muy pocas filas en régimen (t>=30 s, freno suelto, throttle>=60 %)")

avg_thr = sum(r[1] for r in steady) / len(steady)
print(
    f"Duración sim: {span:.0f} s ({t0.strftime('%H:%M:%S')} → {t_last.strftime('%H:%M:%S')}), "
    f"filas={len(rows)}",
    file=sys.stderr,
)
print(
    f"Throttle régimen (t>=30 s): avg={avg_thr:.0f} % n={len(steady)}",
    file=sys.stderr,
)

if avg_thr < min_thr or avg_thr > max_thr:
    sys.exit(
        f"error: throttle medio ~{avg_thr:.0f} % fuera de {min_thr}–{max_thr} %.\n"
        "  → Ajustá a ~80 % con D antes de despausar / salir de la estación."
    )

open(dest, "w", encoding="utf-8").write("\n".join(lines) + "\n")
print(f"Instalado: {dest} ({len(lines) - 1} filas de datos, {span:.0f} s sim)")
PY

echo ""
echo "Regenerar driver y validar:"
echo "  cd examples/chiltern"
echo "  openrailsrs or-eval-driver ../baselines/chiltern_birmingham/or_evaluation_speed.csv \\"
echo "    --out driver_or.csv --brake-full-scale 27"
echo "  # duration en scenario.toml ≈ duración sim del CSV (p. ej. 136)"
echo "  openrailsrs sim scenario.toml --driver driver_or.csv"
echo "  cargo test -p openrailsrs-cli --test chiltern_validate"

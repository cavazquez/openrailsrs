#!/usr/bin/env bash
# Benchmark OR vs openrailsrs with fixed checkpoints/phases.
#
# Uso rápido (desde el repo):
#   ./scripts/benchmark_or_checkpoints.sh
#
# Overrides opcionales:
#   BASELINE=examples/baselines/chiltern_birmingham/or_evaluation_speed.csv \
#   RUN_CSV=examples/chiltern/run.csv \
#   CHECKPOINTS=10,30,60 \
#   PHASE_BOUNDS=0,20,65 \
#   STEP_S=0.1 \
#   ./scripts/benchmark_or_checkpoints.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE="${BASELINE:-examples/baselines/chiltern_birmingham/or_evaluation_speed.csv}"
RUN_CSV="${RUN_CSV:-examples/chiltern/run.csv}"
CHECKPOINTS="${CHECKPOINTS:-10,30,60}"
PHASE_BOUNDS="${PHASE_BOUNDS:-0,20,65}"
STEP_S="${STEP_S:-0.1}"

cd "$REPO_ROOT"

if [[ ! -f "$BASELINE" ]]; then
  echo "error: baseline OR no encontrado: $BASELINE" >&2
  exit 1
fi
if [[ ! -f "$RUN_CSV" ]]; then
  echo "error: run.csv no encontrado: $RUN_CSV" >&2
  exit 1
fi

echo "=== Benchmark OR checkpoints ==="
echo "baseline:    $BASELINE"
echo "run csv:     $RUN_CSV"
echo "step_s:      $STEP_S"
echo "checkpoints: $CHECKPOINTS"
echo "phases:      $PHASE_BOUNDS"
echo ""

cargo run -p openrailsrs-cli -- compare-or \
  "$BASELINE" \
  "$RUN_CSV" \
  --step "$STEP_S" \
  --checkpoints "$CHECKPOINTS" \
  --phase-bounds "$PHASE_BOUNDS"

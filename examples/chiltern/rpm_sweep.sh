#!/usr/bin/env bash
# Barrido ChangeUpRPMpS × RateOfChangeUpRPMpSS (DMBSA) — fase 0–40 s vs OR.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CHILTERN="$ROOT/examples/chiltern"
ENG="$CHILTERN/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng"
BASELINE="$CHILTERN/../baselines/chiltern_birmingham/or_evaluation_speed.csv"
OUT="$CHILTERN/rpm_sweep_results.csv"
SCENARIO="$CHILTERN/scenario_rpm_sweep.toml"

cp "$CHILTERN/scenario.toml" "$SCENARIO"
sed -i 's/^duration = .*/duration = 40.0/' "$SCENARIO"
sed -i 's/^csv = .*/csv = "run_rpm_sweep.csv"/' "$SCENARIO"
sed -i 's/^metadata = .*/metadata = "run_rpm_sweep.json"/' "$SCENARIO"

BACKUP="$(mktemp)"
cp "$ENG" "$BACKUP"
restore() { cp "$BACKUP" "$ENG"; rm -f "$BACKUP" "$SCENARIO" "$CHILTERN/run_rpm_sweep.csv" "$CHILTERN/run_rpm_sweep.json"; }
trap restore EXIT

echo "change_up_rpm_ps,rate_up_rpm_pss,vel_rms_0_40,vel_max_0_40,pos_max_0_40" >"$OUT"

cd "$CHILTERN"
for cup in 30 40 50 60 70; do
  for rate in 5 8 10 13 16; do
    sed -i "s/( ChangeUpRPMpS [0-9.]* )/( ChangeUpRPMpS $cup )/" "$ENG"
    sed -i "s/( RateOfChangeUpRPMpSS [0-9.]* )/( RateOfChangeUpRPMpSS $rate )/" "$ENG"
    cargo run -q -p openrailsrs-cli -- sim "$SCENARIO" --driver driver_or.csv --no-validate >/dev/null
    phase="$(
      cargo run -q -p openrailsrs-cli -- compare-or "$BASELINE" run_rpm_sweep.csv --phase-bounds 0,40 2>&1 \
        | grep '^\s*\[0–40' || true
    )"
    vel_rms="$(echo "$phase" | sed -n 's/.*velocity rms=\([0-9.]*\).*/\1/p')"
    vel_max="$(echo "$phase" | sed -n 's/.*max=\([0-9.]*\).*/\1/p')"
    pos_max="$(echo "$phase" | sed -n 's/.*position rms=[0-9.]* max=\([0-9.]*\).*/\1/p')"
    echo "$cup,$rate,$vel_rms,$vel_max,$pos_max" | tee -a "$OUT"
  done
done

echo ""
echo "=== Mejor vel RMS 0–40 s ==="
sort -t, -k3 -n "$OUT" | head -6

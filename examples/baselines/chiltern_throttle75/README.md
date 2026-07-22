# Baseline Chiltern — throttle 75 % (60 s)

Notch fijo (~notch 6 Pullman). Paridad: [`docs/OR_PARITY.md`](../../../docs/OR_PARITY.md).

```bash
./scripts/capture_chiltern_throttle75_or.sh
./scripts/install_chiltern_throttle75_baseline.sh

cd examples/chiltern
openrailsrs sim scenario_throttle75.toml --driver driver_throttle75.csv
openrailsrs compare-or ../baselines/chiltern_throttle75/or_evaluation_speed.csv run_throttle75.csv --phase-bounds 0,20,60
cargo test -p openrailsrs-cli --test chiltern_throttle75
```

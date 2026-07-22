# Baseline Chiltern — throttle 50 % (30 s)

Calibración motor/RPM a notch fijo. Paridad: [`docs/OR_PARITY.md`](../../../docs/OR_PARITY.md). No uses `RunActivity.exe -help` (rompe Wine).

```bash
./scripts/capture_chiltern_throttle50_or.sh   # Evaluation ON; Data Logger OFF
# Cabina: W + freno suelto + D hasta THROTTLEPERC ~050; 30 s sim
./scripts/install_chiltern_throttle50_baseline.sh

cd examples/chiltern
openrailsrs sim scenario_throttle50.toml --driver driver_throttle50.csv
openrailsrs compare-or ../baselines/chiltern_throttle50/or_evaluation_speed.csv run_throttle50.csv
cargo test -p openrailsrs-cli --test chiltern_throttle50
```

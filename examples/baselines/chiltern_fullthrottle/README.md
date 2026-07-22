# Baseline Chiltern — throttle 100 % (120 s)

Aceleración plena desde parado (Pullman 8 coches en OR). Paridad: [`docs/OR_PARITY.md`](../../../docs/OR_PARITY.md).

```bash
./scripts/capture_chiltern_fullthrottle_or.sh
# Cabina: W, freno suelto (;), D → 100 %; ~120 s sim
./scripts/install_chiltern_fullthrottle_baseline.sh

cd examples/chiltern
openrailsrs sim scenario_throttle100.toml --driver driver_throttle100.csv
openrailsrs compare-or ../baselines/chiltern_fullthrottle/or_evaluation_speed.csv run_throttle100.csv
cargo test -p openrailsrs-cli --test chiltern_fullthrottle

openrailsrs sim scenario_throttle100_multi_body.toml --driver driver_throttle100.csv
cargo test -p openrailsrs-cli --test chiltern_fullthrottle_multi_body
```

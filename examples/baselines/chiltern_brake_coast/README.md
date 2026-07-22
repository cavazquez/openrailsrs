# Baseline Chiltern — frenada + costa (180 s)

OR-P6: throttle pleno → freno 5 s → costa libre. Criterio: fase **105–180 s** con velocity RMS ≤ **0.5 m/s** vs OR.

Driver (`driver_brake_coast.csv`): 0–100 s throttle 1; 100–105 s brake 1; 105–180 s idle. Modelo vs OR: [`docs/OR_PARITY.md`](../../../docs/OR_PARITY.md).

```bash
./scripts/capture_chiltern_brake_coast_or.sh   # Wine/OR; reloj OR, no del sistema
./scripts/install_chiltern_brake_coast_baseline.sh

cd examples/chiltern
openrailsrs sim scenario_brake_coast.toml --driver driver_brake_coast.csv
openrailsrs compare-or ../baselines/chiltern_brake_coast/or_evaluation_speed.csv run_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast

# Multi-cuerpo
openrailsrs sim scenario_brake_coast_multi_body.toml --driver driver_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast_multi_body
```

| Fase costa 115–180 s | Masa puntual | Multi-cuerpo (`coupler_kind=pullman`, coast scalar <16 m/s) |
|----------------------|--------------|---------------------------------------------------------------|
| RMS vs OR | ~0.07 m/s | ~0.10 m/s |

Freno en Wine: teclas `'`/`;` suelen fallar; usar mouse en detente **full service** (anteúltima), o remapear a PageDown/PageUp. Análisis propagación: `python3 ../../scripts/analyze_brake_propagation.py run_brake_coast.csv`.

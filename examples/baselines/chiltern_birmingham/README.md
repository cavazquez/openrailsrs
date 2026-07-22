# Baseline Chiltern / Birmingham Pullman

Captura OR 1.6.1 (Wine). Setup: [`docs/CHILTERN.md`](../../../docs/CHILTERN.md). Paridad: [`docs/OR_PARITY.md`](../../../docs/OR_PARITY.md).

| Campo | Valor |
|-------|--------|
| Actividad | `RS_Let's go to Birmingham` |
| Consist | Birmingham Pullman (8 coches, multi-cuerpo en OR) |
| Eval versionada | ~61 s → `or_evaluation_speed.csv` |

Umbrales en `examples/chiltern/scenario.overlay.toml` (p. ej. velocity RMS 4.5 m/s, position max 55 m).

```bash
./scripts/capture_chiltern_birmingham_or.sh
./scripts/install_chiltern_birmingham_baseline.sh   # MIN_DURATION_S=180 opcional

cd examples/chiltern
openrailsrs or-eval-driver ../baselines/chiltern_birmingham/or_evaluation_speed.csv --out driver_or.csv
openrailsrs sim scenario.toml --driver driver_or.csv
openrailsrs compare-or ../baselines/chiltern_birmingham/or_evaluation_speed.csv run.csv
```

CSV evaluación: `%APPDATA%/Open Rails_<Actividad>Speed.csv`. En Wine, desactivar rendimiento si crashea `pdh.dll`.

# SCE — Scottish Capital Express (Demo Model 1)

Actividad MSTS **0930 Edinburgh-Glasgow Queen Street** (Demo Model 1). Baseline ~100 s: `../baselines/sce_glasgow/`. Paridad: [`docs/OR_PARITY.md`](../../docs/OR_PARITY.md).

| Campo | Valor |
|-------|--------|
| Ruta | `Content/Demo Model 1/ROUTES/SCE` |
| Consist | Class 47 + 6 Mk2 |
| RMS vs OR (100 s) | ~0.30 m/s (puntual) / ~0.29 m/s (multi-cuerpo) |

```bash
SCE="$HOME/Documentos/Open Rails/Content/Demo Model 1/ROUTES/SCE"
openrailsrs import-msts "$SCE" --out-dir examples/sce \
  --activity "$SCE/ACTIVITIES/MT_MT_0930 Edinburgh-Glasgow Queen Street.act"
# Overlay corrige start=n474 (import deja n3).

cd examples/sce
openrailsrs or-eval-driver ../baselines/sce_glasgow/or_evaluation_speed.csv \
  --out driver_or.csv --scenario scenario.toml
openrailsrs sim scenario.toml --driver driver_or.csv
openrailsrs compare-or ../baselines/sce_glasgow/or_evaluation_speed.csv run.csv

openrailsrs sim scenario_multi_body.toml --driver driver_or.csv
cargo test -p openrailsrs-cli --test sce_multi_body
```

Nuevo baseline OR: [`../baselines/sce_glasgow/README.md`](../baselines/sce_glasgow/README.md).

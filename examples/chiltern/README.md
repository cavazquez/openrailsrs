# Chiltern — validación OR

Ruta MSTS Chiltern (Open Rails). Guía: [`docs/CHILTERN.md`](../../docs/CHILTERN.md). Física: [`docs/OR_PARITY.md`](../../docs/OR_PARITY.md).

| Campo | Valor |
|-------|--------|
| Ruta | `$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern` |
| Actividad | `RS_Let's go to Birmingham` |
| Baseline | `../baselines/chiltern_birmingham/` (~136 s) |
| Física default | Masa puntual (OR es multi-cuerpo) — ver OR_PARITY |
| Consist | DMBSA + 6 Pullman + DMBSH (8) |

## Import / coords

```bash
CHILTERN="/path/to/Chiltern/ROUTES/Chiltern"
cargo run -p openrailsrs-cli -- import-msts "$CHILTERN" \
  --out-dir examples/chiltern \
  --activity "$CHILTERN/ACTIVITIES/RS_Let's go to Birmingham.act"
# Solo refrescar x_m/y_m sin reescribir topología: --patch-coords (ver --help)
```

`WORLD/` / `TILES/` / texturas no van al git (~GB). Usá `--route-root` apuntando al Content.

## Sim

```bash
openrailsrs sim scenario.toml
openrailsrs sim scenario_multi_body.toml --driver driver_or.csv
cargo test -p openrailsrs-cli --test chiltern_multi_body
```

Compare vs OR: [`docs/OR_TRACE_COMPARISON.md`](../../docs/OR_TRACE_COMPARISON.md).

## Viewer

```bash
export CHILTERN_ROUTE="$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
# Cabina sin WORLD: añadir --run-corridor
# Solo TDB: --track-dev (+ OPENRAILSRS_TRACK_AUDIT=1 / TRACK_DEV_RENDER=1)
```

Cabina: [`docs/CABVIEW3D.md`](../../docs/CABVIEW3D.md). Tests/goldens: [`docs/VIEWER3D_TESTING.md`](../../docs/VIEWER3D_TESTING.md).

Escenarios extra: `scenario_brake_coast.toml`, throttle*, overlays en este directorio. Baselines: `examples/baselines/chiltern_*`.

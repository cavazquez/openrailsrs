# Chiltern — setup OR y simulación

Ruta de validación principal. Detalle de escenarios/baselines: [`examples/chiltern/README.md`](../examples/chiltern/README.md).

## Content

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"
# Esperado: $CHILTERN_ROUTE/{TILES,SHAPES,TEXTURES,OpenRails/Chiltern.trk}
```

Instalación típica: Open Rails 1.6 + contenido Chiltern (Steam/manual) bajo `Content/Chiltern/`. En Linux, OR vía Wine si hace falta capturar baselines.

## Sim headless

```bash
cargo run -p openrailsrs-cli -- sim examples/chiltern/scenario.toml
# Multi-cuerpo / freno / throttle: ver scenarios en examples/chiltern/
```

Comparar CSV vs OR: [`OR_TRACE_COMPARISON.md`](OR_TRACE_COMPARISON.md) (`openrailsrs compare-or`).

## Viewer live

```bash
# Full (terreno + WORLD)
cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml

# Corredor (sin WORLD pesado; depurar cabina)
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Teclas: **C** cabina · **V** chase · **↑/↓** throttle/freno · **F** fog · **F2** fly · **G** teleport. Spawn WORLD progresivo (~10–30 s según radio).

Ancla típica Birmingham: tile **−6079…−6080 / 14925**. Ver [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

## Goldens visuales

```bash
./scripts/visual_regression_smoke.sh          # smoke CI
UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh  # exterior+cabina (local)
```

Física / RMS: [`OR_PARITY.md`](OR_PARITY.md).

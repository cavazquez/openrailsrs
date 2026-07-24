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

Teclas (estilo OR): **1** cabina · **Alt+1** 2D/3D · **2** chase · **3** orbit · **5** pasajero · **8** fly · **A/D** throttle · **;/'** freno · **W/S** reverser · **Space** bocina · **V** wiper · **Backspace** emergencia · **RMB** mirar (cab/pasajero) · **F** fog · **G** teleport. Spawn WORLD progresivo (~10–30 s según radio).

Ancla de **salida** (Paddington / TrackPDP[0]): tile **−6079…−6080 / 14925**. Destino Birmingham ~**−6111 / 14957**. Ver [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

## Goldens visuales

```bash
./scripts/visual_regression_smoke.sh          # smoke CI
UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh  # exterior+cabina (local)
```

Física / RMS: [`OR_PARITY.md`](OR_PARITY.md).

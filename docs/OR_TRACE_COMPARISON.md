# Comparación con trazas Open Rails

Este documento describe cómo comparar una corrida de **openrailsrs** contra una traza exportada desde **Open Rails** (OR).

## Formato de traza Open Rails

Open Rails puede generar un archivo `dump.csv` con el **Data Logger**:

1. En el menú de OR: **Options → Data Logger** (o equivalente).
2. Activar **Start logging with the simulation start**, o pulsar **F12** durante la simulación.
3. Configurar **Logging interval** ≥ 100 ms para rendimiento del tren (0 = cada frame, útil para GPU pero muy denso).
4. El archivo se escribe en la carpeta de logging configurada (por defecto el Escritorio).

Referencia: [manual OR — Data Logger Options](https://open-rails.readthedocs.io/en/latest/options.html#data-logger-options).

Open Rails **no tiene modo headless**; la captura del baseline es manual.

## Flujo recomendado

```bash
# 1. Correr openrailsrs (desde el directorio del escenario)
openrailsrs sim examples/smoke/scenario.toml

# 2. Correr el mismo escenario en Open Rails (misma ruta MSTS, consist y controles equivalentes)
#    y guardar dump.csv

# 3. Comparar (remuestreo lineal cada 0.1 s por defecto)
openrailsrs compare-or /path/to/dump.csv examples/smoke/run.csv \
  --max-velocity-rms 1.0 \
  --max-position-max 50.0
```

Para rutas MSTS reales, importar primero con `openrailsrs import-msts <route_dir>` y alinear consist/física lo más posible antes de comparar.

## Mapeo de columnas

El adaptador usa [`OrColumnMap`](crates/openrailsrs-validate/src/trace.rs) con defaults:

| Campo OR (default) | openrailsrs | Unidad OR → SI |
|--------------------|-------------|----------------|
| `Time` | `time_s` | segundos |
| `Speed` | `velocity_mps` | mph → m/s (× 0.44704) |
| `Distance` | `odometer_m` | metros |

Si tu `dump.csv` usa otros nombres o unidades, crea un TOML:

```toml
time_column = "Time"
speed_column = "Speed"
distance_column = "Distance"
speed_unit = "mph"      # mph | kmh | mps
distance_unit = "meters" # meters | miles | km
# throttle_column = "Throttle"
# brake_column = "TrainBrake"
```

```bash
openrailsrs compare-or dump.csv run.csv --map or_column_map.toml
```

**Nota:** el layout exacto del data logger puede variar según versión de OR. Tras la primera captura real, revisa el header de `dump.csv` y ajusta el mapa.

### Dump de rendimiento (OR 1.6.x, Wine)

Si activas **Performance / Physics / Steam** en el Registrador de datos, OR escribe un CSV distinto (p. ej. `Speed (mph),Time (M),Throttle (%)…`) **sin** columnas `Time`, `Speed`, `Distance` que espera `compare-or` v1.

Baseline real guardado en el repo:

- `examples/baselines/chiltern_birmingham/` — ruta Chiltern, actividad *Let's go to Birmingham*, ~2.5 min sim (10:00–10:02:24).

Para un CSV compatible con `compare-or`, usa la pestaña **Evaluación** y activa solo el registro de **velocidad del tren** (Time, Train Speed, Distance Travelled). En Wine, desactiva el registro de rendimiento: puede abortar con `pdh.dll.PdhFormatFromRawValue`.

## Escenario TOML — sección `[validate]`

Metadata opcional en `scenario.toml` (no ejecuta la comparación automáticamente):

```toml
[validate]
max_velocity_rms = 0.5
max_position_max = 10.0
baseline_or = "baselines/my_route_dump.csv"
```

Los umbrales siguen el mismo esquema que `openrailsrs compare`. `baseline_or` es una ruta relativa al directorio del escenario, documentada para scripts externos.

## Qué compara el MVP (v1)

| Métrica | Incluida |
|---------|----------|
| Velocidad | Sí (remuestreada) |
| Distancia / odómetro | Sí |
| Energía acumulada | Solo si ambos CSV tienen columna de energía |
| Posición topológica (`edge_id`) | No (modelos distintos) |

## Limitaciones conocidas

- Sin Open Rails en CI: los tests usan fixtures sintéticos en `crates/openrailsrs-validate/tests/fixtures/`.
- Controles, señales y física avanzada pueden divergir aunque velocidad/distancia coincidan.
- Alinear `import-msts` con el loader de `track.toml` mejora comparaciones en rutas MSTS reales (trabajo futuro).

## Fixtures de prueba

```bash
openrailsrs compare-or \
  crates/openrailsrs-validate/tests/fixtures/or_dump_minimal.csv \
  crates/openrailsrs-validate/tests/fixtures/ors_run_aligned.csv \
  --max-velocity-rms 1e-6 --max-position-max 1e-6
```

Debe imprimir `overall: PASS`.

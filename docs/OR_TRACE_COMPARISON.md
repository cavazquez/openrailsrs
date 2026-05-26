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

## Chiltern Birmingham (eval 61 s)

Guía completa Wine + OR + sim: [`CHILTERN_OR_SETUP.md`](CHILTERN_OR_SETUP.md).

Gaps cerrados en openrailsrs:

| Área | Estado |
|------|--------|
| TDB `msts_aliases` + switches salientes | Import emite `[[msts_aliases]]`; PAT resuelve vectores (`tdb_id=2` → `e2`). |
| Placement PAT | `start=n3`, `start_offset_m≈305.6`, destino y `[[route.switches]]` desde PAT+`.srv`. |
| Consist | Trainset `RF_Blue_Pullman` (masa/longitud/freno por vehículo); script `scripts/sync_chiltern_assets.sh`. |
| Física diesel MSTS | Parser con unidades; **`ORTSMaxTractiveForceCurves`** por notch → `DieselTractionModel`; interpolación F(v) por throttle; calibración vs `MaxForce` continuo. |
| Lead loco DMU | Solo la primera locomotora aporta tracción (evita duplicar Pullman 2×). |
| Vapor MSTS | `MstsSteamFields` en `.eng` → `SteamParams` (Pullman OR es diesel, no activa `steam_step`). |
| CI | `cargo test -p openrailsrs-cli --test chiltern_validate` (skip si falta `track.toml`). |

Umbrales `[validate]` en `examples/chiltern/scenario.overlay.toml` (fusionado tras `import-msts`):

```toml
max_velocity_rms = 4.5
max_position_max = 55.0
max_throttle_rms = 0.25
max_brake_rms = 50.0
baseline_or = "../baselines/chiltern_birmingham/or_evaluation_speed.csv"
```

Resultados actuales (driver desde eval OR, ~65 s sim):

| Métrica | vs OR | Umbral | Estado |
|---------|-------|--------|--------|
| Velocity RMS | ~3.3 m/s | 4.5 | PASS |
| Position RMS / max | ~13.5 m / ~46 m | 55 m max | PASS |
| Odómetro @ 65 s | ~200 m (OR eval ~205 m @ 61 s) | — | Δ razonable |

Mejor que el modelo P/v simplificado (~4.4 m/s RMS). Pendiente para objetivo estricto (`0.3` m/s / `25 m`): RPM por notch (`DieselPowerTab` + `ThrottleRPMTab`), scripts cab (`Default.cs`), dinámica de carga motor.

Para un CSV compatible con `compare-or`, usa la pestaña **Evaluación** y activa solo el registro de **velocidad del tren** (Time, Train Speed, Distance Travelled). En Wine, desactiva el registro de rendimiento: puede abortar con `pdh.dll.PdhFormatFromRawValue`.

### Registro de evaluación `*Speed.csv` (OR 1.6.x)

Con **Evaluación → Train speed logging** activo, OR escribe en `%APPDATA%`:

```text
Open Rails_<NombreActividad>Speed.csv
```

Ejemplo (Wine/Linux):

```text
/home/cristian/wine64-OpenRails/drive_c/users/cristian/AppData/Roaming/Open Rails_RS_Let's go to BirminghamSpeed.csv
```

Header típico:

```text
TIME,TRAINSPEED,MAXSPEED,SIGNALASPECT,ELEVATION,DIRECTION,CONTROLMODE,DISTANCETRAVELLED,THROTTLEPERC,...
```

`compare-or` **detecta automáticamente** este formato (no hace falta `--map`):

- `TIME` en `HH:MM:SS` → segundos relativos al primer sample
- `TRAINSPEED` en mph (puede partirse en dos columnas si el CSV usa coma como separador decimal)
- `DISTANCETRAVELLED` en metros (`DistanceTravelledM` en OR)
- `THROTTLEPERC` / `BRAKEPRESSURE` opcionales

Baseline en el repo: `examples/baselines/chiltern_birmingham/or_evaluation_speed.csv` (~61 s sim, throttle ~80 %).

### Driver desde evaluación OR

```bash
openrailsrs or-eval-driver \
  examples/baselines/chiltern_birmingham/or_evaluation_speed.csv \
  --out examples/chiltern/driver_or.csv

openrailsrs sim examples/chiltern/scenario.toml --driver examples/chiltern/driver_or.csv
```

### Dump de rendimiento (`Speed (mph),Time (M),…`)

`compare-or` detecta también el header de rendimiento OR 1.6.x (columnas de ancho variable). Extrae `Time (M)` (`HH:MM:SS`), velocidad (`*mph`) y throttle cuando está presente; la distancia se integra desde velocidad si no hay columna fiable.

Baseline: `examples/baselines/chiltern_birmingham/openrails_dump.csv` (~144 s sim).

Config recomendada en OR (Opciones → Registrador de datos / Evaluación):

- `DataLogTrainSpeed = True`
- `DataLogTSInterval = 1` (1 s)
- Performance / Physics / Steam = **False**
- `DataLogStart = True` o pulsar **F12** al iniciar la simulación

## Escenario TOML — sección `[validate]`

Metadata opcional en `scenario.toml`. Si `baseline_or` está definido, `openrailsrs sim` ejecuta `compare-or` al final (salvo `--no-validate`):

```toml
[validate]
max_velocity_rms = 2.0
max_position_max = 150.0
max_throttle_rms = 0.20
baseline_or = "../baselines/chiltern_birmingham/or_evaluation_speed.csv"
```

Los umbrales siguen el mismo esquema que `openrailsrs compare` / `compare-or` (`max_throttle_rms`, `max_brake_rms`, …). `baseline_or` es una ruta relativa al directorio del escenario.

## Qué compara el MVP (v1)

| Métrica | Incluida |
|---------|----------|
| Velocidad | Sí (remuestreada) |
| Distancia / odómetro | Sí |
| Energía acumulada | Solo si ambos CSV tienen columna de energía |
| Throttle / freno | Sí, si ambas trazas tienen columna |
| Posición topológica (`edge_id`) | No (modelos distintos) |

## Limitaciones conocidas

- Sin Open Rails en CI: los tests usan fixtures sintéticos en `crates/openrailsrs-validate/tests/fixtures/`.
- Controles, señales y física avanzada pueden divergir aunque velocidad/distancia coincidan.
- Alinear `import-msts` con el loader de `track.toml` mejora comparaciones en rutas MSTS reales (trabajo futuro).

## Fixtures de prueba

```bash
# Dump estándar sintético (Time/Speed/Distance)
openrailsrs compare-or \
  crates/openrailsrs-validate/tests/fixtures/or_dump_minimal.csv \
  crates/openrailsrs-validate/tests/fixtures/ors_run_aligned.csv \
  --max-velocity-rms 1e-6 --max-position-max 1e-6

# OR 1.6 evaluación (parseo automático TIME/TRAINSPEED)
openrailsrs compare-or \
  crates/openrailsrs-validate/tests/fixtures/or_eval_speed_minimal.csv \
  examples/smoke/run.csv
```

Debe imprimir `overall: PASS`.

<div align="center">

# openrailsrs

**Simulador ferroviario headless-first en Rust** — primero física y datos por consola (CSV + TOML), después reglas de juego y un viewer 2D desacoplado.

[![CI](https://github.com/cavazquez/openrailsrs/actions/workflows/ci.yml/badge.svg)](https://github.com/cavazquez/openrailsrs/actions/workflows/ci.yml)
[![GitHub stars](https://img.shields.io/github/stars/cavazquez/openrailsrs?style=social&logo=github&label=estrellas)](https://github.com/cavazquez/openrailsrs/stargazers)
[![GitHub all releases](https://img.shields.io/github/downloads/cavazquez/openrailsrs/total?label=descargas&logo=github)](https://github.com/cavazquez/openrailsrs/releases)
[![codecov](https://codecov.io/gh/cavazquez/openrailsrs/graph/badge.svg)](https://codecov.io/gh/cavazquez/openrailsrs)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/rust-stable-f74c00?logo=rust&logoColor=white)](https://www.rust-lang.org/)

*Los badges de estrellas y descargas los sirve [shields.io](https://shields.io/) con datos en vivo de GitHub. La cobertura refleja el último informe subido a [Codecov](https://codecov.io/gh/cavazquez/openrailsrs) (activá el repo en Codecov si el badge aún no muestra porcentaje).*

</div>

---

## Caja de herramientas

| Herramienta | Uso en el proyecto |
|-------------|-------------------|
| 🦀 **Rust** | Lenguaje y toolchain estable. |
| 📦 **Cargo** | Workspace multi-crate, build y publicación. |
| 🔧 **Clippy** | Lint estricto (`-D warnings`) en CI y en `check.sh`. |
| 🎨 **rustfmt** | Estilo uniforme (`cargo fmt --check`). |
| 🧪 **cargo test** | Tests unitarios e integración por crate. |
| 📊 **cargo-llvm-cov** | Cobertura en GitHub Actions → informe LCOV / Codecov. |
| ⚡ **GitHub Actions** | CI en Ubuntu: ejecuta `check.sh` + job de cobertura. |
| 🔄 **rayon** | Batch de escenarios en CLI (paralelismo opcional). |
| 📝 **TOML / CSV** | Escenarios, metadata y series temporales. |
| 🖼️ **minifb** | Viewer 2D mínimo (X11 en Linux), sin acoplar al núcleo `sim`. |

---

## CI local y en GitHub

El script **[`check.sh`](check.sh)** concentra lo que debe pasar antes de pushear:

1. `cargo fmt --all -- --check`  
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings`  
3. `cargo test --workspace --all-features`  
4. `cargo build --workspace --all-features`  

```bash
chmod +x check.sh   # solo la primera vez, si hace falta
./check.sh
```

En GitHub, el workflow **[`.github/workflows/ci.yml`](.github/workflows/ci.yml)**:

- Job **`check.sh`**: mismo flujo que arriba (con librerías X11 instaladas para compilar `openrailsrs-viewer`).
- Job **cobertura**: `cargo llvm-cov` y subida a Codecov (no falla el CI si Codecov no está configurado todavía).

---

## Qué es openrailsrs

Simulador ferroviario pensado como **videojuego de simulación**, pero con **núcleo headless**: la simulación no depende del rendering. **Linux-first**, sin Bevy/wgpu en el stack principal; el viewer vive aparte.

Las fases de producto (0–10) están en **[ROADMAP.md](ROADMAP.md)**.

### Estado de fases (roadmap)

| Fase | Estado actual | Evidencia en el repo |
|------|---------------|----------------------|
| 0 — Bootstrap | Base implementada | Workspace Cargo, crates modulares, CI y documentación. |
| 1 — Parsers MSTS/OpenRails | Profundizado | `openrailsrs-formats`: AST genérico + adaptadores tipados (`EngineFile`, `WagonFile`, `ConsistFile`, `RouteFile`) + conversiones de unidades + dispatch por extensión. |
| 2 — Datos/config juego | Profundizado | `openrailsrs-scenarios`: `scenario.toml` con `[[route.stops]]` (paradas intermedias con `arrive_s`/`depart_s`) y `[train.davis]` (coeficientes Davis sobreescribibles). |
| 3 — Modelo lógico ferroviario | Profundizado | `openrailsrs-track`: señales (`Stop/Caution/Clear`, `clear_after_s`), `insert_signal`, `signals_on_edge`; runner `RunPhase::AwaitingSignal`; **agujas funcionales** (`SwitchPosition::Straight/Diverging`, `set_switch`, `switch_position`, error `NotASwitch`); BFS respeta la posición de cada `NodeKind::Switch`; `default_position` en `track.toml` y `[[switches]]` sobreescribibles por escenario. |
| 4 — Modelo físico del tren | Profundizado | `DavisCoefficients` configurable en `Consist`; `TractiveCurve` (puntos v→F, interpolación piecewise-linear) en `Locomotive`; `TrainPhysics` agrega la curva o sintetiza una desde P/F_te. |
| 5 — Simulación headless | Profundizado | `physics::step` usa `TractiveCurve` si existe, P/v como fallback; máquina de estados `Normal→Approaching→Dwelling→AwaitingSignal`; `ScriptedDriver` replay desde CSV; `run_from_scenario_file_with_driver` para driver externo desde CLI. |
| 6 — Capa de videojuego headless | Profundizado | `evaluate` multi-parada: `missed_stop`, penalización **graduada** (`penalty_per_second_late` pts/s de retraso), `early_departure`; `PlayOutcome` añade `punctuality_pct` y `total_delay_s`; `play-headless` imprime timeline completo + tabla de paradas; `outcome.toml` con desglose. |
| 7 — Validación/comparación | **Profundizado** | `openrailsrs-validate`: `ValidationConfig` con tolerancias por columna (`max_velocity_rms`, `max_position_max`, etc.); `ComparisonReport` con `pass`/`fail` por columna y global; CLI `compare` con 6 flags de umbral y exit code 1 si falla. |
| 8 — Debug sin gráficos | Profundizado | `openrailsrs-export`: DOT/GeoJSON/ASCII + **replay animado** (`animated_replay_from_csv`: barra de progreso ANSI, refresco in-place, velocidad configurable). |
| 9 — Optimización | **Profundizado** | `PathData` pre-computa `Vec<PathEdgeData>` antes del bucle → `physics::step` usa indexación directa (sin `HashMap::get` por tick); benchmarks Criterion: micro, escenario completo, multi-tren. |
| 10 — Viewer 2D animado | **Profundizado** | `openrailsrs-viewer`: topología + señales con aspecto real (rojo/amarillo/verde), **replay multi-tren animado** desde CSV, HUD con t, velocidad por tren, barra de progreso, controles teclado. |
| 13 — Importar rutas reales | **Implementado** | `openrailsrs-import`: Overpass JSON (OSM) → `track.toml`; junctions automáticos, Haversine, proyección equirectangular, estaciones, speed limit desde tag `maxspeed`. |

> Nota: “Base implementada” significa línea base funcional; la **profundidad futura** de cada fase sigue evolucionando en iteraciones.
>
> **Fase 13** implementada: `openrailsrs import-osm overpass.json --out routes/myroute/track.toml`. Ver [`examples/osm/`](examples/osm/).

### Principios

- El núcleo corre **sin gráficos**; la simulación no depende de rendering.
- **Linux-first**, Rust estable.
- Datos de serie temporal en **CSV**; escenarios, configuración y metadata en **TOML**.
- **Sin** Bevy, wgpu ni motores gráficos en las fases iniciales; el viewer mínimo vive en el crate `openrailsrs-viewer` (Fase 10).
- Workspace Cargo modular bajo `crates/`.

### Crates

| Crate | Rol |
|--------|-----|
| `openrailsrs-core` | Tipos compartidos (tiempo simulado, IDs). |
| `openrailsrs-formats` | Tokenizer + AST genérico + adaptadores tipados por extensión (`EngineFile`, `WagonFile`, `ConsistFile`, `RouteFile`) + conversiones de unidades MSTS → SI. |
| `openrailsrs-scenarios` | Carga/validación de `scenario.toml`; paradas intermedias (`[[route.stops]]`), override de Davis y **`[[switches]]`** para sobreescribir posición de agujas por escenario. |
| `openrailsrs-route` | Carga de `track.toml` con `grade_percent`, `[[signals]]` y `default_position` en nodos Switch. |
| `openrailsrs-track` | Grafo de vía, nodos, aristas, señales (`Stop/Caution/Clear`) y **agujas** (`SwitchPosition`, `set_switch`, `switch_position`, error `NotASwitch`). |
| `openrailsrs-train` | Locomotoras, vagones, consists; `DavisCoefficients` y `TractiveCurve` (piecewise-linear) configurables. |
| `openrailsrs-sim` | Bucle headless; `TrainPhysics + TractiveCurve`; máquina `Normal→Approaching→Dwelling→AwaitingSignal`; **BFS switch-aware**; `ScriptedDriver` + `run_from_scenario_file_with_driver`; **`multi_runner`** con `BlockMap` y bucle sincronizado multi-tren; `SimEvent` overspeed/estaciones/señales/`BlockWait`/`BlockClear`; **`PathData`** (pre-cómputo de aristas, sin `HashMap` en el hot loop); `run.csv` + `run.toml`. |
| `openrailsrs-game` | Objetivos, penalizaciones multi-parada (`missed_stop`, `late_stop` graduado, `early_departure`); `PlayOutcome` con `punctuality_pct` / `total_delay_s` / `delay_s` por parada; `play-headless` con **timeline completo** por stdout; `outcome.toml`. |
| `openrailsrs-import` | Importa topología ferroviaria real desde Overpass JSON (OpenStreetMap) → `track.toml`; proyección equirectangular, Haversine, estaciones y speed limit. |
| `openrailsrs-validate` | Comparación cuantitativa de dos `run.csv`: RMSE, max/mean abs por columna; `ValidationConfig` con umbrales por columna; `pass`/`fail` por serie y global. |
| `openrailsrs-export` | DOT, GeoJSON, mapa ASCII, replay textual y **replay animado** (ANSI, barra de progreso, velocidad configurable). |
| `openrailsrs-cli` | Binario **`openrailsrs`**. |
| `openrailsrs-viewer` | Binario **`openrailsrs-viewer`**: topología de vía, señales coloreadas por aspecto, **replay multi-tren animado** desde CSV, HUD con tiempo y velocidad, controles teclado. Lee `scenario.toml` o `route_dir` directamente. |

Los módulos públicos en Rust siguen el patrón `openrailsrs_<crate>::…` (p. ej. `openrailsrs_sim::run_from_scenario_file`).

---

## Requisitos

- Rust estable (edition 2024, `rust-version` en workspace).
- Linux (el viewer usa `minifb` con feature `x11`).

## Construir y probar

```bash
cargo build
cargo test
```

Ejemplo de escenario listo: [`examples/smoke/scenario.toml`](examples/smoke/scenario.toml).

## CLI (`openrailsrs`)

Instalación del binario (queda disponible como comando global):

```bash
cargo install --path crates/openrailsrs-cli
```

O sin instalar, usando `cargo run -p openrailsrs-cli --`:

```bash
# Inspeccionar AST genérico de un archivo MSTS
openrailsrs inspect path/al/archivo.eng

# Exportar grafo DOT de la ruta
openrailsrs graph examples/smoke/routes/test --out route.dot

# Simulación headless con AutoDriver (por defecto)
openrailsrs sim examples/smoke/scenario.toml

# Simulación headless con ScriptedDriver (CSV con time_s,throttle,brake)
openrailsrs sim examples/smoke/scenario.toml --driver examples/smoke/driver_script.csv

# Ruta alternativa: switch en posición divergente → siding_c
openrailsrs sim examples/smoke/scenario_diverging.toml

# Partida headless: imprime timeline completo + tabla de paradas + escribe outcome.toml
openrailsrs play-headless examples/smoke/scenario.toml

# Comparar dos corridas CSV (sin umbrales → siempre pasa)
openrailsrs compare run1.csv run2.csv

# Comparar con umbrales estrictos → exit code 1 si falla
openrailsrs compare run1.csv run2.csv \
  --max-velocity-rms 0.5 \
  --max-position-max 10.0 \
  --max-energy-rms 0.01

# Exportar GeoJSON y mapa ASCII de la ruta
openrailsrs export-geojson examples/smoke/routes/test --out track.geojson
openrailsrs ascii-map examples/smoke/routes/test

# Replay textual (primeras 25 filas del CSV)
openrailsrs replay examples/smoke/run.csv

# Replay animado: panel multi-línea ANSI, 20× más rápido que real-time
openrailsrs replay examples/smoke/run.csv --watch --speed 20

# Simulación multi-tren (block occupancy sincronizado)
openrailsrs sim-multi examples/smoke/scenario_multi.toml

# Importar ruta desde OpenStreetMap (Overpass JSON) → track.toml
openrailsrs import-osm examples/osm/overpass_sample.json \
  --out routes/badner_bahn/track.toml \
  --route-id badner_bahn

# Batch con rayon (varios escenarios en paralelo)
openrailsrs batch examples/smoke/scenario.toml examples/smoke/scenario_diverging.toml

# Logs tracing opcionales
openrailsrs -v sim examples/smoke/scenario.toml
```

### Panel de replay animado

El flag `--watch` muestra un panel multi-línea que se refresca en vivo:

```
┌──────────────────────────────────────────────────────────────┐
│  openrailsrs  ·  replay  ·  run.csv                          │
│                                                              │
│  Recorrido  ████████████████████░░░░░░░░░░░░░░░░   7840m  78%│
│  Tiempo         485.7 s                                      │
│  Velocidad   65.4 km/h       ↑ pico  78.2 km/h              │
│  Tracción    [████████████        ]  60%                     │
│  Freno       [                    ]   0%                     │
│  Arista      e3           pos en arista     340 m            │
│  Energía     22.450 kWh                                      │
└──────────────────────────────────────────────────────────────┘
```

### Viewer 2D animado (Fase 10)

```bash
# Vista estática de la topología (solo track.toml)
cargo run -p openrailsrs-viewer -- examples/smoke/routes/test

# Replay animado multi-tren (lee scenario.toml y los CSV generados)
cargo run -p openrailsrs-viewer -- examples/smoke/scenario_multi.toml

# Escenario individual
cargo run -p openrailsrs-viewer -- examples/smoke/scenario.toml
```

El viewer muestra:
- **Aristas** como líneas naranjas con etiqueta de `edge_id`.
- **Nodos** como círculos: blanco (Plain), cian (Switch), amarillo (Station).
- **Señales** como diamantes coloreados: 🔴 rojo (`stop`), 🟡 amarillo (`caution`), 🟢 verde (`clear`), con poste y etiqueta.
- **Trenes** como círculos animados con glow proporcional a la velocidad, color distinto por tren, velocidad en km/h en el HUD.
- **HUD** inferior: nombre del escenario, `t=XXX.Xs`, multiplicador de velocidad, barra de progreso, leyenda de trenes.

Controles de teclado: `Space` pausar/reanudar · `R` reiniciar · `+`/`-` doblar/dividir velocidad de replay · `Esc` salir.

## Benchmarks (Fase 9)

```bash
# Todos los benchmarks del crate sim
cargo bench -p openrailsrs-sim --bench sim_step

# Benchmarks disponibles:
#   physics_step_100        → 100 ticks en hot loop (micro)
#   full_scenario_smoke     → escenario smoke completo de punta a punta
#   full_scenario_multi_train → escenario multi-tren con block occupancy
```

La optimización clave de Fase 9 es `PathData`: los datos de cada arista del camino se pre-computan en un `Vec` antes del bucle. `physics::step` hace `vec[idx]` en lugar de `HashMap::get(&str)` en cada tick, eliminando hashing por string en el hot loop.

## Formato de escenario (`scenario.toml`)

El escenario describe ruta, tren, gameplay y parámetros de simulación. Los campos nuevos más relevantes:

```toml
[route]
path    = "routes/test"
start   = "yard_a"
destination = "yard_b"

[[route.stops]]        # paradas intermedias (opcional, repetible)
node     = "mid"
arrive_s = 400.0       # tiempo objetivo de llegada (s)
depart_s = 420.0       # tiempo objetivo de salida (s)
dwell_s  = 60.0        # tiempo de espera en plataforma (s, default 0)

[train]
consist = "consists/freight.con"

[train.davis]          # override de resistencia Davis (opcional)
a_n          = 800.0   # término constante (N)
b_n_per_mps  = 12.0    # término lineal (N·s/m)
c_n_per_mps2 = 0.4     # término cuadrático (N·s²/m²)
```

El campo `grade_percent` en cada arista de `track.toml` indica la pendiente (positivo = subida, negativo = bajada). El resultado `outcome.toml` incluye `[[stops]]` con `actual_arrive_s`, `actual_depart_s`, `on_time`, `missed` y `early_departure` por cada parada declarada.

### Señales en `track.toml`

Las señales se definen con `[[signals]]` dentro del archivo de ruta:

```toml
[[signals]]
id           = "sig1"          # identificador único
edge_id      = "e1"            # arista sobre la que actúa
position_m   = 0.0             # distancia desde el inicio de la arista
aspect       = "stop"          # "clear" | "caution" | "stop"
clear_after_s = 120.0          # (opcional) despeje automático a los N segundos

[[signals]]
id      = "sig2"
edge_id = "e2"
aspect  = "caution"            # reduce el speed limit efectivo al 50 %
```

El runner aplica las señales automáticamente:
- `stop` → `RunPhase::AwaitingSignal`: frena antes de entrar al bloque y espera hasta que la señal se despeje (por `clear_after_s` o por controlador externo).
- `caution` → velocidad efectiva limitada al 50 % del límite nominal de la arista, sin detener el tren.
- Eventos emitidos: `SimEvent::SignalStop` y `SimEvent::SignalClear`.

### Agujas en `track.toml` y `scenario.toml`

Los nodos de tipo `switch` definen una bifurcación con dos ramas: **stem** (directo) y **diverging** (desviado). La posición activa determina qué arista toma el BFS al calcular el camino.

Definición en `track.toml`:

```toml
[[nodes]]
id = "junction"
kind = { switch = { stem_edge = "e3", diverging_edge = "e4", default_position = "straight" } }
```

Override por escenario en `scenario.toml`:

```toml
[[route.switches]]
node     = "junction"
position = "diverging"   # "straight" | "diverging"
```

Si el escenario no incluye `[[route.switches]]`, se aplica `default_position` del `track.toml` (por defecto `straight`). El BFS solo expande la arista correspondiente a la posición activa, haciendo imposible llegar a un ramal cerrado.

El repositorio incluye dos escenarios de ejemplo para comparar ambas ramas:

| Escenario | Switch | Destino | Aristas |
|-----------|--------|---------|---------|
| [`scenario.toml`](examples/smoke/scenario.toml) | `straight` (default) | `yard_b` | e1 → e2 → e3 |
| [`scenario_diverging.toml`](examples/smoke/scenario_diverging.toml) | `diverging` | `siding_c` | e1 → e2 → e4 |

---

### Simulación multi-tren (`sim-multi`)

Dos (o más) trenes comparten el mismo grafo de vía con un único reloj de simulación y **block occupancy por arista**: si el tren B intenta entrar a una arista ya ocupada por A, se detiene automáticamente y emite `SimEvent::BlockWait`; cuando A avanza y libera el bloque B recibe `SimEvent::BlockClear` y reanuda la marcha.

```bash
# Ejecutar escenario multi-tren
openrailsrs sim-multi examples/smoke/scenario_multi.toml
```

Salida de ejemplo:

```
=== SimMulti: examples/smoke/scenario_multi.toml ===
  [primary] reached=true t=666.2s odometer=10000m energy=67.500kwh block_waits=0
  [express] reached=true t=793.0s odometer=10000m energy=73.646kwh block_waits=2
```

Los trenes extra se definen con `[[extra_trains]]` en `scenario.toml`:

```toml
[[extra_trains]]
id           = "express"
consist      = "consists/freight.con"
start        = "yard_a"
destination  = "yard_b"
start_time_s = 60.0          # sale 60 s después del primario
output_csv   = "run_express.csv"
davis        = { a_n = 500.0, b_n_per_mps = 8.0, c_n_per_mps2 = 0.2 }
```

Cada tren escribe su propio CSV con las series temporales de velocidad, posición y energía.

### Penalizaciones graduales de timetable

El campo `penalty_per_second_late` (default `1.0`) en `[gameplay]` controla cuántos puntos se descuentan por cada segundo de retraso respecto al horario declarado (más allá del margen de gracia `STOP_GRACE_S = 30 s`):

```toml
[gameplay]
penalty_per_second_late = 2.0   # 2 puntos por segundo de retraso
```

`PlayOutcome` ahora incluye:

| Campo | Descripción |
|-------|-------------|
| `punctuality_pct` | % de paradas alcanzadas a tiempo (0–100) |
| `total_delay_s` | Suma total de segundos de retraso en todas las paradas |
| `delay_s` (en cada `StopResult`) | Retraso individual respecto a `arrive_s` |

---

### Importar rutas reales desde OpenStreetMap (`import-osm`)

El comando `import-osm` convierte un JSON de la **Overpass API** en un `track.toml` listo para usar. No hace falta ninguna instalación de Open Rails ni MSTS — basta con datos libres de OpenStreetMap.

**Flujo:**

1. Abrí [overpass-turbo.eu](https://overpass-turbo.eu/) y pegá la query del archivo [`examples/osm/overpass_query.txt`](examples/osm/overpass_query.txt).
2. Reemplazá `{{bbox}}` por las coordenadas de tu ruta (sur,oeste,norte,este).
3. Exportá como **raw OSM data (JSON)**.
4. Importá con:

```bash
openrailsrs import-osm resultado.json \
  --out routes/mi_ruta/track.toml \
  --route-id mi_ruta \
  --default-speed 120   # km/h cuando no hay tag maxspeed
```

El archivo generado es editable: podés ajustar `grade_percent` de cada arista, promover nodos Plain a Switch, o agregar señales — todo en TOML legible.

**Qué importa automáticamente:**

| Dato OSM | Resultado en `track.toml` |
|----------|--------------------------|
| `railway=rail/light_rail/subway/tram` | Aristas con `length_m` Haversine |
| `maxspeed=120` | `speed_limit_kmh = 120.0` |
| `railway=station` + `name=...` | `NodeKind::Station { name }` |
| Nodo compartido por ≥ 2 ways | Nodo de junction (grafo correcto) |
| Lat/Lon WGS-84 | Proyección equirectangular → `x_m`, `y_m` |

**Limitaciones conocidas:**

- `grade_percent` siempre 0.0 (OSM no tiene altimetría confiable por tramo).
- Switches complejos (>2 ramas) quedan como `Plain`; se editan a mano.
- Señales OSM (`railway=signal`) tienen cobertura irregular, no se importan.

---

### Validación con umbrales (`compare`)

El comando `compare` muestra estadísticas por columna (RMS, max, media) y permite fijar umbrales de tolerancia con flags opcionales. Si cualquier umbral se supera, el proceso sale con **exit code 1** (útil para CI):

```
=== Compare: run_ref.csv vs run_new.csv ===
  velocity  rms=0.123456  max=0.340000  mean=0.089000  n=4859  PASS ✓
  position  rms=1.234567  max=3.100000  mean=0.890000  n=4859  PASS ✓
  energy    rms=0.000012  max=0.000034  mean=0.000008  n=4859  PASS ✓
overall: PASS

--- full report (TOML) ---
file_a = "run_ref.csv"
...
```

Flags disponibles: `--max-velocity-rms`, `--max-velocity-max`, `--max-position-rms`, `--max-position-max`, `--max-energy-rms`, `--max-energy-max`. Cualquier `None` omitido se ignora.

---

## Consists y rutas de assets

Las rutas en `(Engine "…")` y `(Wagon "…")` se resuelven respecto al **directorio del escenario** (carpeta que contiene `scenario.toml`), no respecto a la subcarpeta `consists/`, para alinear el layout con un “directorio de instalación” del escenario.

## Licencia

Este proyecto se distribuye bajo la **GNU General Public License v3.0 only** (SPDX: `GPL-3.0-only`). Ver el texto completo en [LICENSE](LICENSE).

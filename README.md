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
| 🎮 **Bevy** | Viewer 3D experimental (`openrailsrs-viewer3d`): grafo desde `track.toml`, grilla y cámara orbit/fly; desacoplado del `sim`. |

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

- Job **`check.sh`**: mismo flujo que arriba (con librerías X11 + `libxkbcommon-dev` para compilar `openrailsrs-viewer` y `openrailsrs-viewer3d`).
- Job **cobertura**: `cargo llvm-cov` y subida a Codecov (no falla el CI si Codecov no está configurado todavía).

---

## Qué es openrailsrs

Simulador ferroviario pensado como **videojuego de simulación**, pero con **núcleo headless**: la simulación no depende del rendering. **Linux-first**; el rendering vive en crates aparte (`openrailsrs-viewer` 2D, `openrailsrs-viewer3d` experimental con Bevy).

Las fases de producto (0–10) están en **[ROADMAP.md](ROADMAP.md)**.

### Estado de fases (roadmap)

| Fase | Estado actual | Evidencia en el repo |
|------|---------------|----------------------|
| 0 — Bootstrap | Base implementada | Workspace Cargo, crates modulares, CI y documentación. |
| 1 — Parsers MSTS/OpenRails | Profundizado | `openrailsrs-formats`: AST genérico + adaptadores tipados (`EngineFile`, `WagonFile`, `ConsistFile`, `RouteFile`) + conversiones de unidades + dispatch por extensión. |
| 2 — Datos/config juego | Profundizado | `openrailsrs-scenarios`: `scenario.toml` con `[[route.stops]]` (paradas intermedias con `arrive_s`/`depart_s`) y `[train.davis]` (coeficientes Davis sobreescribibles). |
| 3 — Modelo lógico ferroviario | Profundizado | `openrailsrs-track`: señales (`Stop/Caution/Clear`, `clear_after_s`), `insert_signal`, `signals_on_edge`; runner `RunPhase::AwaitingSignal`; **agujas funcionales** (`SwitchPosition::Straight/Diverging`, `set_switch`, `switch_position`, error `NotASwitch`); BFS respeta la posición de cada `NodeKind::Switch`; `default_position` en `track.toml` y `[[switches]]` sobreescribibles por escenario. |
| 4 — Modelo físico del tren | Profundizado | `DavisCoefficients`; `TractiveCurve` piecewise-linear; **`regen_factor`** (frenado regenerativo, 0.70 en CAF 6000) y **`diesel_sfc_g_per_kwh`** (consumo diésel); CSV exporta `regen_energy_kwh` + `fuel_consumption_l`. |
| 5 — Simulación headless | Profundizado | `physics::step` usa `TractiveCurve` o P/v; máquina de estados `Normal→Approaching→Dwelling→AwaitingSignal`; `ScriptedDriver`; `run_from_scenario_file_with_driver`. |
| 6 — Capa de videojuego headless | Profundizado | `evaluate` multi-parada: `missed_stop`, penalización **graduada** (`penalty_per_second_late` pts/s de retraso), `early_departure`; `PlayOutcome` añade `punctuality_pct` y `total_delay_s`; `play-headless` imprime timeline completo + tabla de paradas; `outcome.toml` con desglose. |
| 7 — Validación/comparación | **Profundizado** | `openrailsrs-validate`: `ValidationConfig` con tolerancias por columna (`max_velocity_rms`, `max_position_max`, etc.); `ComparisonReport` con `pass`/`fail` por columna y global; CLI `compare` con 6 flags de umbral y exit code 1 si falla. |
| 8 — Debug sin gráficos | Profundizado | `openrailsrs-export`: DOT/GeoJSON/ASCII + **replay animado** (`animated_replay_from_csv`: barra de progreso ANSI, refresco in-place, velocidad configurable). |
| 9 — Optimización | **Profundizado** | `PathData` pre-computa `Vec<PathEdgeData>` antes del bucle → `physics::step` usa indexación directa (sin `HashMap::get` por tick); benchmarks Criterion: micro, escenario completo, multi-tren. |
| 10 — Viewer 2D animado | **Profundizado** | `openrailsrs-viewer`: topología + señales con aspecto real (rojo/amarillo/verde), **replay multi-tren animado** desde CSV, HUD con t, velocidad por tren, barra de progreso, controles teclado. |
| 13 — Importar rutas reales | **Implementado** | `openrailsrs-import`: Overpass JSON (OSM) → `track.toml`; junctions automáticos, Haversine, proyección equirectangular, estaciones, speed limit desde tag `maxspeed`; aristas bidireccionales por defecto. Línea Mitre (Buenos Aires) en `examples/routes/mitre/`. |
| 11/14 — Escenario real + Modo cabina | **Implementado** | Escenarios: Retiro→Victoria (22.9 km), Retiro→Tigre (28.1 km, con nodos sintéticos); CAF 6000 con regen 70%; `openrailsrs cab` — HUD: velocidad/límite, acelerador/freno, energía+regen, **próxima parada + tiempo restante + penalizaciones acumuladas**. |
>
> **Campaña**: `openrailsrs campaign status examples/mitre_campaign/campaign.toml`
>
> **Despacho**: `openrailsrs dispatch examples/routes/mitre/scenario_retiro_victoria.toml --speed 20`
| 12/15 — Panel de despacho + Campaña | **Implementado** | `openrailsrs-campaign`: 5 misiones progresivas (incluye **servicio duplo**); unlock por score; `progress.json`; `openrailsrs dispatch` — TUI ratatui **multi-tren** (`LiveMultiSim`): tabla por tren (estado, v, energía neta, regen), log de bloqueos y llegadas. |
| Cuatro mejoras combinadas | **Implementado** | D: regen+diésel en model físico; C: brecha Victoria→Tigre cerrada + escenario Retiro→Tigre 28 km; B: HUD puntualidad en cab mode; A: `LiveMultiSim` + dispatch multi-tren + misión duplo. |

> Nota: “Base implementada” significa línea base funcional; la **profundidad futura** de cada fase sigue evolucionando en iteraciones.
>
>
> **Fase 13**: `openrailsrs import-osm overpass.json --out routes/myroute/track.toml`. Ver [`examples/osm/`](examples/osm/).
>
> **Modo cabina**: `openrailsrs cab examples/routes/mitre/scenario_retiro_victoria.toml --speed 10`.

### Principios

- El núcleo corre **sin gráficos**; la simulación no depende de rendering.
- **Linux-first**, Rust estable.
- Datos de serie temporal en **CSV**; escenarios, configuración y metadata en **TOML**.
- El núcleo y la CLI **no** dependen de Bevy/wgpu; el viewer 2D está en `openrailsrs-viewer` (Fase 10) y el viewer 3D experimental en `openrailsrs-viewer3d` (Fase 23 / issue #8).
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
| `openrailsrs-viewer3d` | Binario **`openrailsrs-viewer3d`**: grafo 3D desde `track.toml` (aristas cilindro o gizmo compact, nodos esfera, señales coloreadas) + plano/grilla + cámara orbit/fly + follow (`T`); ver `docs/OPEN_RAILS_VIEWER_3D.md`. |

Los módulos públicos en Rust siguen el patrón `openrailsrs_<crate>::…` (p. ej. `openrailsrs_sim::run_from_scenario_file`).

---

## Requisitos

- Rust estable (edition 2024, `rust-version` en workspace).
- Linux (el viewer 2D usa `minifb` con feature `x11`; el viewer 3D usa Bevy/winit y en CI se instala también `libxkbcommon-dev`).

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

### Viewer 3D experimental (Bevy, Fase 23 / issue #8)

```bash
# Solo topología (grafo estático)
cargo run -p openrailsrs-viewer3d
cargo run -p openrailsrs-viewer3d -- examples/smoke/routes/test

# Grafo + tren animado desde CSV de simulación
cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml   # genera run.csv
cargo run -p openrailsrs-viewer3d -- examples/smoke/scenario.toml
```

Muestra el **grafo lógico** de la ruta en 3D:

- **Aristas** — cilindros naranjas entre nodos en rutas pequeñas (≤800 aristas); en rutas grandes (p. ej. Mitre) **modo compact**: líneas naranjas vía gizmos (arranque más rápido).
- **Nodos** — esferas: blanco (Plain), cian (Switch), amarillo (Station); en modo compact solo Switch/Station.
- **Señales** — diamantes coloreados como en el viewer 2D: rojo (`stop`), amarillo (`caution`), verde (`clear`), con poste.
- **Tren** (con `scenario.toml`) — cubo magenta que recorre la ruta según `run.csv`.
- **HUD** — franja inferior (~60 px): título, estado replay (tiempo, km/h, barra de progreso, leyenda de trenes), modo cámara/follow y atajos.
- **Objetos `.w`** — si la ruta tiene carpeta `WORLD/` con tiles MSTS, cubos coloreados por tipo (`Static`, `Forest`, …) en posición global (tile × 2048 m + local); `Static` con `.s` en `SHAPES/` usa mesh MSTS real.
- **Plano + grilla** — centrados y escalados al bounding box de la ruta.
- **Cámara orbit** — encuadra la ruta al abrir; zoom máximo adaptado a rutas grandes (p. ej. Mitre OSM).

Controles:

- `F1` / `F2`: cámara **orbit** / **fly** (WASD en plano horizontal, `Q`/`E` arriba/abajo; con replay cargado, `Space` pausa en lugar de subir en fly).
- Orbit: botón derecho rotar, botón del medio pan, rueda zoom.
- Fly: botón derecho mantenido para mirar (cursor oculto y confinado a la ventana); `Shift` acelera ×4, `Ctrl` ralentiza ×0.25.
- Replay: `Space` pausar/reanudar · `R` reiniciar · `+`/`-` velocidad.
- **`T`** (con replay activo): ciclo **follow** off → orbit follow (foco en el tren) → chase cam (detrás del tren); pan con botón medio desactiva follow.
- `Esc`: salir.

Rutas grandes:

```bash
cargo run -p openrailsrs-viewer3d -- examples/routes/mitre   # modo compact (~5k aristas)
```

Siguiente hito del plan: **textura `.ace` en material** (orden 7 en `docs/OPEN_RAILS_VIEWER_3D.md`).

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

### Modo cabina (`cab`) — Fase 11

Conduce el tren en tiempo real desde la terminal. El simulador corre a velocidad configurable (por defecto 10×) para que el maquinista sienta la inercia real sin esperar 30 minutos.

```bash
openrailsrs cab examples/routes/mitre/scenario_retiro_victoria.toml --speed 10
```

**Controles:**

| Tecla | Acción |
|-------|--------|
| W / ↑ | Aumentar acelerador (+10%) |
| S / ↓ | Reducir acelerador / aplicar freno |
| Espacio | Freno de emergencia (freno al 100%) |
| Q / Esc | Salir |

**HUD en pantalla completa:**

```
 openrailsrs — MODO CABINA — Línea Mitre — Retiro → Victoria
 ─────────────────────────────────────────────
 Velocidad      78.3 km/h   límite    90 km/h
 Acelerador  [████████████        ]  60%
 Freno       [                    ]   0%
 Recorrido   [▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░]  8.7 km / 22.9 km  (38.0%)
 Tiempo sim     312 s       Energía  8.712 kWh
 ─────────────────────────────────────────────
 W/↑ acelerar   S/↓ freno   Espacio=freno emergencia   Q=salir
```

La carpeta `examples/routes/mitre/` incluye la **Línea Mitre real** importada de OpenStreetMap (2133 nodos, 4926 aristas) y el consist **CAF 6000** (EMU eléctrico de 6 coches, 270 t, 900 kW) para el trayecto Retiro → Victoria.

---

### Panel de despacho (`dispatch`) — Fase 12

Monitor en tiempo real con TUI completa. El tren corre automáticamente (throttle al 100%) mientras la pantalla se actualiza.

```bash
openrailsrs dispatch examples/routes/mitre/scenario_retiro_victoria.toml --speed 20
```

```
 openrailsrs DISPATCH  •  Línea Mitre — Retiro → Victoria  •  t=245s  •  20×
┌ Trenes en servicio ────────────────────────────────────────────────────────────────┐
│ Tren         Estado       Velocidad  Límite    Odómetro   Progreso    Energía      │
│ CAF-6000 #1  EN SERVICIO   84.2 km/h  90 km/h  4 821 m  [▓▓▓▓░░…]   6.14 kWh    │
└────────────────────────────────────────────────────────────────────────────────────┘
┌ Log de eventos ────────────────────────────────────────────────────────────────────┐
│  Arista: e_n1618345519_n…  312m  lím 90km/h → Belgrano C                          │
│  Arista: e_n6463425690_n…  245m  lím 90km/h → Núñez                               │
└────────────────────────────────────────────────────────────────────────────────────┘
  Espacio=pausa/reanudar   +/-=velocidad   Q/Esc=salir
```

### Motor de campaña (`campaign`) — Fase 15

Sistema progresivo de misiones con persistencia de progreso.

```bash
# Ver estado de la campaña Mitre
openrailsrs campaign status examples/mitre_campaign/campaign.toml

# Jugar una misión
openrailsrs campaign play examples/mitre_campaign/campaign.toml tutorial

# Reiniciar progreso
openrailsrs campaign reset examples/mitre_campaign/campaign.toml
```

```
  🚆  Línea Mitre — Operador Ferroviario

  ID                Nombre                          Estado        Score    Dificultad
  ────────────────────────────────────────────────────────────────────────
  tutorial          Tutorial — Primer servicio       ✅ completada  100/100 ⭐  Easy
  retiro_olivos     Retiro → Olivos                  ▶ disponible   —          Easy
  retiro_san_isidro Retiro → San Isidro C            🔒 bloqueada   —          Medium
  retiro_victoria   Retiro → Victoria (completo)     🔒 bloqueada   —          Hard
```

El archivo `progress.json` persiste el mejor score de cada misión entre sesiones. Una misión se desbloquea cuando se completa la anterior con un score ≥ `min_pass_score` (configurable en `campaign.toml`).

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

## Hoja de ruta hacia OpenRails — próximas fases

Las fases siguientes cierran la brecha entre el simulador headless actual y un simulador ferroviario completo comparable a OpenRails.  
Ordenadas de **menor a mayor dificultad** para facilitar la priorización.

---

### Fase 16 — Carga de pasajeros y masa variable `✅`

**Dificultad:** ⭐ Fácil (días)

- `passengers_on`/`passengers_off` por parada en `StopDef`; `max_capacity` en `TrainSection`.
- `passengers: u32` + `extra_mass_kg: f64` en `TrainSimState`; actualizados en cada `StationDeparture`.
- `step()` usa `effective_mass = train.mass_kg + state.extra_mass_kg` → física variable.
- HUD de cabina muestra "Pasajeros N / capacidad (+X kg)".
- Columna `passengers` en CSV de salida.

---

### Fase 17 — Audio básico `✅`

**Dificultad:** ⭐⭐ Fácil-media (semanas)

- [`rodio`](https://crates.io/crates/rodio) 0.21 con ondas sinusoidales generadas en tiempo real (sin archivos externos).
- `AudioEngine::try_start()` — CI-safe: devuelve `None` si no hay dispositivo de audio.
- Sonidos: motor (volumen proporcional a velocidad), frenos (volumen al brake), bocina (`H`).
- Thread dedicado recibe comandos vía `mpsc::channel`.

---

### Fase 18 — Timetable completo (red multi-tren) `✅`

**Dificultad:** ⭐⭐ Fácil-media (semanas)

- Archivo `timetable.toml` con `[[trains]]`: `id`, `consist`, `start`, `destination`, `depart_s`.
- `LiveMultiSim::from_timetable(path)` — carga N agentes desde el timetable sobre el grafo compartido.
- `openrailsrs timetable run <timetable.toml>` imprime tabla de resultados + métricas de red.
- Métricas: trenes llegados, bloqueos totales, energía media, tiempo total.
- Ejemplo: `examples/mitre_timetable.toml` (4 servicios Retiro → Victoria).

---

### Fase 19 — Física de frenos avanzada (freno de aire) `✅`

**Dificultad:** ⭐⭐⭐ Media

- `BrakeSystem` con `BrakeCylinder` por vehículo: presión viaja a ~200 m/s por la tubería.
- Estados por cilindro: `Charged`, `Applying`, `Applied`, `Releasing` con rampa configurable.
- Retardo de propagación: vagón trasero (30 m) frena ~0.15 s después del frontal (verificado en test).
- `physics::step()` usa `brake_system.total_force_n()` cuando hay cilindros; fallback escalar para legado.
- `runner.rs` y `multi_runner.rs` construyen `BrakeSystem` desde el consist al cargar la simulación.
- Activado en el ejemplo Mitre y el escenario de prueba smoke.

---

### Fase 20 — Dinámica de enganche (coupler forces) `✅`

**Dificultad:** ⭐⭐⭐ Media

- `coupler.rs`: `VehicleState` (velocidad/posición individual), `CouplerState` (rigidez, amortiguación, holgura, fuerza de ruptura).
- `multi_body_step()`: solver de resorte-amortiguador entre vehículos adyacentes; retorna velocidad media ponderada.
- `TrainSimState` tiene `vehicles`, `couplers` y `vehicle_masses`; vacíos → modo de masa puntual (compatible).
- `physics::step()` delega al solver multi-cuerpo si `state.vehicles` no está vacío.
- Test verifica que el vagón arranca después de la locomotora (holgura inicial de 0.05 m).

---

### Fase 21 — Editor de rutas interactivo `🔲`

**Dificultad:** ⭐⭐⭐ Media (1-2 meses)

- `openrailsrs edit <route_dir>` abre el viewer 2D en modo edición (crate `openrailsrs-viewer`).
- Click izquierdo: agregar nodo; drag entre nodos: agregar arista.
- Panel de propiedades lateral: editar `length_m`, `speed_limit_kmh`, `grade_percent`, `name`.
- Colocar señales y agujas visualmente con teclas de acceso rápido.
- Guardar directamente al `track.toml` existente; soporta deshacer (undo stack).

---

### Fase 22 — Señalización dinámica con scripts TOML `✅`

**Dificultad:** ⭐⭐⭐⭐ Alta

- `SignalScript` añadido a `TrackSignal`: reglas `on_block_ahead`, `on_second_block_ahead`, `default`.
- `TrackGraph::evaluate_signals(block_map)` evalúa todos los scripts y muta los aspectos con prioridad (Stop > Caution > Clear).
- `runner.rs` llama `evaluate_signals` cada ~1 s real de simulación; sincroniza `signal_runtime`.
- `multi_runner.rs` evalúa con el `block_map` multi-tren real (todos los edges ocupados).
- Formato `track.toml` extendido: `[signals.script]` inline, retrocompatible (sin script = señal estática).
- 4 tests unitarios en `openrailsrs-track/tests/signal_script.rs` cubren todos los casos.
- Base para ETCS/UEPFP en fases futuras.

---

### Fase 23 — Viewer 3D con Bevy `🔲`

**Dificultad:** ⭐⭐⭐⭐ Alta (3-4 meses)

- Integrar [Bevy](https://bevyengine.org/) como renderer desacoplado del sim headless.
- El sim sigue corriendo en `openrailsrs-sim` (sin cambios); Bevy lo llama como sistema ECS cada frame.
- Cargar `track.toml` → generar splines 3D de vía con peralte y gradiente.
- Material rodante 3D desde modelos GLTF/OBJ; cámara libre y seguimiento de tren.
- HUD Bevy con velocímetro, barra de freno, mapa mini.
- Primer paso hacia compatibilidad con contenido MSTS (texturas, modelos).

---

### Fase 24 — Tracción vapor `✅`

**Dificultad:** ⭐⭐⭐⭐⭐ Muy alta

**Implementado:**

- **`SteamParams`** en `openrailsrs-train`: cilindros (n, bore, stroke), rueda motriz, presión de caldera, evaporación, consumo de carbón, agua/carbón inicial.
- **`BoilerState`** en `openrailsrs-sim::steam`: presión, agua y carbón mutables; se inicializa desde `SteamParams` al arrancar la simulación.
- **`steam_step()`**: fórmula `F_te = n × (π/4) × bore² × stroke × P_mep / r_wheel`; dinámica de caldera (ODE de primer orden: balance supply/demand); consumo de agua y carbón; inyector automático (headless) que repone agua cuando cae al 30 %.
- **`physics.rs`**: rama condicional — si `train.steam_params.is_some()` usa `steam_step`, si no usa P/v o curva explícita. Retrocompatible: simulaciones existentes no se ven afectadas.
- **CSV extendido**: columnas `boiler_pressure_bar`, `water_kg`, `coal_kg` se añaden automáticamente cuando la locomotora es vapor.
- **Loader TOML nativo** (`steam_loader.rs`): formato `[engine] + [steam]` para definir locos vapor sin depender del parser MSTS. Detección automática por primer carácter (`[` = TOML, `(` = MSTS S-expr).
- **Ejemplo completo**: `examples/steam/` — loco 2-8-0 Consolidation (16 bar, ~155 kN), consist, ruta 50 km con parada intermedia, `scenario.toml`.
- **11 tests** en `steam_physics.rs`: fuerza en arranque, escala con regulador, dinámica de presión, consumo de agua y carbón, inyector automático, loader TOML.
- *Pendiente*: sonido sincronizado con golpes de émbolo (Fase 17 ampliada).

---

### Fase 25 — Compatibilidad con contenido MSTS / Open Rails `✅`

**Dificultad:** ⭐⭐⭐⭐⭐ Muy alta

**Implementado:**

- Nuevo crate `openrailsrs-msts` con importadores de rutas y actividades MSTS.
- **`openrailsrs-formats`** profundizado:
  - `EngineFile` ahora parsea `MaxTractiveEffortCurves` → `traction_curve: Vec<(f64, f64)>`.
  - `WagonFile` parsea `Length` → `length_m: f64` (default 15 m).
  - Nuevos parsers: `TrackDbFile` (`.tdb`), `PathFile` (`.pat`), `ActivityFile` (`.act`).
- **`import_route`**: convierte un `.tdb` a `track.toml` (nodos End/Junction/Vector → nodes+edges).
- **`import_activity`**: convierte `.act` + `.pat` a `scenario.toml` listo para `openrailsrs sim`.
- **CLI**: `openrailsrs import-msts <route_dir> [--out-dir …] [--activity …]` — auto-detecta `.act`.
- **`openrailsrs-train`**: `From<EngineFile> for Locomotive` propaga la curva de tracción real.
- Tests con fixtures mínimas `minimal.tdb / .pat / .act`; todos los tests del workspace pasan.
- *Pendiente (iteración futura)*: rutas reales completas; binary tokenized `.s` y resolución global de tiles `.w` (Fase 23).

---

### Fase 25b — Compatibilidad MSTS completa `✅`

**Dificultad:** ⭐⭐⭐⭐⭐ Muy alta (iteración continua)

Esta fase documenta los **gaps que quedan** para que `openrailsrs import-msts` pueda procesar una ruta MSTS/Open Rails real sin intervención manual.  La Fase 25 implementó la base (topología + actividad del jugador); la 25b cubre todo lo demás.

#### 1. Encoding de archivos `✅`

Los archivos MSTS reales usan **BOM UTF-16-LE** (la mayoría de rutas con editor de MSTS) o **Latin-1 / Windows-1252** (rutas antiguas europeas).

**Implementado en Fase 25b:**
- `openrailsrs-formats/src/encoding.rs` — `read_msts_file_to_string()` y `decode_msts_bytes()`.
- Detección automática por BOM: `FF FE` → UTF-16-LE, `FE FF` → UTF-16-BE, `EF BB BF` → UTF-8 (strip BOM).
- Fallback a Windows-1252 si no hay BOM y existen bytes `> 0x7F`.
- Integrado en `dispatch.rs`, `track_db.rs`, `path.rs` y `activity.rs`.
- 11 tests en `tests/encoding.rs` incluyendo fixtures binarios `.eng` en UTF-16-LE y Windows-1252.

#### 2. Señales desde `TrItemTable` `✅`

Las señales en un `.tdb` real no viven como nodos independientes sino en la sección `TrItemTable` (lista de `TrItem` con tipo `Signal`, posición en el track como `(TrItemId, position_m_on_vector)` y estado inicial).

**Implementado en Fase 25b:**

- `TrackDbFile.items: Vec<TrItem>` con variantes `Signal { aspect_initial }` y `Other`.
- Cada `TrVectorNode` parsea su sección `TrItemRefs` para mapear ítems a edges.
- `import_route` emite `[[signals]]` con `id="sig{TrItemId}"`, `edge_id="e{vector_node_id}"`, `position_m` desde `TrItemSData` y aspecto inicial (`Stop` por defecto, configurable vía `(InitialAspect …)`).
- Tests: `parse_tritem_table_extracts_signal`, `import_route_emits_signals_section`, `import_route_without_signals_omits_section`.

#### 3. `TrafficService` y paths múltiples `✅`

Las actividades reales incluyen trenes de tráfico AI (`Tr_Activity_Service_Definition` / `Service_Definition`) con horarios propios y paths separados.

**Implementado en Fase 25b:**

- `ActivityFile.services: Vec<TrafficServiceDef>` con `name`, `path_file`, `consist` opcional y `start_time_s` (`Service_Init_Time`).
- `import_activity` carga el `.pat` de cada servicio y emite una entrada `[[extra_trains]]` con `start`, `destination`, `start_time_s` y `output_csv = "run_{id}.csv"`.
- Servicios sin `.pat` resoluble se omiten silenciosamente (parser tolerante).
- Tests: `parse_activity_collects_traffic_services`, `import_activity_emits_extra_trains_from_traffic`.

#### 4. Eventos y restricciones de actividad `✅`

Secciones ahora soportadas por el importer:

| Sección MSTS | Equivalente openrailsrs | Estado |
|---|---|---|
| `ActivityObject` (cargas a recoger/dejar) | `[[route.stops]]` con `passengers_on/off` | ✅ |
| `FailedSignals` (señales averiadas) | `[[signals]] aspect="stop"` forzado | ✅ |
| `RestrictedSpeedZones` | `speed_limit_mps` mínimo por edge tocado | ✅ |
| `SoundRegions` | `SoundSourceItem` en TDB + overrides en `.act` → `[[sound_regions]]`; cabina + crate `openrailsrs-audio` | ✅ |
| `StartTime` + `Season` | `[scenario] start_time_s` + `season` | ✅ |

`import_route_with_activity(route_dir, act_path)` aplica `FailedSignals` y `RestrictedSpeedZones` al `track.toml` resultante; `import_activity` proyecta cada `ActivityObject` al endpoint más cercano del vector node que contiene su `TrItem`. La metadata `[scenario].start_time_s` y `[scenario].season` se popula automáticamente desde `(StartTime …)` y `(Season …)` cuando están presentes.

#### 5. Assets visuales (`.s` / `.ace`) `✅` (offline)

- **`.s` Shape files** ✅: parser ASCII en `openrailsrs-formats` (`ShapeFile`: puntos, normales, UVs, `prim_states`, primitivas, LODs, jerarquía).  La variante "binary tokenized" devuelve `FormatError::UnsupportedBinaryShape` (queda para Fase 23).  CLI: `openrailsrs shape-dump <file.s> [--json]`.
- **`.ace` Textures** ✅: crate nuevo `openrailsrs-ace` con decoder de mip 0 (RGBA8 + DXT1/3/5 vía `texpresso`) y `write_png`.  CLI: `openrailsrs ace-decode <file.ace> <out.png>`.  Mips superiores, BGRA→RGBA con flag y formatos extra → Fase 23.

Estos parsers son **headless puros**: no requieren Fase 23 y dejan listos los datos para que el viewer 3D los consuma cuando llegue.

#### 6. World files (`.w`) `✅` (offline)

Tiles de terreno (`~2km × 2km`).  Parser ASCII en `openrailsrs-formats` (`WorldFile`: `Static`, `Forest`, `TrackObj`, `Signal`, `Dyntrack`, `Other`) preservando posiciones locales del tile.  CLI: `openrailsrs world-dump <file.w> [--csv <out.csv>]`.  La resolución a coordenadas globales (TileX/TileZ → mundo) queda para Fase 23.

#### 7. Paths múltiples en actividades complejas

Algunas actividades referencian varios `.pat` (itinerarios alternativos para AI).  El importer actual carga solo el primero.

- Iterar sobre todos los `TrActivity_PathFile` en el `.act`.
- Asociar cada path al servicio AI correspondiente.

#### Resumen de compatibilidad actual

```
openrailsrs import-msts <ruta_msts>

✅ Topología de vía (nodos, aristas, longitudes, velocidades)
✅ Consist del jugador (.con → .eng + .wag)
✅ Curvas de tracción de locomotoras
✅ Path del jugador (.pat → start/destination en scenario.toml)
✅ Hora de inicio y duración de la actividad

✅ Encoding: UTF-16-LE/BE (BOM), Windows-1252 y UTF-8 — automático
✅ Señales (TrItemTable → [[signals]] con edge_id + aspecto inicial)
✅ Tráfico AI: TrafficService + .pat múltiples → [[extra_trains]]
✅ Eventos de actividad: FailedSignals, RestrictedSpeedZones, ActivityObject
✅ Metadata: StartTime → [scenario].start_time_s, Season → [scenario].season
⚠️  Rutas con cientos de nodos: funciona pero sin validación de integridad
✅  Shapes `.s` ASCII: parser (`ShapeFile`) + `openrailsrs shape-dump [--json]` (binary tokenized → error explícito; pendiente Fase 23)
✅  Texturas `.ace`: decoder mip 0 (RGBA8 + DXT1/3/5) + `openrailsrs ace-decode <in> <out.png>` (mips superiores → Fase 23)
✅  Tiles `.w` ASCII: parser (`WorldFile`) + `openrailsrs world-dump [--csv]` (Static / Forest / TrackObj / Signal / Dyntrack; coords globales → Fase 23)
✅  SoundRegions: import MSTS (`import_activity` + TDB) y runtime en modo cabina vía `openrailsrs-audio` (`EnterRegion` / `LeaveRegion`)
```

---

### Fase 26 — Multijugador `🔲`

**Dificultad:** ⭐⭐⭐⭐⭐ Muy alta (6+ meses)

- Arquitectura cliente-servidor: servidor autoritativo corre `LiveMultiSim`, clientes sincronizan estado.
- Protocolo: WebSocket + mensajes binarios (serde + bincode).
- Roles: conductor (controla un tren), dispatcher (controla señales/agujas), observador.
- Tolerancia a desconexiones: el servidor toma el control del tren con `AutoDriver` si el cliente cae.
- Base en `openrailsrs-sim` ya soporta multi-tren y block occupancy; el networking es la capa nueva.

---

### Fase 27 — IA de despacho `🔲`

**Dificultad:** ⭐⭐⭐⭐⭐ Muy alta (6+ meses)

Scheduler global que coordina todos los trenes en la red con visión completa del estado del grafo.

- **Modelo de conflictos**: representar el grafo de ocupación como un problema de satisfacción de restricciones (CSP); detectar deadlocks antes de que ocurran (p.ej. dos trenes bloqueándose mutuamente en un tramo de vía única).
- **Algoritmo de scheduling**: variante de *job-shop scheduling* adaptada a grafos ferroviarios; objetivo: minimizar retrasos totales ponderados (prioridad por tipo de servicio — larga distancia > regional > carga).
- **Resolución de conflictos en tiempo real**: al detectar un conflicto inminente, el dispatcher IA propone una de tres acciones: (1) detener un tren en el próximo desvío libre, (2) invertir el orden de paso, (3) rerouting por vía alternativa.
- **Integración con el sim**: `DispatcherAI` implementa una interfaz similar a `Driver`; se conecta al `LiveMultiSim` como capa de control por encima de los conductores individuales. El conductor humano puede anular decisiones del AI.
- **Interfaz de visualización**: panel `ratatui` con tabla de conflictos activos, propuesta de resolución, y tiempo estimado de retraso evitado.
- **Entrenamiento / calibración**: ejecutar miles de simulaciones headless con el scheduler para ajustar los pesos de prioridad; exportar métricas a CSV para análisis.
- **Prerequisitos**: Fase 22 (señalización dinámica), Fase 18 (timetable), y un grafo con al menos 2 vías paralelas o lazos de evitación.

---

## Consists y rutas de assets

Las rutas en `(Engine "…")` y `(Wagon "…")` se resuelven respecto al **directorio del escenario** (carpeta que contiene `scenario.toml`), no respecto a la subcarpeta `consists/`, para alinear el layout con un “directorio de instalación” del escenario.

## Licencia

Este proyecto se distribuye bajo la **GNU General Public License v3.0 only** (SPDX: `GPL-3.0-only`). Ver el texto completo en [LICENSE](LICENSE).

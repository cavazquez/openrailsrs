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
| 3 — Modelo lógico ferroviario | Profundizado | `openrailsrs-track`: señales ferroviarias (`TrackSignal` con `id`, `aspect`, `clear_after_s`), `insert_signal`, `signals_on_edge`; `track.toml` con sección `[[signals]]`; runner respeta `Stop` (`RunPhase::AwaitingSignal`, auto-despeje por tiempo) y `Caution` (speed limit ×0.5); `SimEvent::SignalStop/SignalClear`. |
| 4 — Modelo físico del tren | Profundizado | `DavisCoefficients` configurable en `Consist`; `TractiveCurve` (puntos v→F, interpolación piecewise-linear) en `Locomotive`; `TrainPhysics` agrega la curva o sintetiza una desde P/F_te. |
| 5 — Simulación headless | Profundizado | `physics::step` usa `TractiveCurve` si existe, P/v como fallback; máquina de estados `Normal→Approaching→Dwelling` para frenos de aproximación dinámicos y dwell real en paradas; `ScriptedDriver` permite replay desde CSV (`time_s,throttle,brake`, semántica hold-last). |
| 6 — Capa de videojuego headless | Profundizado | `evaluate` con multi-parada: penaliza `missed_stop`, `late_stop`, **`early_departure`** (`StationDeparture.time_s < depart_s − GRACE`); `StopResult` incluye `actual_depart_s` y flag `early_departure`. |
| 7 — Validación/comparación | Base implementada | `openrailsrs-validate` + comando `compare`. |
| 8 — Debug sin gráficos | Base implementada | `openrailsrs-export` (DOT/GeoJSON/ASCII/replay). |
| 9 — Optimización | Base implementada | benchmark Criterion + batch con `rayon`. |
| 10 — Viewer mínimo | Base implementada | `openrailsrs-viewer` 2D desacoplado del núcleo. |

> Nota: “Base implementada” significa línea base funcional; la **profundidad futura** de cada fase sigue evolucionando en iteraciones.

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
| `openrailsrs-scenarios` | Carga/validación de `scenario.toml`; soporta paradas intermedias (`[[route.stops]]`) y override de Davis (`[train.davis]`). |
| `openrailsrs-route` | Carga de layout de vía (`track.toml`) con `grade_percent` por arista y sección `[[signals]]`. |
| `openrailsrs-track` | Grafo de vía, nodos, aristas, límites de velocidad, pendientes y señales (`TrackSignal`: `Stop/Caution/Clear`, `clear_after_s` para auto-despeje). |
| `openrailsrs-train` | Locomotoras, vagones, consists; `DavisCoefficients` y `TractiveCurve` (curva de tracción real, piecewise-linear) configurables. |
| `openrailsrs-sim` | Bucle headless con física configurable (`TrainPhysics` + `TractiveCurve`); dwell en paradas; señales `Stop/Caution` aplicadas en `RunPhase`; `ScriptedDriver` para replay desde CSV; `SimEvent` con overspeed/estaciones/señales; salida `run.csv` + `run.toml`. |
| `openrailsrs-game` | Objetivos, penalizaciones multi-parada (`missed_stop`, `late_stop`, `early_departure`), puntuación; `outcome.toml` con desglose por parada (`play-headless`). |
| `openrailsrs-validate` | Comparación cuantitativa de dos `run.csv`. |
| `openrailsrs-export` | DOT, GeoJSON, mapa ASCII, replay textual. |
| `openrailsrs-cli` | Binario **`openrailsrs`**. |
| `openrailsrs-viewer` | Binario **`openrailsrs-viewer`** (2D mínimo, opcional). |

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

```bash
# Fase 1 — inspeccionar AST genérico
cargo run -p openrailsrs-cli --bin openrailsrs -- inspect path/al/archivo.eng

# Fase 3 — exportar grafo DOT
cargo run -p openrailsrs-cli --bin openrailsrs -- graph examples/smoke/routes/test --out route.dot

# Fase 5 — simulación headless
cargo run -p openrailsrs-cli --bin openrailsrs -- sim examples/smoke/scenario.toml

# Fase 6 — partida headless (escribe outcome.toml)
cargo run -p openrailsrs-cli --bin openrailsrs -- play-headless examples/smoke/scenario.toml

# Fase 7 — comparar dos corridas
cargo run -p openrailsrs-cli --bin openrailsrs -- compare run1.csv run2.csv

# Fase 8 — GeoJSON, mapa ASCII, replay textual
cargo run -p openrailsrs-cli --bin openrailsrs -- export-geojson examples/smoke/routes/test --out track.geojson
cargo run -p openrailsrs-cli --bin openrailsrs -- ascii-map examples/smoke/routes/test
cargo run -p openrailsrs-cli --bin openrailsrs -- replay examples/smoke/run.csv

# Fase 9 — batch con rayon
cargo run -p openrailsrs-cli --bin openrailsrs -- batch examples/smoke/scenario.toml

# Logs (`tracing`) opcionales
cargo run -p openrailsrs-cli --bin openrailsrs -- -v sim examples/smoke/scenario.toml
```

### Viewer 2D (Fase 10)

```bash
cargo run -p openrailsrs-viewer --bin openrailsrs-viewer -- examples/smoke/routes/test
```

No acopla la simulación al render: solo lee `track.toml` y dibuja la topología.

## Benchmarks (Fase 9)

```bash
cargo bench -p openrailsrs-sim --bench sim_step
```

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

Para replay o test de regresión, se puede usar un `ScriptedDriver` cargando un CSV con columnas `time_s,throttle,brake` (el mismo formato que `run.csv`):

```bash
# Simulación headless con conductor scripted
cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml \
  --driver examples/smoke/driver_script.csv
```

---

## Consists y rutas de assets

Las rutas en `(Engine "…")` y `(Wagon "…")` se resuelven respecto al **directorio del escenario** (carpeta que contiene `scenario.toml`), no respecto a la subcarpeta `consists/`, para alinear el layout con un “directorio de instalación” del escenario.

## Licencia

Este proyecto se distribuye bajo la **GNU General Public License v3.0 only** (SPDX: `GPL-3.0-only`). Ver el texto completo en [LICENSE](LICENSE).

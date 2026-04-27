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
| `openrailsrs-formats` | Tokenizer + parser AST tipo S-exp (`.trk`, `.eng`, `.wag`, `.con`). |
| `openrailsrs-scenarios` | Carga y validación de `scenario.toml`. |
| `openrailsrs-route` | Carga de layout de vía (`track.toml` en carpeta de ruta). |
| `openrailsrs-track` | Grafo de vía, nodos, aristas, límites de velocidad. |
| `openrailsrs-train` | Locomotoras, vagones, consists (desde AST). |
| `openrailsrs-sim` | Bucle headless, salida `run.csv` + `run.toml`. |
| `openrailsrs-game` | Objetivos, penalizaciones, puntuación (`play-headless`). |
| `openrailsrs-validate` | Comparación cuantitativa de dos `run.csv`. |
| `openrailsrs-export` | DOT, GeoJSON, mapa ASCII, replay textual. |
| `openrailsrs-cli` | Binario **`openrailsrs`**. |
| `openrailsrs-viewer` | Binario **`openrailsrs-viewer`** (2D mínimo, opcional). |

Los módulos públicos en Rust siguen el patrón `openrailsrs_<crate>::…` (p. ej. `openrailsrs_sim::run_from_scenario_file`).

---

## Requisitos

- Rust estable (edition 2021, `rust-version` en workspace).
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

## Consists y rutas de assets

Las rutas en `(Engine "…")` y `(Wagon "…")` se resuelven respecto al **directorio del escenario** (carpeta que contiene `scenario.toml`), no respecto a la subcarpeta `consists/`, para alinear el layout con un “directorio de instalación” del escenario.

## Licencia

Este proyecto se distribuye bajo la **GNU General Public License v3.0 only** (SPDX: `GPL-3.0-only`). Ver el texto completo en [LICENSE](LICENSE).

<div align="center">

# openrailsrs

**Simulador ferroviario headless-first en Rust** — física y datos por consola (CSV + TOML), luego reglas de juego y viewers desacoplados.

[![CI](https://github.com/cavazquez/openrailsrs/actions/workflows/ci.yml/badge.svg)](https://github.com/cavazquez/openrailsrs/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/cavazquez/openrailsrs/graph/badge.svg)](https://codecov.io/gh/cavazquez/openrailsrs)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

</div>

## Qué es

Núcleo de simulación **sin gráficos** (Linux-first, Rust ≥1.95). CSV para series temporales; TOML para escenarios. Viewer 2D (`minifb`) y 3D (Bevy 0.19.1) en crates aparte. Referencia de paridad fijada en **Open Rails 1.6.1**; commit y hashes en [`oracles/openrails-reference.toml`](oracles/openrails-reference.toml).

Fases y prioridades: [`ROADMAP.md`](ROADMAP.md). Docs: [`docs/README.md`](docs/README.md).

## CI local

```bash
./check.sh   # fmt → clippy → tests → build
```

GitHub Actions: mismo `check.sh` + cobertura Codecov + visual smoke (xvfb/lavapipe).

## Inicio rápido

```bash
cargo build
cargo test
cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml
```

```bash
# Servicio completo: Northolt Park → South Ruislip → West Ruislip (7,06 km).
# Necesita el Content Chiltern instalado; CHILTERN_ROUTE permite cambiar su ubicación.
./scripts/run_chiltern_service.sh
./scripts/run_chiltern_service.sh --autodrive --cab

# La misma partida y conductor automático sin ventana.
target/debug/openrailsrs play-service examples/chiltern_local/scenario.toml --out-dir tmp/service

# Verifica versión/hashes y compara física con las capturas OR congeladas.
python3 scripts/run_oracles.py
```

`1` cabina, `Alt+1` alterna 2D/3D, `2` exterior; `W/S` inversor, `A/D` regulador, `;/'` freno, `Q` puertas, `Pause` pausa. `F5` información de conducción, `F4` monitor de vía. En cada estación: detenerse a ≤0,1 m/s dentro de ±10 m, abrir puertas, completar el embarque y cerrar puertas; el HUD indica cuándo salir. Detalles y límites de paridad: [`examples/chiltern_local/README.md`](examples/chiltern_local/README.md).

Guías: [`docs/CHILTERN.md`](docs/CHILTERN.md) · [`docs/VIEWER3D_TESTING.md`](docs/VIEWER3D_TESTING.md) · [`docs/BEVY.md`](docs/BEVY.md).

## CLI

```bash
cargo install --path crates/openrailsrs-cli   # binario `openrailsrs`

openrailsrs inspect path/file.eng
openrailsrs sim examples/smoke/scenario.toml
openrailsrs play-headless examples/smoke/scenario.toml
openrailsrs compare run1.csv run2.csv --max-velocity-rms 0.5
openrailsrs audit-vehicle examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng
```

Más subcomandos: `graph`, `export-geojson`, `replay --watch`, `cab`, `dispatch`, `campaign`, `import-osm`, `compare-or` — `openrailsrs --help`.

## Crates (resumen)

| Crate | Rol |
|-------|-----|
| `formats` / `scenarios` / `route` / `track` / `train` | Datos MSTS + grafo + consists |
| `sim` / `game` / `validate` / `export` / `cli` | Headless + CLI |
| `viewer` | Replay 2D |
| `or-shader` / `bevy-scenery` / `viewer3d` / `render3d` | Capa 3D Bevy — [`docs/BEVY.md`](docs/BEVY.md) |

## Notas de paridad visual (OR)

Bugs reales ya cubiertos por tests en `check.sh` (no “arreglar” a ojo):

- Terreno: patches 16×16 / 17×17; diagonal OR; UV `W/B/C/H`; no sumar `CenterX/Z` encima del placement local.
- Shapes: `prim_state_idx` intercalado con trilists; alpha por ShaderName/`AlphaTestMode`/ACE (no solo nombre “glass”).
- Tren live: no forzar LOD lejano cerca de cámara; forests: `TreeSize` del WORLD.

Física vs OR: [`docs/OR_PARITY.md`](docs/OR_PARITY.md). Trazas: [`docs/OR_TRACE_COMPARISON.md`](docs/OR_TRACE_COMPARISON.md).

## Licencia

GPL-3.0 — ver `LICENSE`.

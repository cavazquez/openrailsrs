# Roadmap openrailsrs

Simulador ferroviario **headless-first** (Rust, Linux-first, CSV/TOML). Viewer 3D desacoplado (Bevy).

| Símbolo | Significado |
|---------|-------------|
| ✅ | Hecho |
| 🔶 | Base / profundizable |
| 🔲 | Planeado |

## Fases producto (resumen)

| Fase | Tema | Estado |
|------|------|--------|
| 0 | Bootstrap / CI | ✅ |
| 1 | Parsers MSTS/OR | ✅ |
| 2 | Escenarios TOML | ✅ |
| 3 | Grafo vía / señales / agujas | ✅ |
| 4 | Física tren (Davis, tracción, regen) | ✅ |
| 5 | Sim headless + drivers | ✅ |
| 6 | Gameplay headless (paradas, score) | ✅ |
| 7 | Validación / compare | ✅ |
| 8 | Export / debug sin GPU | ✅ |
| 9 | Optimización PathData / benches | ✅ |
| 10 | Viewer 2D | ✅ |
| 11–12 | Cab mode + dispatch/campaña Mitre | ✅ |
| 13 | Import OSM | ✅ |
| **3D** | viewer3d / render3d / scenery | 🔶 ver [`docs/VIEWER3D.md`](docs/VIEWER3D.md) |

## Prioridades actuales

1. Paridad visual residual Chiltern ([`docs/VIEWER3D.md`](docs/VIEWER3D.md)).
2. Cabina CVF / rolling-stock ([`docs/CABVIEW3D.md`](docs/CABVIEW3D.md)).
3. Física OR ([`docs/OR_PARITY.md`](docs/OR_PARITY.md)) — multi_body, señales sin assume-clear.
4. Audio en viewer 🔲.

## Docs

Índice: [`docs/README.md`](docs/README.md). Chiltern: [`docs/CHILTERN.md`](docs/CHILTERN.md) · [`examples/chiltern/README.md`](examples/chiltern/README.md).

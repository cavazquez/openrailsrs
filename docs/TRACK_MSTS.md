# Vía MSTS — TDB / Track Viewer / TSRE5

Lecciones para alinear grafo lógico ↔ geometría TDB (no portar Track Viewer).

## Modelo

- **TDB:** nodos + *vector sections* con heading; arcos vía `tsection.dat`.
- **Track Viewer / TSRE5:** mismos espacios MSTS; útiles para validar a ojo.
- **openrailsrs:** grafo `track.toml` + pose TDB validada (≤25 m al grafo absoluto; luego offset de render).

## Lecciones clave

1. No teletransportar por ID `nNNNN` sin validar distancia.
2. Placement WORLD ↔ TDB en XZ absoluto + anillo 3×3 de tiles.
3. Señales TrItem: solo evaluar si el tile WORLD está cargado (`outside_coverage`).
4. Paths `.pat`: PDPs vs nodos; outliers Birmingham documentados en audits.
5. `--track-dev` / audit: comparar chords vs `FindLocationInSection` OR.

## Comandos

```bash
# Audits (viewer3d / CLI — ver flags en --help)
cargo test -p openrailsrs-viewer3d track_audit -- --nocapture
# Fixtures: docs/fixtures/smoke-track-audit-good.json
```

Coords: [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md). Testing: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

## TSRE5 (opcional)

Build/referencia externa para inspeccionar tiles; no es dependencia de runtime. Validación manual cerca de Birmingham (−6080, 14925).

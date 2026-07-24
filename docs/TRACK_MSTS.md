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
6. **`start_offset_m` = cabeza del consist** (#132), no `DistanceDownPath` ni cola OR. TrackPDP[0] es cola; conversión opcional `head_offset_from_rear_snap`.
7. **Pose por coche** (#128): cada vehículo samplea chainage absoluto TDB/grafo, incluido el offset inicial; no barra rígida ni offsets relativos al origen del path.
8. Un `eNNNN` del grafo solo reutiliza el vector TDB `NNNN` si sus extremos son cercanos; si no, se hace *nearest snap* desde la posición espacial del grafo.
9. `TrackPose` orienta `+Z` por la vía y el frame de tren usa `+X`: la rotación de vehículo debe componer ese cambio de base después del yaw/pitch/roll TDB.

## Comandos

```bash
# Audits (viewer3d / CLI — ver flags en --help)
cargo test -p openrailsrs-viewer3d track_audit -- --nocapture
# Fixtures: docs/fixtures/smoke-track-audit-good.json
```

Coords: [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md). Testing: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

## TSRE5 (opcional)

Build/referencia externa para inspeccionar tiles; no es dependencia de runtime. Validación manual cerca de Birmingham (−6080, 14925).

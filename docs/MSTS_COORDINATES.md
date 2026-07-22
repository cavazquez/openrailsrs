# Coordenadas MSTS → Bevy

| Espacio | Qué es | Uso |
|---------|--------|-----|
| **MSTS world** | Absoluto `.w` / TDB (~±10⁷ m) | Carga tiles, `RouteFocus` |
| **Render** | World − centro foco − `height_origin` (MSL ancla) | Clasificación WORLD, grafo |
| **View** | Render − floating origin (**solo XZ**) | Entidades Bevy, cámara/tren |

- **`height_origin`:** MSL terreno en ancla (~28 m Chiltern), no bbox Y del `.w`.
- Floating origin **no mueve Y** (altura ya normalizada).
- Snap TDB: `OPENRAILSRS_TDB_SNAP_RADIUS_M` (default 2500). Mapping grafo→TDB: ID validado ≤25 m al grafo **absoluto**.

Detalle de tests: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md). Vía: [`TRACK_MSTS.md`](TRACK_MSTS.md).

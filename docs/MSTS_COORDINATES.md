# Coordenadas MSTS → Bevy

| Espacio | Qué es | Uso |
|---------|--------|-----|
| **MSTS world** | Absoluto `.w` / TDB (~±10⁷ m) | Carga tiles, `RouteFocus` |
| **Render** | World − centro foco − `height_origin` (MSL ancla) | Clasificación WORLD, grafo |
| **View** | Render − floating origin (**solo XZ**) | Entidades Bevy, cámara/tren |

- **`height_origin`:** MSL terreno en ancla (~28 m Chiltern), no bbox Y del `.w`.
- Floating origin **no mueve Y** (altura ya normalizada).
- Snap TDB: `OPENRAILSRS_TDB_SNAP_RADIUS_M` (default 2500). Mapping grafo→TDB: ID validado ≤25 m al grafo **absoluto**.
- **WORLD pose:** `QDirection` → Quat Bevy; `Matrix3x3` → `linear: Mat3` XNA (puede incluir shear). Bevy `Transform` es solo TRS y **no** representa shear: esos Static se dibujan por GPU instancing aunque sea **una** instancia (#139). Sin eso, edificios pueden verse inclinados ~45°.

Detalle de tests: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md). Vía: [`TRACK_MSTS.md`](TRACK_MSTS.md).

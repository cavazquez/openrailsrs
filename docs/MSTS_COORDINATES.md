# Coordenadas MSTS → Bevy

| Espacio | Qué es | Uso |
|---------|--------|-----|
| **MSTS world** | Absoluto `.w` / TDB (~±10⁷ m) | Carga tiles, `RouteFocus` |
| **Render** | World − centro foco − `height_origin` (MSL ancla) | Clasificación WORLD, grafo |
| **View** | Render − floating origin (**solo XZ**) | Entidades Bevy, cámara/tren |

- **`height_origin`:** MSL terreno en ancla (~28 m Chiltern), no bbox Y del `.w`.
- Floating origin **no mueve Y** (altura ya normalizada).
- Snap TDB: `OPENRAILSRS_TDB_SNAP_RADIUS_M` (default 2500). Mapping grafo→TDB: ID validado ≤25 m al grafo **absoluto**.

## Precisión de posiciones WORLD

Una posición MSTS absoluta de Chiltern llega a `|X| ≈ 12,5 Mm` y
`|Z| ≈ 30,6 Mm`; un `f32` en ese rango sólo distingue pasos de aproximadamente
1–2 m. Las posiciones `.w` se indexan todavía en el espacio absoluto, pero
`WorldObject` conserva por separado el residuo de la conversión `f64→f32`.
Para renderizar se resta primero `RouteFocus` y recién entonces se suma ese
residuo. El orden es importante: sumarlo antes volvería a perderlo. Esto mantiene
unidos rieles consecutivos con placements submétricos.

Matrices / TRS vs shear: [`BEVY_TRANSFORMS.md`](BEVY_TRANSFORMS.md).

Detalle de tests: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md). Vía: [`TRACK_MSTS.md`](TRACK_MSTS.md).

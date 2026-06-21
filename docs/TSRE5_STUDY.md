# Estudio: TSRE5 como referencia MSTS/OR

TSRE5 ([`../../TSRE5/`](../../TSRE5/), Piotr Gadecki, **GPL-3.0**) es motor + editores de rutas MSTS/Open Rails. **No se redistribuye** dentro de openrailsrs; vive como carpeta hermana en el workspace.

**Regla:** estudiar comportamiento y layouts de datos; reimplementar en Rust siguiendo Open Rails (`FindLocationInSection`, etc.). **No** copiar C++ verbatim.

Variante CMake más nueva: [TSRE5vc](https://github.com/GokuMK/TSRE5vc).

Relacionado: [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md), [`THIRD_PARTY_REFERENCES.md`](THIRD_PARTY_REFERENCES.md), [`TRACKVIEWER_STUDY.md`](TRACKVIEWER_STUDY.md).

---

## Build (Linux)

```bash
cd TSRE5
qmake nbproject/qt-linux.pro
make -j$(nproc)
```

Configurar `TSRE5/settings.txt`: `gameRoot` → carpeta `Content` de MSTS/OR, `routeName` → subcarpeta de la ruta (p. ej. `Chiltern`).

---

## Mapa TSRE5 → openrailsrs

| Tema | TSRE5 | openrailsrs |
|------|-------|-------------|
| Tiles 2048 m, ±1024 local | `Game.cpp` `check_coords`, `GeoCoordinates.h` | [`coordinates.rs`](../crates/openrailsrs-or-shader/src/coordinates.rs) |
| Origen flotante | `CameraFree.cpp` (`pozT` + `playerPos`) | [`floating_origin.rs`](../crates/openrailsrs-viewer3d/src/floating_origin.rs) |
| Streaming escena | `Route.cpp` `tileLod` (±2 tiles default) | [`view_window.rs`](../crates/openrailsrs-viewer3d/src/view_window.rs) (~120 m) |
| Carga `.w` | `Tile.cpp` | `openrailsrs-formats` [`world.rs`](../crates/openrailsrs-formats/src/typed/world.rs) |
| Posición en vía | `TDB.cpp` `getDrawPositionOnTrNode` | [`tdb_track.rs`](../crates/openrailsrs-bevy-scenery/src/spawn/tdb_track.rs) `section_path_spans` |
| Snap a vía | `findNearestPositionOnTDB` | [`track_position.rs`](../crates/openrailsrs-viewer3d/src/track_position.rs) — señales, paradas, nodos grafo |
| TrItem ↔ TrackObj | `checkDatabase`, `fillWorldObjectsByTrackItemIds` | [`tr_item_audit.rs`](../crates/openrailsrs-viewer3d/src/tr_item_audit.rs), [`tr_item_index.rs`](../crates/openrailsrs-viewer3d/src/tr_item_index.rs), [`track_audit.rs`](../crates/openrailsrs-viewer3d/src/track_audit.rs), [`placement_audit.rs`](../crates/openrailsrs-viewer3d/src/placement_audit.rs) |
| Inicio de ruta | `Trk.cpp` `RouteStart` | [`route.rs`](../crates/openrailsrs-formats/src/typed/route.rs), overlay `world_anchor` |
| Merge rutas | `Route.cpp` `mergeRoute` | (pendiente) |
| Terreno costuras | `TerrainLibSimple.cpp` `fillRaw` | [`terrain_spawn.rs`](../crates/openrailsrs-viewer3d/src/terrain_spawn.rs) |

---

## Coordenadas y tiles

MSTS divide el mundo en tiles de **2048 m**. Posición local dentro del tile: **±1024 m** (centro en 0).

TSRE normaliza al cruzar límites (`Game::check_coords`): si `local_x >= 1024`, resta 2048 e incrementa `tile_x`.

Nombre de fichero `.w`: `w` + tile X con signo + tile Z con signo invertido (`Tile::getNameXY`, `Tile::load()`).

Distancia LOD de un objeto respecto al jugador:

```text
lodx = (obj_tile_x - playerT_x) * 2048 + obj.local_x - playerW_x
lodz = (obj_tile_z - playerT_z) * 2048 + obj.local_z - playerW_z
```

(`Tile.cpp` `pushRenderItems` — mismo espíritu que OR `PrepareFrame`.)

Detalle unificado: [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md).

---

## Origen flotante (TSRE vs openrailsrs)

| | TSRE5 | openrailsrs |
|---|-------|-------------|
| Tile del jugador | `camera->pozT[2]` | implícito en `FloatingOrigin` + `RouteFocus` |
| Offset local | `playerPos` ±1024 | tren/cámara en view space |
| Recentre | `CameraFree::check_coords` | `floating_origin.rs` umbral 256 m, solo XZ |

---

## Streaming

TSRE carga tiles en ventana **`±tileLod`** (default 2 → 5×5 tiles ≈ 10 km):

- `Route::pushRenderItems` → `requestTile(playerT + i, playerT + j)`
- `TerrainLibSimple::render` → crea `Terrain(tx,tz)` bajo demanda

openrailsrs usa **metros** (`OPENRAILSRS_VIEW_RADIUS_M`, default 120) centrados en el tren en live.

---

## TDB: posición en vía

Pipeline TSRE (`TDB.cpp`):

1. `getDrawPositionOnTrNode(out, nodeId, chainage_m)` recorre `trVectorSection[]`
2. `TSection::getDrawPosition` por enlace procedural
3. Transform con tile + local + superelevación

Equivalente openrailsrs: `section_path_spans` + `find_location_in_section_world` (port OR `FindLocationInSection`).

Snap inverso (punto mundo → vía): TSRE `findNearestPositionOnTDB` construye segmentos con `getLines` y busca distancia punto-segmento. openrailsrs: `TrackPositionResolver::nearest_on_tile` + `nearest_track_position`.

**Uso en viewer3d (2026-06):** paradas live, señales replay, nodos/vía lógica del grafo — ver [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md) § Alineación grafo → TDB.

Layout `trVectorSection.param[]` (vector node tipo 1):

| Índice | Contenido |
|--------|-----------|
| 8–9 | tile X, Z |
| 10–12 | local X, Y, Z |
| 13–15 | rotaciones / peralte |

Al unir puntos en tiles distintos: `pos2[0] += 2048 * (tile_x2 - tile_x1)` (`TRnode.cpp`).

---

## Carga `.w`

`Tile::load()`:

1. Ruta: `routes/<route>/world/w±XXXXXX±YYYYYY.w`
2. **Texto:** token `Tr_Worldfile` → `loadUtf16Data` (ParserX)
3. **Binario:** token `375`, offset tokens `261844`, bloques length-prefixed (`TS.h`)

Tras parse: `wczytajObiekty()` → `WorldObj::load(x,z)` por tipo (`Static`, `TrackObj`, …).

---

## Validación TrItem ↔ mundo

TSRE `TDB::checkDatabase` cruza items TDB con `TrackObj` en tiles `.w` por `(tile, UiD)` y `trItemId`.

openrailsrs:

- `track_audit` — distancia TrackObj → chord TDB
- `placement_audit` — grafo vs TDB vs `.w` vs anchor
- **`--audit-tr-item`** — [`tr_item_audit.rs`](../crates/openrailsrs-viewer3d/src/tr_item_audit.rs): host vector, pose centreline, enlace `.w` (`TrItemId` / `SignalUnits`), delta XZ
- **`TrItemWorldIndex`** — índice `tr_item_id → Signal/Speedpost` en tiles streamed ([`tr_item_index.rs`](../crates/openrailsrs-viewer3d/src/tr_item_index.rs))
- Señales replay — pose desde `TrItem` TDB; diamante oculto si mesh `.w` ya cubre el item

---

## Route start

`Trk.cpp` lee `RouteStart ( tileX tileZ startpX startpZ )` dentro de `Tr_RouteFile`.

TSRE también soporta `TsreGeoProjection ( lat lon centerX centerZ )` → `GeoTsreCoordinateConverter`; rutas clásicas usan proyección **IGH/USGS** (`GeoMstsCoordinateConverter`).

openrailsrs: overlay `[viewer3d.world_anchor]` (log OR) > `.trk` RouteStart > bbox `.w` > grafo.

---

## Merge de rutas (referencia futura)

`Route::mergeRoute(route2, offsetX, offsetY, offsetZ)`:

1. `TDB::mergeTDB` — offset posiciones, remap IDs
2. Por cada `WorldObj`: `position += offset`, `check_coords`, retilear
3. **Nota signo Z:** `position[2] -= offsetZ` en merge WORLD

---

## Chiltern / Birmingham (validación manual)

1. TSRE5: abrir ruta Chiltern, navegar a tile **-6080 / 14925**
2. Comparar marquesina (Static) vs vía TDB en el editor
3. openrailsrs:

```bash
export CHILTERN_ROUTE="$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cargo run --release -p openrailsrs-viewer3d -- \
  --audit-placement --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Ver [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md) § Validar con TSRE5.

---

## Próximos frentes (referencia TSRE5)

Ideas ordenadas por impacto en Chiltern / rutas MSTS reales. No implementados salvo donde se indique lo contrario.

| Prioridad | TSRE5 | openrailsrs hoy | Siguiente paso |
|-----------|-------|------------------|----------------|
| ~~Alta~~ | `checkDatabase` / TrItem ↔ TrackObj | ✅ `--audit-tr-item`, `TrItemWorldIndex`, señales desde TrItem | Chainage real mid-edge en paradas |
| Alta | `getDrawPositionOnTrNode` + chainage | Paradas en chainage 0 | Chainage real al final de vector para paradas mid-edge |
| Media | `Route::mergeRoute` | pendiente | Herramienta CLI merge rutas parcheadas + grafo |
| Media | `TerrainLibSimple::fillRaw` | `terrain_spawn.rs` | Costuras `_Y.RAW` entre tiles (TSRE rebalancea bordes) |
| Media | `tileLod` ±2 tiles | `VIEW_RADIUS_M` 120 m | Modo “tile lab” con ventana 5×5 tiles para comparar TSRE side-by-side |
| ~~Media~~ | Corredor `--run-corridor` | ✅ `build_snapped_corridor_path` snap TDB | Densificar corredor en runtime con el tren |
| Baja | `GeoTsreCoordinateConverter` | solo IGH/MSTS clásico | Rutas con `TsreGeoProjection` en `.trk` |
| Baja | Superelevación TSection | chords planos | `param[]` peralte en `section_path_spans` |
| ~~Baja~~ | `fillWorldObjectsByTrackItemIds` | ✅ `TrItemWorldIndex` + parse `TrItemId` en `.w` | Aspectos desde `sigcfg.dat` |

Detalle de validación actual: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

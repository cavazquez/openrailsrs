# Testing — `openrailsrs-viewer3d`

## Run tests

```bash
cargo test -p openrailsrs-viewer3d
```

Full workspace (CI):

```bash
./check.sh
```

## Layout

| Module | Role |
|--------|------|
| [`test_harness.rs`](../crates/openrailsrs-viewer3d/src/test_harness.rs) | Shared `minimal_app()`, smoke fixtures, `with_replay_world` / `with_live_world` |
| [`openrailsrs-bevy-scenery`](../crates/openrailsrs-bevy-scenery/src/test_harness.rs) | `minimal_scenery_app()` — materiales OR sin ventana |
| [`app_smoke.rs`](../crates/openrailsrs-viewer3d/src/app_smoke.rs) | Replay: track, train, camera, precipitation |
| [`app_floating.rs`](../crates/openrailsrs-viewer3d/src/app_floating.rs) | Floating origin multi-frame + B0001 schedule |
| [`app_live.rs`](../crates/openrailsrs-viewer3d/src/app_live.rs) | Live drive spawn/update/input |
| [`app_gameplay.rs`](../crates/openrailsrs-viewer3d/src/app_gameplay.rs) | Stops, toasts, billboards, vignette |
| [`app_spawn.rs`](../crates/openrailsrs-viewer3d/src/app_spawn.rs) | Per-spawn smoke + startup chain |
| `world.rs`, `terrain.rs`, `train.rs`, … | Pure unit / fixture tests |

Tests use `bevy::ecs::system::RunSystemOnce` with `MinimalPlugins` (no window, no GPU loop).

Fixtures: [`examples/smoke`](../examples/smoke) (`scenario.toml`, `routes/test`).

## Spawn coverage

| Startup system | Test |
|----------------|------|
| `spawn_track_meshes` | `app_smoke`, `app_spawn` |
| `spawn_signal_markers` | `app_smoke` |
| `spawn_train_markers` | `app_smoke` (replay active) |
| `spawn_camera` | `app_smoke`, `app_floating`, `app_live` |
| `spawn_precipitation` | `app_smoke` |
| `spawn_terrain_meshes` | `app_spawn`, `terrain` unit |
| `spawn_world_boxes` | `app_spawn`, `world` unit |
| `spawn_forest_patches` | `app_spawn`, `forest` unit |
| `spawn_water_patches` | `app_spawn`, `water` unit |
| `spawn_dyntrack_segments` | `app_spawn`, `dyntrack` unit |
| `spawn_ground_and_lights` | `app_spawn` |
| `spawn_sky_dome` | `app_spawn`, `sky` unit |
| `spawn_live_train` | `app_live` |
| `spawn_gameplay_ui` / `spawn_gameplay_markers` | `app_gameplay` |
| Full Startup chain | `app_spawn::viewer_startup_chain_smoke_route` |

## Live / gameplay

- **Live:** `LiveDrive` from `examples/smoke/scenario.toml`; sim stepping in `openrailsrs-sim` + Bevy wiring in `app_live`.
- **Billboards:** `stop_billboard_ui_from_viewport` (unit) + `update_stop_billboards` (system smoke with mock `Window`).

## Regressions (Chiltern / coordinates)

Unit tests in `world.rs` / `terrain.rs` for `RouteFocus`, MSL vs scenery Y, terrain patch offset, hash tile discovery.

Optional manual test (ignored in CI):

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
cargo test -p openrailsrs-viewer3d chiltern_live_startup_no_panic -- --ignored
```

Requires `examples/chiltern/scenario.toml` and MSTS content on disk.

## Pullman exterior (Chiltern / DMBSA)

Regresiones y diagnóstico de carrocería + texto **PULLMAN** (alpha, winding, cull):

- Doc: [`PULLMAN_EXTERIOR_SESSION_2026-06-21.md`](PULLMAN_EXTERIOR_SESSION_2026-06-21.md)
- Tests: `pullman_exterior_alpha_modes_audit`, `pullman_train_exterior_single_sided_back_cull`, `pullman_prim_state_z_bias_sane`
- Matriz visual: `./scripts/pullman_visual_matrix.sh` → `tmp/pullman_matrix/`
- CLI OBJ: `cargo run -p openrailsrs-cli -- shape-obj-dump …/RF_WP_DMBSA.s -o /tmp/DMBSA.obj`

## Floating origin

- Threshold 256 m (`floating_origin.rs` unit).
- System tests: camera + scene shift together, `FloatingOrigin.shift` accumulation, noop below threshold, double recentre, no B0001 with `follow_train_camera`.

## Track Viewer study / `--track-dev` audit

Estudio comparativo OR Track Viewer vs parsers y vía procedural openrailsrs:

- Doc: [`TRACKVIEWER_STUDY.md`](TRACKVIEWER_STUDY.md)
- Fixtures JSON: [`docs/fixtures/smoke-track-audit-good.json`](fixtures/smoke-track-audit-good.json), [`docs/fixtures/chiltern-track-audit.json`](fixtures/chiltern-track-audit.json)
- Tests: `track_audit::tests` (`aligned_synthetic_route_scores_good`, `write_smoke_track_audit_fixture`, `export_chiltern_msts_track_audit` ignored)
- Regenerar Chiltern (requiere MSTS):

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
OPENRAILSRS_TRACK_AUDIT="$PWD/docs/fixtures/chiltern-track-audit.json" \
  cargo test -p openrailsrs-viewer3d --lib export_chiltern_msts_track_audit -- --ignored --nocapture
```

Modo dev interactivo: `OPENRAILSRS_TRACK_DEV_RENDER=1` + `--track-dev --route-root …` (ver estudio §8).

Geometría TSection (2026-06): chords y meshes usan [`section_path_spans`](../crates/openrailsrs-bevy-scenery/src/spawn/tdb_track.rs) (port de OR `FindLocationInSection`); tests `minimal_tdb_shape_zero_*`, `path_spans_chain_with_zero_intra_node_gap`.

## Ventana móvil ~120 m (live)

En `--live`, el centro de carga/despawn es la cabeza del tren ([`ViewWindow`](../crates/openrailsrs-viewer3d/src/view_window.rs)), no el anchor fijo de la ruta.

| Variable | Default | Rol |
|----------|---------|-----|
| `OPENRAILSRS_VIEW_RADIUS_M` | 120 | Radio unificado (world, TDB stream, terreno) |
| `OPENRAILSRS_VISIBLE_RADIUS_M` | alias → view radius | Retrocompat |
| `OPENRAILSRS_RUN_CORRIDOR_RADIUS_M` | 150 | Radio TDB en `--run-corridor` |
| `OPENRAILSRS_RUN_CORRIDOR_WIDTH_M` | 240 | Ancho total del corredor (±120 m lateral) |
| `OPENRAILSRS_RUN_CORRIDOR_AHEAD_M` | 80 | Ventana longitudinal delante del tren |
| `OPENRAILSRS_RUN_CORRIDOR_BEHIND_M` | 40 | Ventana longitudinal detrás del tren |
| `OPENRAILSRS_RUN_CORRIDOR_SCENERY` | — | Con `--run-corridor`: carga WORLD+terreno+shapes (estación OR) además de vía `.tdb` |

Validación manual Chiltern:

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"
OPENRAILSRS_VIEW_RADIUS_M=120 cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Modo Full + live (escenario OR completo, TrackObj en `.w`):

```bash
OPENRAILSRS_VIEW_RADIUS_M=300 cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

El overlay Chiltern trae `[viewer3d.world_anchor]` (posición OR al arrancar la actividad). El viewer aplica un **trim** `anchor − graph_start` para alinear tren y estación aunque el grafo parcheado esté ~1–2 km desfasado.

Comando recomendado (estación techada + vía `.tdb`):

```bash
OPENRAILSRS_RUN_CORRIDOR_SCENERY=1 OPENRAILSRS_VIEW_RADIUS_M=300 \
  cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Usa **300 m** al arranque para ver la marquesina de Birmingham; en marcha puedes bajar a 120 m.

Modo corredor mínimo (solo vía `.tdb`, sin estación): `view_window::tests`, `launch::corridor_tests`, `tdb_track` (`segment_key_stable`).

Con `.tdb` cargado, la polilínea de `--run-corridor` se **snappea a la centreline TDB** tras construir `RouteAssets` (log `run_corridor — snapped N/M points`). Cada vértice prefiere `tdb_node_track_pose(nNNNN, 0)` y cae a `snap_msts_to_tdb` con el hint del grafo. Sin `.tdb`: comportamiento anterior (grafo + `RouteWorldOffset`).

Tests: `track_position::snap_corridor_path_moves_off_graph_hint`, `launch::snapped_corridor_path_has_points_on_fixture_tdb`.

## Track Viewer study — Parte 2 (pat / items / outliers)

Profundización: TrItem, paths `.pat`, casos outlier Chiltern.

- Doc: [`TRACKVIEWER_STUDY_PART2.md`](TRACKVIEWER_STUDY_PART2.md)
- Tests headless (requieren MSTS Chiltern + `examples/chiltern/track.toml`):

```bash
cargo test -p openrailsrs-msts document_birmingham_pat_for_study -- --ignored --nocapture
cargo test -p openrailsrs-viewer3d document_chiltern_outlier_nodes -- --ignored --nocapture
```

- TrackObj outlier (C2): `cargo run -p openrailsrs-cli -- world-dump "$CHILTERN/WORLD/w-006079+014925.w" --csv /tmp/w6079.csv`

## Validar con TSRE5 (Chiltern / Birmingham)

Referencia: [`TSRE5_STUDY.md`](TSRE5_STUDY.md), [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md).

1. **TSRE5:** `gameRoot` → Content OR, `routeName` = carpeta Chiltern; navegar a tile **-6080 / 14925** (estación Birmingham).
2. **openrailsrs placement audit** (headless, sin ventana):

```bash
export CHILTERN_ROUTE="$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cargo run --release -p openrailsrs-viewer3d -- \
  --audit-placement --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Salida JSON: posiciones grafo / TDB / `.w` / `world_anchor` y deltas XZ. Criterio visual: marquesina–riel **< 5 m** XZ.

**Audit TrItem** (TSRE `checkDatabase`, headless):

```bash
export OPENRAILSRS_TR_ITEM_AUDIT=/tmp/tr_item_audit.json
cargo run --release -p openrailsrs-viewer3d -- \
  --audit-tr-item --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Comprueba host vector único por `TrItem`, resolución de pose TDB, enlace opcional con `.w` (`TrItemId` / `SignalUnits`) y delta XZ vs mesh (< 25 m). Test ignored: `tr_item_audit::export_chiltern_tr_item_audit`.

Campos relevantes por parada (`stops[]`):

| Campo | Espacio | Significado |
|-------|---------|-------------|
| `graph_bevy` | MSTS absoluto | Nodo en coords del grafo parcheado + offset |
| `tdb_bevy` | MSTS absoluto | Centreline `.tdb` en el nodo homólogo (`n10778` → id 10778) |
| `marker_bevy` | Render | Posición que usa el viewer (TDB si resuelve) |
| `delta_graph_tdb_xz_m` | — | Desfase horizontal grafo vs TDB (~1835 m en Chiltern Birmingham) |

3. **Viewer híbrido** (marquesina + vía `.tdb`):

```bash
OPENRAILSRS_RUN_CORRIDOR_SCENERY=1 OPENRAILSRS_VIEW_RADIUS_M=300 \
  cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Tests unitarios: `placement_audit::tests`, `track_position::tests`, `route::tests` (RouteStart).

## Alineación grafo → TDB (marcadores y objetos del grafo)

Cuando el grafo importado (`track.toml`) diverge del escenario MSTS (~1–2 km en Chiltern), la **simulación** sigue el grafo lógico; los **objetos visuales derivados del grafo** se alinean a la centreline `.tdb` (port TSRE `getDrawPositionOnTrNode` + `findNearestPositionOnTDB`).

Referencia: [`track_position.rs`](../crates/openrailsrs-viewer3d/src/track_position.rs), [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md) § Alineación visual.

### Qué se alinea

| Objeto | Módulo | Estrategia |
|--------|--------|------------|
| Paradas / destino (`--live`) | `gameplay.rs` | `marker_render_world_at_node` — pose TDB en nodo (`n10778`) |
| Señales (replay / no-live) | `signals.rs` | `TrItem` → `tdb_node_track_pose`; fallback `marker_render_world_on_edge`; oculta diamante si `.w` Signal cubre el mismo `TrItem` (< 25 m) |
| Corredor `--run-corridor` | `track_position.rs`, `main.rs` | Polilínea snappeada a TDB tras cargar `.tdb` (filtra chords TDB en live) |
| Nodos grafo (switch/estación) | `track.rs` | `marker_render_world_at_graph_node` |
| Vía lógica (full + compact lines) | `track.rs` | Extremos de arista alineados a TDB |

### Qué no se alinea (a propósito)

| Objeto | Motivo |
|--------|--------|
| Tren en `--live` | Física y odometría en grafo (`position_on_graph`) |
| Vía `.tdb` procedural | Ya es la referencia MSTS (`tdb_track.rs`) |
| Escenario `.w` | Posición nativa MSTS |

### Pipeline

```text
1. Hint MSTS: grafo + RouteWorldOffset (o nodo TDB directo si id nNNNN)
2. Snap: nearest_track_position en tile del hint (radio configurable)
3. Y: ground_y_at + RouteFocus.to_render_surface
```

### Variables

| Variable | Default | Rol |
|----------|---------|-----|
| `OPENRAILSRS_TDB_SNAP_RADIUS_M` | 2500 | Radio máximo XZ hint grafo → centreline TDB (50–10000 m) |

Constante: `TDB_GRAPH_SNAP_RADIUS_M` en `track_position.rs` (cubre desfase Chiltern ~1835 m).

### Validación manual (marcadores)

1. Ejecutar placement audit y comprobar `delta_graph_tdb_xz_m` grande pero `marker_bevy` ≈ `tdb_bevy` en render (marquesina alineada).
2. Viewer híbrido con parada Birmingham (`n10778`):

```bash
OPENRAILSRS_RUN_CORRIDOR_SCENERY=1 OPENRAILSRS_VIEW_RADIUS_M=300 \
  cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Esfera azul de parada debe coincidir con la marquesina (< 5 m XZ vs Static `.w` en tile -6080/14925).

3. Señales (modo replay, sin `--live`):

```bash
cargo run --release -p openrailsrs-viewer3d -- \
  --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Los diamantes de señal deben quedar sobre la vía TDB visible, no ~1,8 km desplazados.

### Tests unitarios

| Test | Qué verifica |
|------|--------------|
| `track_position::marker_render_world_prefers_tdb_on_fixture` | Nodo: TDB gana sobre hint grafo erróneo |
| `track_position::marker_render_world_on_edge_snaps_to_tdb_fixture` | Arista/señal: snap TDB |
| `track_position::marker_render_world_falls_back_without_tdb` | Smoke route sin `.tdb` |
| `track_position::snap_corridor_path_moves_off_graph_hint` | Corredor: vértice alineado a TDB |
| `track_position::tr_item_pose_matches_snap` | Pose TrItem = centreline en nodo host |
| `track_position::tdb_snap_radius_default_covers_chiltern_patch` | Radio ≥ 2000 m |
| `signals::signal_at_mid_edge` | Sin resolver: comportamiento grafo clásico |
| `signals::signal_uses_tdb_pose_when_resolver` | TrItem pose ≠ interpolación grafo |
| `tr_item_audit::tr_item_audit_on_with_signals_fixture` | Host vector único (fixture sintético) |
| `app_gameplay::spawn_gameplay_markers_*` | Marcadores live en render-local Y |

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

## WORLD GPU instancing (#58)

Repeated static **opaque** WORLD shapes share one entity per `(shape, part, tile)` with a GPU instance buffer.

| Piece | Path |
|-------|------|
| Module | [`world_instancing.rs`](../crates/openrailsrs-viewer3d/src/world_instancing.rs) |
| Min instances / tile | 4 (`WORLD_INSTANCING_MIN`) |
| Opt-out | `OPENRAILSRS_WORLD_INSTANCING=0` |

Animated shapes (#34) and transparent parts stay on the per-entity path. LOD swaps the group mesh (shared LOD per tile). Unload uses `WorldTileBound` (#62). Log line: `GPU instanced N group(s) covering M instance(s)`.

v1 shader: albedo + simple Lambert (not full `StandardMaterial` shadows/PBR).

## PBR normal maps opt-in (#44)

Classic MSTS shapes need no change. Optional sibling sidecar:

`MiShape.s` → `MiShape.s.pbr.json`

```json
{
  "normal_maps": { "body.ace": "body_n.ace" },
  "flip_normal_map_y": false
}
```

| Piece | Detail |
|-------|--------|
| Module | [`pbr_sidecar.rs`](../crates/openrailsrs-bevy-scenery/src/shapes/pbr_sidecar.rs) |
| Tangents | `ensure_tangents_for_normal_mapping` (MikkTSpace) only when mapped |
| Material | Bevy `StandardMaterial` + linear normal ACE; OpenGL Y default |
| Tests | `pbr_sidecar_*`, `ensure_tangents_*`, `pbr_sidecar_adds_tangents_and_normal_map` |

`OrSceneryMaterial` / cab OR shaders ignore the sidecar. Do not treat `uv_op_embossbump` as a modern normal map.

## Visual regression (#43)

Deterministic smoke capture + structural metrics (no OpenRails/Wine automation).

| Piece | Path |
|-------|------|
| Script | [`scripts/visual_regression_smoke.sh`](../scripts/visual_regression_smoke.sh) |
| Golden | [`docs/fixtures/visual/smoke_orbit.png`](fixtures/visual/smoke_orbit.png) |
| Diff tool | `cargo run -p openrailsrs-viewer3d --bin openrailsrs-visual-diff` |
| Structural test | `smoke_route_structural_metrics` (headless, no GPU) |

```bash
./scripts/visual_regression_smoke.sh
UPDATE_GOLDEN=1 ./scripts/visual_regression_smoke.sh   # regenerate golden
```

Key env (script sets defaults):

| Variable | Role |
|----------|------|
| `OPENRAILSRS_SCREENSHOT` | Output PNG path |
| `OPENRAILSRS_SCREENSHOT_AFTER_READY=1` | Capture after WORLD spawn done + N Playing frames |
| `OPENRAILSRS_SCREENSHOT_READY_FRAMES` | Frames after ready (default 30 / script 45) |
| `OPENRAILSRS_SCREENSHOT_DELAY_S` | Max wait / legacy delay |
| `OPENRAILSRS_CAM_YAW` / `_PITCH` / `_DIST` | Fixed orbit |
| `OPENRAILSRS_WINDOW_WIDTH` / `_HEIGHT` | Fixed resolution (golden 640×360) |
| `OPENRAILSRS_VISUAL_TOL` | Hot ΔRGB per channel (default 16) |
| `OPENRAILSRS_VISUAL_MAX_HOT_PCT` | Fail if hot pixels exceed % (default 2) |

CI job `visual-smoke` runs the script under `xvfb-run` and uploads `actual.png` / `diff.png` on failure.

### Chiltern Birmingham exterior + cabina (#71)

Deterministic dual-camera capture vs **openrailsrs** baseline goldens (local; needs MSTS content + GPU).

| Piece | Path |
|-------|------|
| Script | [`scripts/visual_regression_chiltern.sh`](../scripts/visual_regression_chiltern.sh) |
| Goldens | [`docs/fixtures/visual/chiltern/`](fixtures/visual/chiltern/) (`birmingham_exterior.png`, `birmingham_cabina.png`) |
| Diff | `openrailsrs-visual-diff` (shared core: `visual_diff_core`) |
| Injection tests | `visual_diff_core::tests` — synthetic train ×1.5 / sink must fail hot-% budget |

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh   # first run / refresh baselines
./scripts/visual_regression_chiltern.sh                         # compare
```

Views:

| Name | Camera |
|------|--------|
| `birmingham_exterior` | `OPENRAILSRS_FOLLOW=orbit` + fixed yaw/pitch/dist near tile −6080/14925 |
| `birmingham_cabina` | `OPENRAILSRS_FOLLOW=driver` |

Optional OR references (manual only): `docs/fixtures/visual/or_reference/{desdeafuera,cabina}.png`.

Not wired into GitHub Actions (Chiltern assets not on runners); smoke CI remains #43.

### Manual OpenRails checklist (Chiltern Birmingham)

Useful when comparing Bevy vs OR at station tile **-6080 / 14925**:

1. Content: `OPENRAILSRS_MSTS_CONTENT` → Chiltern route on disk.
2. OpenRails (Wine or native): activity *RS_Let's go to Birmingham* (or free roam to Birmingham).
3. Position near marquesina / tile **-6080, 14925**; note camera yaw/pitch/height roughly matching Bevy orbit if comparing screenshots.
4. Bevy: `./scripts/visual_regression_chiltern.sh` or `OPENRAILSRS_VIEW_RADIUS_M=400` + `--live --route-root … examples/chiltern/scenario.toml`.
5. Check: platform/canopy alignment vs track, Transfer/forest presence, no NaN/missing major Static. Pixel-perfect not required.

OR speed-CSV capture helpers remain under `scripts/capture_chiltern_birmingham_or.sh` (physics baseline, not visual golden).

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

## Rolling stock part animation (#40)

Exterior parts (`LiveTrainBody` / `train:*:part:*`) get drivers from matrix names (`WHEEL*`, `BOGIE*`, `DOOR*`, `PANTO*`):

| Kind | Driver |
|------|--------|
| Wheel | `angle += (v/r)*dt`; rotation X local; body transform unchanged |
| Bogie | relative yaw clamp (curve lever approx.) |
| Door / Panto | stub key `0` or `OPENRAILSRS_DEBUG_DOOR_KEY` / `OPENRAILSRS_DEBUG_PANTO_KEY` ∈ `[0,1]` |

Module: [`rolling_stock_anim.rs`](../crates/openrailsrs-viewer3d/src/rolling_stock_anim.rs). Cab interior is not animated.

**Manual checklist (Pullman Birmingham, lateral + curve):**

1. Live or replay with exterior visible (chase / orbit).
2. Wheels: rotation visible when speed ≠ 0; car body does not spin with wheels.
3. Bogies: small relative yaw on curves (may be subtle on Pullman if few `BOGIE*` matrices).
4. Optional: `OPENRAILSRS_DEBUG_DOOR_KEY=1` / `_PANTO_KEY=1` — keyed parts stay finite (no NaN / hierarchy break).
5. Acceptance primaria: unit tests in `rolling_stock_anim`; Pullman is visual checklist.
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

En `--live`, el centro de carga/despawn es la cabeza del tren cuando hay follow activo; con `follow:off` sigue la cámara libre ([`ViewWindow`](../crates/openrailsrs-viewer3d/src/view_window.rs)).

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

Salida JSON: posiciones grafo / TDB / `.w` / `world_anchor`, deltas XZ y **ΔY** (`delta_y_m` scenery→TDB; `delta_marker_tdb_y_m` en paradas). Contadores `scenery_buried_vs_tdb` / `scenery_floating_vs_tdb` (umbrales: buried si ΔY < −2 m, floating si ΔY > +5 m). Criterio visual: marquesina–riel **< 5 m** XZ.

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
| Física / odometría | Siguen en el grafo lógico; solo el **render** del tren usa TDB (#67: `vehicle_pose_on_graph_edge`) |
| Vía `.tdb` procedural | Referencia MSTS; Y/pitch/roll desde TDB (`CreateFromYawPitchRoll`, #65) |
| Pitch/roll del tren | `TrackPose` expone yaw + `pitch_rad`/`roll_rad` (#65); vehículo aún usa yaw-only en Quat (#67) |
| Escenario `.w` | Posición nativa MSTS |

### Pipeline

```text
1. Hint MSTS: grafo + RouteWorldOffset (o nodo TDB directo si id nNNNN)
2. Snap: nearest_track_position en tile del hint (radio configurable)
3. Y: TDB absoluto + RouteFocus.to_render_surface (sin aplanar con ground_y_at; #65/#67)
```

### Startup PERF (#82 / #55)

With `OPENRAILSRS_PERF_DEBUG=1`:

| Metric | When | Includes |
|--------|------|----------|
| `time_to_first_presented_ms` | First Update with a sized primary window | Bevy plugins, Winit, GPU/swapchain init — **not** route parse |
| `time_to_ready_ms` | Route background thread finishes | Boot → `RouteLoadBundle` ready (may be after first presented frame) |

Do **not** treat a pre-`App::run` “time_to_window” stamp as “window visible”. There is no fixed &lt;500 ms SLO without a measured baseline on target hardware.

### Variables

| Variable | Default | Rol |
|----------|---------|-----|
| `OPENRAILSRS_TDB_SNAP_RADIUS_M` | 2500 | Radio máximo XZ hint grafo → centreline TDB (50–10000 m) |
| `OPENRAILSRS_PERF_DEBUG` | unset | Log `[PERF]` spans (startup presentation, LOD, unload, …) |

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

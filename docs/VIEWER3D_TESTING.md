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

## Track Viewer study — Parte 2 (pat / items / outliers)

Profundización: TrItem, paths `.pat`, casos outlier Chiltern.

- Doc: [`TRACKVIEWER_STUDY_PART2.md`](TRACKVIEWER_STUDY_PART2.md)
- Tests headless (requieren MSTS Chiltern + `examples/chiltern/track.toml`):

```bash
cargo test -p openrailsrs-msts document_birmingham_pat_for_study -- --ignored --nocapture
cargo test -p openrailsrs-viewer3d document_chiltern_outlier_nodes -- --ignored --nocapture
```

- TrackObj outlier (C2): `cargo run -p openrailsrs-cli -- world-dump "$CHILTERN/WORLD/w-006079+014925.w" --csv /tmp/w6079.csv`

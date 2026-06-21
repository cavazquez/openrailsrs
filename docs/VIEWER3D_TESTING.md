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

## Floating origin

- Threshold 256 m (`floating_origin.rs` unit).
- System tests: camera + scene shift together, `FloatingOrigin.shift` accumulation, noop below threshold, double recentre, no B0001 with `follow_train_camera`.

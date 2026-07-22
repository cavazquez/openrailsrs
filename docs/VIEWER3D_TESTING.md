# Testing — `openrailsrs-viewer3d`

## Comandos

```bash
cargo test -p openrailsrs-viewer3d
./check.sh                          # CI local (fmt + clippy + tests + build)
```

Fixtures: [`examples/smoke`](../examples/smoke). Harness: `test_harness.rs` (`minimal_app`, replay/live). Tests ECS con `MinimalPlugins` (sin ventana).

| Área | Módulos / tests |
|------|-----------------|
| Replay / spawn | `app_smoke`, `app_spawn` |
| Floating origin | `app_floating` |
| Live | `app_live`, `app_gameplay` |
| Unidades | `world`, `terrain`, `train`, `shapes`, `sky`, … |

## Live Chiltern

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"

# Full
cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml

# Corredor (cabina sin WORLD pesado)
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Setup: [`CHILTERN.md`](CHILTERN.md). Cabina: [`CABVIEW3D.md`](CABVIEW3D.md).

## Features cubiertos por tests

| Tema | Notas |
|------|--------|
| Instancing WORLD (#58) | `world_instancing`; opt-out `OPENRAILSRS_WORLD_INSTANCING=0` |
| Sombras instanced (#72) | receive + cast Shadow phase |
| Fog (#39) | on by default; `F` → densidad 0 (no quitar componente) |
| PBR sidecar (#44) | `*.s.pbr.json` → tangents + normal map |
| Bogies (#69) / puertas (#81) | `rolling_stock_anim` |
| Pullman exterior | alpha/cull tests; `./scripts/pullman_visual_matrix.sh` |

## Visual regression

| | Smoke (#43) | Chiltern (#71) |
|---|---|---|
| Script | `./scripts/visual_regression_smoke.sh` | `./scripts/visual_regression_chiltern.sh` |
| Golden | `docs/fixtures/visual/smoke_orbit.png` | `docs/fixtures/visual/chiltern/` |
| CI | job `visual-smoke` (xvfb + lavapipe) | local (necesita Content) |

```bash
UPDATE_GOLDEN=1 ./scripts/visual_regression_smoke.sh
# Diff: cargo run -p openrailsrs-viewer3d --bin openrailsrs-visual-diff -- actual.png golden.png
```

Inyección (escala/sink): tests en `visual_diff_core`.

## Coords / audit

Grafo→TDB: ID solo si pose ≤25 m al grafo absoluto ([`MSTS_COORDINATES.md`](MSTS_COORDINATES.md), [`TRACK_MSTS.md`](TRACK_MSTS.md)).

```bash
cargo test -p openrailsrs-viewer3d track_audit -- --nocapture
# Ignored (Content): chiltern_live_startup_no_panic
```

Checklist OR manual Birmingham: tile **−6080 / 14925** — plataforma/canopy vs vía, sin NaN.

## Estado / gaps

[`VIEWER3D.md`](VIEWER3D.md) · arquitectura [`BEVY.md`](BEVY.md).

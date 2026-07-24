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
| Instancing light model (#138) | TexDiff/Unknown only; HalfBright/Tex→FullBright/Specular*/emissive/`metallic>0.1`/metallic-roughness → entity path |
| Instancing HDR | Luz física escalada por `view.exposure`; diffuse Lambert normalizado con `1/π` para evitar scenery blanco |
| SortIndex (#102) / dual-pass (#101) | `mesh.rs` order; `blend_alpha_passes_*`; DDS scenery dual_blend |
| Sombras instanced (#72) | receive + cast Shadow phase |
| Fog (#39) | on by default; `F` → densidad 0 (no quitar componente) |
| PBR sidecar (#44) | `*.s.pbr.json` → tangents + normal map |
| Bogies (#69) / puertas (#81) | `rolling_stock_anim` |
| Pullman exterior | alpha/cull tests; `./scripts/pullman_visual_matrix.sh` |

## Visual regression

| | Smoke (#43) | Chiltern (#71 / #170 cab slice) |
|---|---|---|
| Script | `./scripts/visual_regression_smoke.sh` | `./scripts/visual_regression_chiltern.sh` |
| Golden | `docs/fixtures/visual/smoke_orbit.png` | `docs/fixtures/visual/chiltern/` |
| CI | job `visual-smoke` (xvfb + lavapipe) | local (necesita Content) |

Vistas Chiltern: `birmingham_exterior`, `birmingham_cabina` (frente), `_up`, `_left`, `_right`. Look cabina: `OPENRAILSRS_LOOK_YAW` / `_PITCH` (radianes). Chase/orbit cab2d y máscaras estructurales → follow-up #170.

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
# Env limpio recomendado (sin vars de sesión heredadas):
env -i HOME="$HOME" USER="$USER" PATH="$PATH" \
  DISPLAY="${DISPLAY:-}" WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" \
  XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-}" \
  OPENRAILSRS_MSTS_CONTENT="$OPENRAILSRS_MSTS_CONTENT" \
  UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh
./scripts/visual_regression_chiltern.sh                     # compara vs goldens
UPDATE_GOLDEN=1 ./scripts/visual_regression_smoke.sh
# Diff suelto: cargo run -p openrailsrs-viewer3d --bin openrailsrs-visual-diff -- actual.png golden.png
```

Inyección (escala×1.5 / sink ~5 m): tests en `visual_diff_core` (`scale_train_1_5x_fails_diff`, `sink_train_5m_equivalent_fails_diff`).

OR lado a lado (manual): `docs/fixtures/visual/or_reference/{desdeafuera,cabina}.png`.

## Coords / audit

Grafo→TDB: ID solo si pose ≤25 m al grafo absoluto ([`MSTS_COORDINATES.md`](MSTS_COORDINATES.md), [`TRACK_MSTS.md`](TRACK_MSTS.md)).

```bash
cargo test -p openrailsrs-viewer3d track_audit -- --nocapture
# Ignored (Content): chiltern_live_startup_no_panic
```

Checklist OR manual Birmingham: tile **−6080 / 14925** — plataforma/canopy vs vía, sin NaN.

## Estado / gaps

[`VIEWER3D.md`](VIEWER3D.md) · arquitectura [`BEVY.md`](BEVY.md).

# Cabina 3D (`CABVIEW3D`)

Vista conductor en `viewer3d --live` (Pullman Chiltern: `RF_Blue_Pullman` / `PULLMAN_GR.s` + `.cvf`).

## Estado

| Pieza | Estado |
|-------|--------|
| `DriverCam` (C/V) | ✅ |
| Mesh `CABVIEW3D/*.s` + ACE | ✅ |
| Shader `or_cab` (TexDiff) | ✅ |
| Cámara `ORTS3DCabHeadPos` | ✅ |
| Ocultar exterior en L1 | ✅ |
| CVF overlay 2D en cabina 3D | ❌ off (#151; vista 2D: #152) |
| Panel HUD | ✅ |

## Matrices CVF (Pullman)

| Matriz | Rol típico |
|--------|------------|
| M0 | Raíz / body cab |
| M4 | Inversor / selector |
| M8–M10 | Palancas thr/brk (bindings `.cvf`) |

Detalle de bindings: `cab_cvf.rs` + tests Pullman. Debug: `OPENRAILSRS_CAB_DEBUG=uv|albedo|vcolor`.

## Env

| Variable | Default | Efecto |
|----------|---------|--------|
| `OPENRAILSRS_CAB_ALBEDO` | `1.0` | Tint |
| `OPENRAILSRS_CAB_SUN` | on | Sol/ambiente OR en TexDiff (`0` apaga) |
| `OPENRAILSRS_CAB_OR_LIKE` | off | Brillo fijo legacy (debug) |
| `OPENRAILSRS_CAB_MIN_BRIGHT` | `0` | Piso de brillo opcional |
| `OPENRAILSRS_CAB_CVF_OVERLAY` | off | Overlay ACE 2D en cabina 3D (debug; #151) |
| `OPENRAILSRS_CAB_BRIGHTEN` | off | Levantar ACE oscuros |
| `OPENRAILSRS_FOLLOW` | — | `driver`/`cab` fuerza DriverCam (capturas) |

## Arranque

```bash
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
# Full scenery: omitir --run-corridor
```

Teclas cabina: **C** · **↑/↓** · **H** bocina · **Home** centrar. Setup: [`CHILTERN.md`](CHILTERN.md).

# Bevy — arquitectura 3D

Presentación 3D separada del núcleo headless. Versión **Bevy 0.19.1** (`Cargo.lock`), con Rust mínimo 1.95 por el requisito de Bevy.

La física interactiva se ejecuta en `FixedUpdate` con reloj Bevy de 60 Hz y
cuantos físicos de hasta 0,05 s. La representación, cámaras, cabina y HUD se
actualizan en `Update`. `LiveDriveSession` y la máquina de estados de servicio
son Rust independiente de Bevy; `play-service` usa exactamente la misma
sesión y el mismo conductor automático. El test de recorrido completo
compara 30 y 144 FPS sin cambiar las llegadas ni las posiciones.

`DrivingHudPlugin` separa la presentación de conducción del núcleo. Utiliza
`Text`, `FontSource`, recursos de visibilidad y consultas Bevy disjuntas,
actualizando textos sólo cuando cambian y como máximo a 20 Hz. La fuente
DejaVu Sans Mono está incluida con su licencia para representar el español.
Los vectores TDB conservan posición, elevación, tangente y sentido de marcha,
incluyendo las aristas inversas `eNNN_r` y los offsets de andén.
Los vértices MSTS ya invierten Z al convertirse a Bevy: la base del coche
rota −90° sobre Y para que el frente de la cabina siga el +X del recorrido.
La prueba de cámara compara esa vista contra la dirección real de marcha.

Features de ventana: `x11` + `wayland`. En sesiones Wayland, sin `wayland` winit cae a XWayland y RADV suele fallar con `Surface::configure → Invalid surface`.

Present mode del viewer: default `AutoVsync` (`Fifo`). Override: `OPENRAILSRS_PRESENT_MODE=auto_vsync|auto_no_vsync|fifo|mailbox|immediate`. En híbridas AMD+NVIDIA rotas, ver troubleshooting en [`VIEWER3D.md`](VIEWER3D.md).

## Crates

| Crate | Rol |
|-------|-----|
| `openrailsrs-or-shader` | Clasificación shaders MSTS (sin Bevy) |
| `openrailsrs-bevy-scenery` | Materiales OR/WGSL, ACE, spawn, VSM, `MstsAssetPlugin` |
| `openrailsrs-viewer3d` | App jugable (`--live`, cabina, HUD) |
| `openrailsrs-render3d` | Validación visual OR (tiles + VSM) |

```mermaid
flowchart LR
  formats[formats/ace/route] --> or_shader[or-shader]
  or_shader --> scenery[bevy-scenery]
  scenery --> viewer[viewer3d]
  scenery --> render[render3d]
```

**Reglas:** headless no depende de Bevy; `bevy-scenery` no depende de las apps; WGSL solo en `bevy-scenery/assets/shaders/`.

## Apps

| | **viewer3d** | **render3d** |
|---|---|---|
| Objetivo | Sim jugable | Paridad visual / tile lab |
| Arranque | Ventana → parse ruta en background (#55) | `LoadStage::ParsingTiles` async |
| VSM | Opcional | Completo (`OPENRAILSRS_OR_VSM`) |

```bash
# Jugable Chiltern: tres estaciones, cabina y exterior
./scripts/run_chiltern_service.sh

# Validación por tiles
cargo run -p openrailsrs-render3d -- \
  --route "$CHILTERN_ROUTE" --tile-x -6084 --tile-z 14923 --radius 2
```

Scripts: `./scripts/run_render3d_*.sh`. Controles render3d: WASD/QE · RMB · F3 HUD · F4–F8 VSM.

| Env | Valores | Default |
|-----|---------|---------|
| `OPENRAILSRS_OR_VSM` | `pcf+or` / `approx` / `exact` | `pcf+or` |
| `OPENRAILSRS_OR_SHADERS` | `0` / `1` | `1` |

## Fuera de bevy-scenery

Sim live, cabina CVF, floating origin, `--run-corridor`, parse `.act` (local a render3d).

Ver también: [`VIEWER3D.md`](VIEWER3D.md) · [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md) · [`BEVY_TRANSFORMS.md`](BEVY_TRANSFORMS.md) (DirectX/MSTS → `Transform` / `Mat4`).

# Modo Full — Chiltern live sin `--run-corridor`

Documento del proceso de depuración para cargar **terreno + WORLD + cabina + tren** en `openrailsrs-viewer3d --live` con la ruta Chiltern externa.

Relacionado: [`CABVIEW3D_SESSION_2026-06-19.md`](CABVIEW3D_SESSION_2026-06-19.md) · [`PULLMAN_EXTERIOR_SESSION_2026-06-21.md`](PULLMAN_EXTERIOR_SESSION_2026-06-21.md) · [`CABVIEW3D_MATRICES.md`](CABVIEW3D_MATRICES.md) · [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md) · [`CHILTERN_OR_SETUP.md`](CHILTERN_OR_SETUP.md)

---

## Comandos

### Corredor mínimo (solo tren + vía procedural + cielo)

Útil para depurar cabina/CVF sin esperar ~30 s de spawn WORLD:

```fish
set -gx CHILTERN_ROUTE "$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" \
  examples/chiltern/scenario.toml
```

### Modo Full (terreno + 60 tiles + ~35k objetos WORLD)

```fish
cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" \
  examples/chiltern/scenario.toml
```

Teclas: **C** cabina · **V** chase · **↑/↓** throttle/freno · esperar ~30 s al spawn progresivo del world antes de juzgar ventanas.

Log esperado al arrancar:

```
render height origin 28 m terrain MSL (scenery bbox y 36)
loaded 60 world tile(s) … within 8000m of (…)
viewer3d world anchor tile -6080,14925 … -> delta -1821,0,230 m (graph already in world coords; no shift)
floating origin — XZ shift (1821, -230) m (total …)
```

---

## Espacios de coordenadas

| Espacio | Descripción | Quién lo usa |
|---------|-------------|--------------|
| **MSTS world** | Coordenadas absolutas del `.w` / TDB (~−12 M en X/Z) | Carga de tiles, `RouteFocus::center` |
| **Render** | `RouteFocus::scenery_to_render` / `to_render_surface` — resta centro y `height_origin` (28 m MSL) | Clasificación WORLD, posición del grafo vía `position_on_graph` |
| **View** | Render − `FloatingOrigin.shift` (**solo XZ**) | Entidades en escena Bevy, tren vía `view_position` cada frame |

Convenciones verticales:

- **`height_origin`**: MSL del terreno en el ancla (~28 m), no el Y del bbox del `.w` (36 m).
- **Terreno**: vértices con `y - height_origin`; tiles en XZ relativos al foco.
- **Tren**: `ground_y_at` = sample MSL + 0,30 m (cabeza de rail) → `to_render_surface` → Y ≈ **+0,3 m** sobre el plano del terreno (Y=0 en view).

El floating origin **no debe tocar Y**: la altura ya está normalizada con `height_origin`.

---

## Síntomas observados (2026-06-19)

### 1. Pantalla azul vacía en cabina (primer bug)

| HUD / log | Valor |
|-----------|-------|
| `pos` / `focus` | Números astronómicos (~10³⁰) |
| `cab: eye` | ~ (−752, −236, −1938) coherente |
| Cab parts | 69 cargadas, `inside` |

**Causa:** desincronización entre `FloatingOrigin` y spawn progresivo. El tren/cámara usaban view space; terreno/WORLD spawneaban en render space sin restar `origin.shift`. Tras el rebase, escenario y tren quedaban a ~1,8 km de separación.

**Fix:** `view_translation` / `view_transform` al spawn de terreno y WORLD; placeholders con offset horizontal de origen.

### 2. Cámara en (0,0,0) y cabina lejos (segundo bug)

| HUD | Valor |
|-----|-------|
| `pos` | 0, 0, 0 |
| `cab: eye` | ~ (−1746, 124, 879) (diag congelado) |

**Causa:** `apply_floating_origin` desplazaba **todos** los `Transform`, incluidos hijos del tren (`LiveTrainBody`, lead car). Eso corrompía locales (doble shift en jerarquía). Además, en modo CABINA, el sistema ponía la cámara a cero **después** de `follow_train_camera`.

**Fix:**

- Solo recentrar entidades raíz de escenario (`Without<ChildOf>`, sin marcadores de tren/cabina).
- No mover la cámara en `DriverCam` (la fija `follow_train_camera` cada frame).
- Orden: `apply_floating_origin` → `update_live_train_marker` → `follow_train_camera`.
- `follow_train_camera` activo también en CABINA aunque el modo sea fly (fly bloqueado en cabina).

### 3. Tren bajo el terreno / ventanas azules (tercer bug)

| Log | `floating origin — shift (1821, 9, -230)` |
| Cabina | Pupitre OK (`eye Y ≈ 3,6 m`), ventanas azules (layer 0 vacío) |
| Órbita | `pos Y=60` — cámara **encima** mirando hacia abajo |

**Causa:** el rebase acumulaba **9 m en Y** en `origin.shift`. `view_position` restaba `shift.y` del render Y (~0,3 m) → tren a **Y ≈ −8,7 m**, bajo la malla del terreno (Y=0).

**Fix:** floating origin **solo horizontal (XZ)**. `view_position` preserva `render.y`. Rebase de escenario solo resta `delta.x` / `delta.z`.

---

## Archivos modificados (integración Full)

| Archivo | Cambio |
|---------|--------|
| `floating_origin.rs` | Shift XZ; `view_position` sin Y; excluir jerarquía tren; ParamSet queries; log XZ |
| `world.rs` | `view_transform` en spawn queue; placeholders horizontales |
| `terrain_spawn.rs` | `origin_shift` horizontal al posicionar tiles |
| `lib.rs` | Orden de systems; `follow_train_camera` / fly en cabina |
| `camera.rs` | `follow_train_camera_active`, fly bloqueado en CABINA |
| `cab_render.rs` | HUD cab actualiza `eye` cuando cambia (no solo al entrar) |

---

## Arquitectura objetivo (integración terreno + cabina + exterior)

```
position_on_graph → to_render_surface (Y = MSL + 0.3 − height_origin)
                 → view_position (resta origin.shift.xz)
                 → LiveTrainMarker Transform (cada frame)

spawn terreno/WORLD → scenery_to_render → view_transform (resta origin.shift.xz al spawn)

apply_floating_origin (si |XZ| > 256 m):
  origin.shift.xz += delta.xz
  mover solo raíces escenario (XZ)
  NO mover tren, NO mover cámara en CABINA
```

Estado ~**80 %** de la arquitectura de capas (`RenderLayers` L0 mundo, L1 exterior oculto, L2 cabina). Pendiente: pulido visual, 345 `.s` sin resolver, animación CVF regulador.

---

## Verificación

```bash
./check.sh
cargo test -p openrailsrs-viewer3d floating_origin
cargo test -p openrailsrs-viewer3d app_floating
```

Checklist manual (modo Full):

1. Arrancar sin `--run-corridor`; esperar log `spawned world objects`.
2. **C** → cabina: `pos` y `cab: eye` en la misma zona (± pocos m).
3. `eye Y` ≈ 3–4 m; tren no hundido (vía visible al bajar a órbita con **V**).
4. Log: `floating origin — XZ shift (…)` **sin componente Y**.
5. Ventanas: paisaje layer 0 cuando el spawn terminó (puede tardar ~30 s).

---

## Bugs abiertos

| Bug | Notas |
|-----|-------|
| Animación regulador CVF (M8) | Sigue flotando con `thr` alto — ver sesión cabina |
| 345 TrackObj `.s` no resueltos | Log al spawn; placeholders omitidos |
| Forest/water stream | Spawn inicial OK; tiles streamed pueden necesitar mismo offset XZ |
| `--run-corridor` vs Full | Corredor excluye terreno/WORLD a propósito |

---

## Historial

| Fecha | Cambio |
|-------|--------|
| 2026-06-19 | Documento inicial tras fix floating origin XZ + spawn view + jerarquía tren |

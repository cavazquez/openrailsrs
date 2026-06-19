# Sesión cabina 3D — 2026-06-19 (Pullman / PULLMAN_GR)

Documento de trabajo para la depuración de **CABVIEW3D** en `openrailsrs-viewer3d` con Chiltern live (`RF_Blue_Pullman`, `PULLMAN_GR.s` + `.cvf`).

Relacionado: [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md) · [`FULL_SCENERY_LIVE_CHILTERN.md`](FULL_SCENERY_LIVE_CHILTERN.md) · [`OPENBVE_REFERENCE.md`](OPENBVE_REFERENCE.md) · Open Rails `ThreeDimentionCabViewer` (`MSTSLocomotiveViewer.cs`).

---

## Objetivo de la sesión

Conseguir que al subir el acelerador (**↑** / `thr`) en vista cabina (**C**) gire la **rueda del regulador** en el pupitre, no que se desplace geometría vertical u objetos flotando lejos del tablero.

Comando habitual (corredor mínimo — depurar cabina/CVF):

```fish
set -gx CHILTERN_ROUTE "$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cd ~/repos/propios/ProyectoOpenRails/openrailsrs
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Modo **Full** (terreno + WORLD): mismo comando **sin** `--run-corridor` — ver [`FULL_SCENERY_LIVE_CHILTERN.md`](FULL_SCENERY_LIVE_CHILTERN.md).

Teclas: **C** cabina · **↑/↓** throttle/freno · `OPENRAILSRS_CAB_DEBUG=albedo` para texturas planas.

---

## Contenido MSTS relevante (Pullman)

| Asset | Ruta OR |
|-------|---------|
| Cab shape | `Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s` |
| CVF | `…/PULLMAN_GR.cvf` |
| Exterior | `…/RF_WP_DMBSA.s` + `.eng` (`ORTS3DCabHeadPos`) |

Datos del shape (LOD0):

- **18 matrices** con nombres CVF (`THROTTLE:0:0` → índice **8**, `TRAIN_BRAKE:0:0/0:1` → **9/10**, `DIRECTION:0:0` → **4**).
- **69 partes** cab tras split por `(sub_object, prim_state)` (antes: un mesh por `prim_state` fusionando sub_objects).
- **Sin bloque `animations`** en el `.s` — Open Rails **no anima** matrices sin keyframes (`AnimateOneMatrix` retorna si no hay anim).
- Jerarquía: `[-1, 0, 0, …]` — todos los `vtx_state` apuntan a matriz **0 (MAIN)**.
- Pivotes aprox. (MSTS → Bevy Z flip): M8 `(-0.505, 2.454, -9.340)`, M9 `(-1.186, 2.470, -9.266)`.

---

## Qué se implementó hoy

### 1. Enlace mesh ↔ matriz CVF (`shapes.rs`, `cab_cvf.rs`, `cab_view.rs`)

**Antes:** un mesh por `prim_state` mezclando sub_objects → ninguna parte en matrices 8/9/10.

**Ahora:** un mesh por `(sub_object, prim)` con `cab_matrix_idx` opcional y componente `CabCvfPart { matrix_idx }`.

Heurísticas en `cab_matrix_for_prim()`:

| Textura / caso | Matriz | Regla |
|----------------|--------|--------|
| `Brake_wheel.ace` | 9 o 10 | Solo esa primitiva; filtro proximidad al pivote freno |
| `Controller_base.ace` | 8 | Solo la pieza **más grande** cerca del pivote throttle (exclusiva) |
| `Controls.ace` en sub 8 | — | **Excluida** (placa vertical, no rueda) |
| sub_object *i* pequeño (≤500 verts, 1 prim) | *i* | Hueso dedicado (p. ej. reversora sub 4) |

Selección exclusiva throttle (`pick_exclusive_controller_base_throttle`): entre candidatos `Controller_base` a ≤0.35 m del pivote M8 y radio ≥2 cm, gana el de **mayor extensión** (rueda r≈0.21 m).

Bindings finales diagnosticados:

```
sub 0 prim 9  → matrix 8 (THROTTLE)   Controller_base.ace  center≈(-0.37,2.43,-9.33) r=0.21
sub 4 prim 8  → matrix 4 (DIRECTION)  Controls.ace
(sub 8 Controls.ace ya NO enlazada a M8)
(Brake_wheel sin enlace: malla a ~1.3 m del pivote M9/M10)
```

### 2. Bake + animación runtime

- **`omit_leaf_matrices`**: matrices hoja CVF no se omiten del bake final; en su lugar:
  - Bake **cadena completa** M8×M0 (u homóloga).
  - **Rebase** vértices al espacio local del hueso (`coordinates::rebase_points_to_bone_local`).
  - Entidad con `static_hierarchy_chain_transform` + rotación delta en `update_cab_cvf_controls`.
- **`fallback_lever_rotation`**: eje **Y local** para throttle/freno (rueda horizontal MSTS).
- **`lever_pose_from_fallback`**: cadena jerárquica completa, no solo matriz aislada.
- **`static_matrix_transform`**: delega en `static_hierarchy_chain_transform` (M8×M0…).

Funciones nuevas en `coordinates.rs`:

- `matrix43_to_transform`, `hierarchy_chain_transform`, `static_hierarchy_chain_transform`
- `rebase_points_to_bone_local`, `rebase_vectors_to_bone_local`

### 3. Bugs corregidos en la sesión

| Bug | Síntoma | Fix |
|-----|---------|-----|
| Sub_object 10 entero ligado a M10 | Mesa/asiento rotaban fuera del tren al frenar | `Brake_wheel.ace` per-prim solamente |
| `Controller_base` amplio | Chips del pupitre + placa vertical animaban | Filtro proximidad + pieza exclusiva + excluir `Controls.ace` sub 8 |
| Eje X en fallback | Movimiento tipo “subida” | Eje Y local |
| Doble `static_t.rotation` en fallback | Rotación errática | Delta solo en `fallback_lever_rotation` |
| Vértices en coords absolutas + traslación hueso | Pieza orbita al techo con thr alto | Rebase bone-local + cadena jerárquica |

### 4. Referencia Open Rails (código en repo `openrails/`)

- `ThreeDimentionCabViewer`: enlaza matrices por nombre `TYPE:order:param` → `AnimatedPartMultiState`.
- Solo inicializa animación si `TrainCarShape.SharedShape.Animations != null`.
- `AnimateOneMatrix`: `slerp_rot` / `linear_key` sobre `XNAMatrices[i]`.
- Render: producto de `animatedXNAMatrices[hi]` siguiendo jerarquía desde `vtx_state.imatrix`.
- **Pullman**: todos los `imatrix=0` → en OR la rueda 3D tampoco se movería sin bloque `animations` en el `.s`.

Nuestro **fallback de rotación** es necesario porque el content no trae keyframes.

### 5. Diagnóstico y tests

- `cab_diag.rs`: `OPENRAILSRS_CAB_DEBUG=uv|albedo|vcolor`, logs por parte al spawn.
- Test `pullman_cvf_lever_binding_diagnostics`: imprime bindings, pivotes, bounds (skip si no hay Content OR).
- Tests Pullman: UV, ventanas DDS, texturas ≥39, CVF matrices.

---

## Estado al cierre del día — **bug abierto**

**Síntoma (usuario, tras rebase):** al subir aceleración la geometría sigue apareciendo **muy elevada**, lejos del pupitre (objeto flotando hacia ventana/techo a thr 90–100 %).

**Hipótesis pendientes:**

1. **Cadena de bake vs OR**: producto M8×M0 puede diferir en orden XNA vs nuestro `transform_shape_point` (child→parent).
2. **Matrix 0 `zero_translation`**: el bake usa `zero_translation` en MAIN; `static_hierarchy_chain_transform` usa traducción completa de M0 — posible desalineación rest pose.
3. **Pieza incorrecta**: el `Controller_base` r=0.21 m podría no ser la rueda visible (rueda negra horizontal a la derecha).
4. **Imatrix=0 en OR**: la animación correcta podría requerir animar `XNAMatrices[8]` en el renderer global, no entidades Bevy separadas.
5. **Freno**: `Brake_wheel.ace` a 1.3 m del pivote — sin enlace hasta resolver proximidad o hueso correcto.

**Próximos pasos sugeridos:**

1. Comparar posición en reposo (thr=0) vs mesh estático sin `CabCvfPart` — debe coincidir pixel-perfect.
2. Log de AABB de la parte M8 antes/después de rebase y tras una rotación de prueba.
3. Inspeccionar en Shape Viewer / OR qué sub_object/prim es la rueda necha del regulador (textura distinta de `Controller_base`).
4. Añadir bloque `animations` mínimo al shape o leer anim externa si existe en el trainset OR.
5. Unificar bake y pose con la misma función de cadena (incl. `zero_translation` en M0).

---

## Modo Full (sin `--run-corridor`) — misma sesión, tarde

Documentación completa: [`FULL_SCENERY_LIVE_CHILTERN.md`](FULL_SCENERY_LIVE_CHILTERN.md).

Resumen de tres bugs de coordenadas corregidos:

1. **Spawn progresivo vs `origin.shift`** — WORLD/terreno en render space; tren en view space → pantalla azul / coords astronómicas.
2. **Rebase en jerarquía del tren** — `apply_floating_origin` movía hijos del consist → `GlobalTransform` del lead corrupto; cámara a (0,0,0) con cabina a ~1,7 km.
3. **Shift en Y** — `origin.shift.y ≈ 9 m` enterraba el tren ~9 m bajo el terreno; ventanas azules con pupitre OK.

Tras los fixes: cabina Pullman visible (69 partes), `eye` y `pos` coherentes, log `floating origin — XZ shift (1821, -230) m` sin Y.

---

## Archivos tocados (2026-06-19)

| Área | Archivos |
|------|----------|
| CVF runtime | `crates/openrailsrs-viewer3d/src/cab_cvf.rs` |
| Mesh / binding | `crates/openrailsrs-viewer3d/src/shapes.rs` |
| Spawn cab | `crates/openrailsrs-viewer3d/src/cab_view.rs` |
| Transforms | `crates/openrailsrs-viewer3d/src/coordinates.rs` |
| Diagnóstico | `crates/openrailsrs-viewer3d/src/cab_diag.rs`, `cab_render.rs` |
| Floating origin / Full | `floating_origin.rs`, `world.rs`, `terrain_spawn.rs`, `lib.rs`, `camera.rs` |
| Shader / material | `or_cab.wgsl`, `or_cab_material.rs`, `or_shader.rs` |
| Formats | `shape_binary.rs`, `typed/shape.rs` |
| Docs | `docs/CABVIEW3D_ROADMAP.md`, `docs/FULL_SCENERY_LIVE_CHILTERN.md`, este archivo |
| CI local | `check.sh` |

---

## Verificación

```bash
./check.sh
cargo test -p openrailsrs-viewer3d floating_origin
# Tests opcionales con Content OR instalado:
cargo test -p openrailsrs-viewer3d pullman_cvf_lever_binding_diagnostics -- --nocapture
```

Variables útiles:

| Variable | Uso |
|----------|-----|
| `OPENRAILSRS_CAB_DEBUG=albedo` | Texturas sin iluminación |
| `OPENRAILSRS_CAB_SUN=0` | Default; cabina plana |
| `OPENRAILSRS_DISABLE_AUDIO=1` | Tests / CI sin audio |

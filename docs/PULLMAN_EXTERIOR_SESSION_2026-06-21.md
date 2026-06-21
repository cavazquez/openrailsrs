# Sesión exterior Pullman — 2026-06-21 (RF_WP_DMBSA / Blue Pullman)

Documento de cierre para la depuración del **exterior** del tren Blue Pullman en `openrailsrs-viewer3d` (Chiltern live, escenario `RS_Let's go to Birmingham`).

Relacionado: [`CABVIEW3D_SESSION_2026-06-19.md`](CABVIEW3D_SESSION_2026-06-19.md) (cabina) · [`FULL_SCENERY_LIVE_CHILTERN.md`](FULL_SCENERY_LIVE_CHILTERN.md) · [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md) · Open Rails `Coordinates.cs` / `Materials.cs` (`CullCounterClockwise`).

---

## Objetivo

Renderizar el consist **Blue Pullman** con carrocería opaca, texturas correctas y letras **PULLMAN** legibles (no espejadas), con paridad razonable respecto a Open Rails.

**Shape clave:** `RF_WP_DMBSA.s` (driving motor car delantera) en trainset `RF_Blue_Pullman`.

**Comando habitual (corredor mínimo):**

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"

cd ~/repos/propios/ProyectoOpenRails/openrailsrs
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" \
  examples/chiltern/scenario.toml
```

Modo full (terreno + WORLD): mismo comando **sin** `--run-corridor`.

---

## Consist — no hay locomotora clásica aparte

Archivo: `examples/chiltern/consists/birmingham_pullman.con`

```
Engine  RF_WP_DMBSA.eng   ← driving motor car (cabina delantera)
Wagon ×6                  ← coches intermedios
Engine  RF_WP_DMBSH.eng   ← driving motor car / DVT trasero
```

Es un **EMU / multiple-unit**. La sensación de “falta locomotora” es correcta: los extremos son **power cars** (`.eng`), no un trainset de loco + vagones clásico.

Log con inventario detallado:

```bash
OPENRAILSRS_DEBUG_CONSIST=1 cargo run --release -p openrailsrs-viewer3d -- ...
OPENRAILSRS_DEBUG_VEHICLE_TRANSFORMS=1  # matrices, determinante, forward por coche
```

---

## Cronología de problemas y soluciones

### Fase 1 — Carrocería agujereada / interior visible (alpha + z_bias)

| Síntoma | Causa raíz | Solución |
|---------|------------|----------|
| Paneles laterales “faltantes”, bogies/interior visibles, agujeros en la cáscara | Texturas `.ace` con canal alpha interpretadas como `Mask(0.5)` en Bevy; OR usa opaco (`ReferenceAlpha=-1`) en `TexDiff` | `shape_alpha_mode()` / reglas en `alpha_mode_from_prim_state` — opaco salvo blend explícito (cristal) |
| `depth_bias` enorme (p. ej. 16777216) en 17 `prim_state` | Parser binario de `prim_state`: token 54 leía `ZBias` como `i32` en lugar del tipo correcto | Fix en `openrailsrs-formats` (`dump_prim_state_content`, mapeo en `typed/shape.rs`) |
| Valores absurdos aunque el parser falle | Sin clamp defensivo | `clamp_msts_z_bias_for_bevy` (±10) en `bevy-scenery/shapes/debug.rs` |

**Resultado:** carrocería **sólida** (audit Pullman: 28 Opaque + 2 Blend cristal, 0 Mask, 0 z_bias corruptos).

**Tests:** `pullman_exterior_alpha_modes_audit`, `pullman_prim_state_z_bias_sane`, `no_huge_depth_bias_in_bevy_materials`.

**Aprendizaje:** en MSTS/OR, alpha en atlas **no implica** transparencia en el shader; hay que seguir `prim_state` + nombre de shader (`TexDiff` vs `BlendATexDiff`), no heurística Bevy por defecto.

---

### Fase 2 — “PULLMAN” espejado + backfaces

| Síntoma | Hipótesis inicial | Veredicto |
|---------|-------------------|-----------|
| Letras **PULLMAN** / **NAMLLUP** en el lateral | UV U invertida | **Descartada** (`FLIP_U` no corrige) |
| Mismo texto espejado con double-sided | Backfaces visibles | **Confirmada** |
| Agujeros al activar cull OR sin más cambios | Winding incorrecto | **Confirmada** |
| Vehículo “al revés” en escena | Transform determinante &lt; 0 | **Descartada** (escala/rotación OK) |
| Nariz DMBSA hacia el consist | Orientación `.eng` / consist flip | **Pendiente** (aparte del winding) |

#### Cadena causal (confirmada)

```
Conversión MSTS→Bevy (flip Z, handedness)
  → índices de triángulo sin invertir (faltaba swap winding como OR/XNA)
  → front-face del GPU = interior del vagón
  → material double_sided + cull_mode None
  → se ve la backface texturada
  → texto PULLMAN espejado
```

Open Rails documenta explícitamente en `Coordinates.cs`:

> *“the winding order of triangles is reversed in XNA”*

Y renderiza con `RasterizerState.CullCounterClockwise` (cull back faces).

#### Matriz visual (experimentos A–G)

Script: `scripts/pullman_visual_matrix.sh`  
Capturas: `tmp/pullman_matrix/*.png`

| Modo | Env | Carrocería | Texto | Conclusión |
|------|-----|------------|-------|------------|
| A1 | (producción pre-fix) | Sólida | Espejado | Baseline roto |
| A2 | `FORCE_DOUBLE_SIDED` | Igual A1 | Espejado | Ya era double-sided |
| A3 | `CULL_NORMAL` | **Agujeros** | Espejado donde queda malla | Cull sin winding → culled exterior |
| A4 | `FLIP_WINDING` (debug) | Sólida | Mejora parcial en OBJ, no basta solo | Winding necesario pero no suficiente en viewer |
| A5 | `CULL_NORMAL` + `FLIP_WINDING` | Sólida | Aún espejado en algunas tomas chase | Combo debug ≠ fix producción |
| B1 | `FLIP_U` | Sólida | Espejado | No es espejo U de conversión |
| C1 | `FACE_COLORS=back` | Silueta **roja** | **NAMLLUP en rojo** | Lo visible en A1 = **backface** |
| C2 | `FACE_COLORS=front` | Silueta **verde** | Sin texto legible | Letras no están en front-face GPU |
| A6 | `CULL_FRONT` | Sólida | Espejado | Mismas backfaces que baseline |

**Evidencia numérica (OBJ bakeado, lateral \|x\| &gt; 0.85 m):**

| Export | Triángulos “hacia afuera” | Triángulos “hacia adentro” | Normal ≠ winding |
|--------|---------------------------|----------------------------|------------------|
| Pre-fix | 6 | 22 | 30/30 |
| Post-fix winding | 22 | 6 | 0/30 |

---

## Soluciones implementadas (2026-06-21)

### 1. Inversión de winding en bake (global `.s`)

**Archivo:** `openrailsrs-bevy-scenery/src/shapes/mesh.rs` — `append_primitive_mesh_buffers`

Tras resolver vértices de cada triángulo, se hace `swap(1, 2)` (paridad OR/XNA tras `shape_point_to_bevy`).

- `OPENRAILSRS_DEBUG_FLIP_WINDING=1` (solo scope tren) **vuelve a invertir** → reproduce el bug viejo para regresión.

### 2. Material exterior de tren: single-sided + back cull

**Archivos:** `bevy-scenery/shapes/material.rs`, `viewer3d/shapes.rs`

- `train_exterior_material_with_texture()` / `apply_train_exterior_culling()`
- `double_sided: false`, `cull_mode: Face::Back`
- Activo cuando `load_shape_render_asset_from_path(..., train_exterior: true)` — **live y replay**
- **World scenery y cabina** siguen con double-sided (sin cambio de alcance global en materiales)

**Test:** `pullman_train_exterior_single_sided_back_cull`

### 3. Instrumentación de diagnóstico (se mantiene)

**Archivo:** `bevy-scenery/src/shapes/debug.rs`, `viewer3d/train_diagnostics.rs`

| Variable | Uso |
|----------|-----|
| `OPENRAILSRS_DEBUG_CULL_NORMAL` | Cull back, single-sided (experimento OR) |
| `OPENRAILSRS_DEBUG_FLIP_WINDING` | Deshace winding producción (scope tren) |
| `OPENRAILSRS_DEBUG_FLIP_U/V/UV`, `NO_UV_FLIP` | UV experimentales (scope tren) |
| `OPENRAILSRS_DEBUG_FACE_COLORS=front\|back` | Verde = front, rojo = back |
| `OPENRAILSRS_DEBUG_CONSIST` | Log `.con` (Engine/Wagon, power, cab) |
| `OPENRAILSRS_DEBUG_VEHICLE_TRANSFORMS` | Determinante, forward/right por coche |

Scope tren: `set_train_shape_debug_scope(true)` durante carga de shapes del consist (no afecta WORLD).

### 4. Herramientas CLI

```bash
# Dump OBJ con UVs/normales (mismo bake que Bevy)
cargo run -p openrailsrs-cli -- shape-obj-dump \
  "$HOME/Documentos/Open Rails/Content/.../RF_WP_DMBSA.s" \
  -o /tmp/DMBSA.obj --lod-distance-m 80

# Stats shape (existente)
cargo run -p openrailsrs-cli -- shape-dump path/to/file.s
```

Capturas automáticas: `OPENRAILSRS_SCREENSHOT=out.png OPENRAILSRS_SCREENSHOT_DELAY_S=12` (ver `viewer3d/src/capture.rs`).

---

## Aprendizajes (reglas para el futuro)

1. **Double-sided en shapes MSTS enmascara winding malo.** Si el texto se ve espejado, probar `FACE_COLORS=back` antes de tocar UVs.

2. **Flip V en UV (`1.0 - v`) es independiente del espejo lateral.** OR pasa U,V directos en muchos paths; nuestro flip V es correcto para Bevy; el espejo “NAMLLUP” era backface, no `FLIP_U`.

3. **MSTS → Bevy requiere invertir winding** igual que OR → XNA. Cualquier cambio en `shape_point_to_bevy` debe revisarse junto con el swap de índices.

4. **Cull OR sin winding correcto produce agujeros**, no arregla el texto. Orden: winding → cull → alpha.

5. **Alpha/z_bias y winding son capas distintas.** Fase 1 (opaco) no arregla fase 2 (espejo); no mezclar diagnósticos.

6. **Blue Pullman = EMU.** Dos entradas `Engine` en `.con`; no buscar locomotora separada en el trainset.

7. **Primitivas distintas pueden tener winding distinto** en el mismo shape; un swap global corrige la mayoría del lateral DMBSA (22/30 → mayoría outward) pero crestas/texto pueden necesitar revisión por `prim_state` si aparecen regresiones.

8. **Validar con OBJ + Blender** antes de iterar a ciegas en el viewer: `shape-obj-dump` + inspección de normales exteriores.

---

## Pendiente / no incluido en este fix

| Tema | Notas |
|------|-------|
| **Orientación DMBSA** (nariz hacia el interior del consist en chase) | Placement / flags `.eng` — revisar aparte de winding |
| **World scenery single-sided** | Winding flip es global en bake; materiales WORLD siguen double-sided — valorar alinear cull con OR en otro PR |
| **Dual-pass BlendATexDiff** (OR `ReferenceAlpha` 250/10) | Roadmap alpha avanzado |
| **`pullman_cab_window_parts_use_or_shader_on_dds`** | Test preexistente fallido (cab `.dds`), no bloqueante exterior |

---

## Archivos tocados (referencia)

| Área | Rutas |
|------|-------|
| Winding bake | `crates/openrailsrs-bevy-scenery/src/shapes/mesh.rs` |
| UV debug | `crates/openrailsrs-bevy-scenery/src/shapes/debug.rs` |
| Alpha / materiales | `crates/openrailsrs-bevy-scenery/src/shapes/material.rs` |
| prim_state parser | `crates/openrailsrs-formats/src/shape_binary.rs`, `typed/shape.rs` |
| Carga tren + materiales | `crates/openrailsrs-viewer3d/src/shapes.rs`, `live.rs`, `train.rs` |
| Log consist | `crates/openrailsrs-viewer3d/src/train_diagnostics.rs` |
| CLI OBJ | `crates/openrailsrs-cli/src/main.rs` (`shape-obj-dump`) |
| Matriz capturas | `scripts/pullman_visual_matrix.sh` |

---

## Verificación rápida post-fix

```bash
cargo test -p openrailsrs-viewer3d pullman_train_exterior pullman_exterior_alpha -q
```

Visual:

1. Lateral azul: **PULLMAN** legible, no espejado.
2. Carrocería sólida sin agujeros (sin flags debug).
3. `OPENRAILSRS_DEBUG_FACE_COLORS=back` → rojo con texto espejado ya **no** debe coincidir con el aspecto normal (el lateral correcto debe ser front-face verde en `FACE_COLORS=front`).

Captura de referencia post-fix: `tmp/pullman_matrix/FIX_chase.png`.

---

## Referencia Open Rails

- `openrails/Source/Orts.Common/Coordinates.cs` — winding invertido al cargar en XNA
- `openrails/Source/RunActivity/Viewer3D/Materials.cs` — `CullCounterClockwise`
- `openrails/Source/RunActivity/Viewer3D/Shapes.cs` — UV U,V sin flip arbitrario en bake OR

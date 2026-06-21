# Matrices de cabina MSTS (`M0`, `M4`, `M8`, `M9`, `M10`…)

Referencia para entender los **índices de matriz** que aparecen en logs, tests y código de cabina 3D (`cab_cvf.rs`, `shapes.rs`).

Relacionado: [`CABVIEW3D_SESSION_2026-06-19.md`](CABVIEW3D_SESSION_2026-06-19.md) · [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md)

---

## Qué significa «M8» o «M10»

**No son tipos de tren ni mandos del simulador.** Son abreviaturas de conveniencia:

| Notación | Significado |
|----------|-------------|
| **M*n*** | Matriz número *n* en el array `shape.matrices[]` del archivo `.s` de cabina |
| **M0** | Casi siempre la matriz raíz **`MAIN`** (marco estático de la cabina) |
| **M8** | La matriz cuyo nombre CVF es `THROTTLE:0:0` (regulador / aceleración) |
| **M9** | `TRAIN_BRAKE:0:0` — primer sub-mando del freno de tren |
| **M10** | `TRAIN_BRAKE:0:1` — **segundo** sub-mando del **mismo** freno de tren |

En MSTS/Open Rails cada mando del `.cvf` puede tener **una o más matrices** en el shape 3D. El nombre sigue el patrón:

```text
<NOMBRE_CONTROL>:<estado>:<subparte>
```

Ejemplos en Pullman (`PULLMAN_GR.s`):

```text
THROTTLE:0:0        → índice 8
TRAIN_BRAKE:0:0     → índice 9
TRAIN_BRAKE:0:1     → índice 10   ← misma palanca lógica, sub-parte distinta
DIRECTION:0:0       → índice 4
```

**M9 y M10 no son «freno 9» y «freno 10»:** son dos huesos del **mismo** control `TRAIN_BRAKE` (parte `:0` y sub-parte `:1`). Open Rails puede animar ambos con el mismo valor de freno.

---

## Inventario Pullman (`PULLMAN_GR.s`, LOD0)

El shape tiene **18 matrices** (índices 0–17). Todas las primitivas del mesh usan `vtx_state.matrix_idx = 0` (MAIN) en el bake estático; las matrices 1–17 son **pivotes lógicos** para animación CVF, no huesos de skinning clásicos.

| Índice | Nombre en `.s` | Control CVF | Uso en OR / viewer |
|--------|----------------|-------------|-------------------|
| **0** | `MAIN` | — | Marco fijo de toda la cabina |
| **1** | `AMMETER:0:0` | Amperímetro | Aguja multi-state (visibilidad) |
| **2** | `BRAKE_CYL:0:0` | Cilindro freno | Instrumento |
| **3** | `BRAKE_PIPE:0:0` | Tubería freno | Instrumento |
| **4** | `DIRECTION:0:0` | Inversor / sentido | Palanca FWD–OFF–REV |
| **5** | `HORN:0:0` | Bocina | — |
| **6** | `MAIN_RES:0:0` | Reservorio principal | Manómetro |
| **7** | `SPEEDOMETER:0:0` | Velocímetro | Aguja |
| **8** | `THROTTLE:0:0` | Regulador | Rueda del acelerador |
| **9** | `TRAIN_BRAKE:0:0` | Freno tren | Palanca izquierda + rueda 3D derecha |
| **10** | `TRAIN_BRAKE:0:1` | Freno tren (sub) | Segunda pieza del mismo mando |
| 11–17 | `EXTERNALWIPERS` / `WIPERS` | Limpiaparabrisas | Exterior / cabina |

**Sí hay M1** (`AMMETER:0:0`), **M2**, **M3**, etc. En logs solo destacamos M4/M8/M9/M10 porque son los **mandos que el jugador mueve** con teclado en live.

---

## Cómo se enlaza una malla 3D a una matriz

El `.cvf` define mandos 2D (sprites). El `.s` define geometría 3D. **No hay tabla automática** «textura → matriz»: el viewer debe adivinar qué primitiva del pupitre corresponde a cada pivote CVF.

Flujo en `openrailsrs-viewer3d`:

1. Parser lee nombres `THROTTLE:0:0` → `matrix_drivers[8] = Lever(Throttle)`.
2. Al cargar meshes, `cab_matrix_for_prim()` asigna `cab_matrix_idx` por textura + proximidad al pivote.
3. Cada entidad con `CabCvfPart { matrix_idx }` rota según telemetría live.

Bindings actuales (Pullman, tras fix 2026-06):

| sub / prim | Textura | Matriz | Pieza visible |
|------------|---------|--------|---------------|
| 0 / 9 | `Controller_base.ace` | **M8** | Rueda del regulador (centro pupitre) |
| 1 / 8 | `Controls.ace` | **M9** | Palanca gris freno (izquierda, como OR 2D) |
| 9 / 8 | `Controls.ace` | **M10** | Segunda placa del freno (sub-parte CVF) |
| 2 / 19 | `Brake_wheel.ace` | **M9** | Rueda negra freno (derecha; pivote en centro malla) |
| 1 / 10 | `switch panel.ace` | **M4** | Palanca inversor (FWD/OFF/REV) |

Pivotes aprox. (MSTS → Bevy, Z invertido):

| Matriz | Pivote Bevy `(x, y, z)` |
|--------|-------------------------|
| M4 | `(-0.41, 2.53, -9.42)` |
| M8 | `(-0.51, 2.45, -9.34)` |
| M9 | `(-1.19, 2.47, -9.27)` |
| M10 | `(-1.19, 2.46, -9.14)` |

---

## Problemas que aparecieron (y cómo se resolvieron)

### 1. Regulador M8 «volaba» al subir throttle

**Síntoma:** al pulsar ↑, la rueda orbitaba lejos del pupitre en lugar de girar sobre su eje.

**Causa:** desajuste entre bake con cadena `M8×M0`, rebase al pivote y transform Bevy vs convención XNA de Open Rails.

**Fix:** bake solo **M0**; rebase al pivote de **M8** individual; entidad con `matrix43_to_transform(M8)` + rotación fallback en eje **Y** local.

**Test:** `pullman_throttle_rebased_rest_pose_matches_bake`.

---

### 2. Confundir M8 con piezas `Controls.ace`

**Síntoma:** chips del tablero o placas verticales giraban con el acelerador.

**Causa:** heurística «sub_object *i* → matriz *i*» enlazaba `Controls.ace` de sub 4/8 a M4/M8.

**Fix:** selección exclusiva por textura `Controller_base.ace` + mayor radio cerca del pivote M8; excluir `Controls.ace` del sub 8.

---

### 3. Sub_object entero ligado a M10

**Síntoma:** mesa o asiento rotaban al frenar.

**Causa:** un `sub_object` completo recibía `cab_matrix_idx = 10`.

**Fix:** enlace **por primitiva** (`sub_object`, `prim_state`), no por sub_object entero.

---

### 4. Freno: pivote M9 lejos de la rueda 3D (`Brake_wheel`)

**Síntoma:** la rueda negra a la **derecha** (~1.3 m del pivote M9 en la **izquierda**) no se enlazaba (filtro proximidad 0.35 m).

**Causa:** en OR el CVF 2D pone `BrakeHandle` a la izquierda; la rueda 3D es otra malla decorativa/funcional en el lado opuesto del pupitre.

**Fix:**

- **M9** → `Controls.ace` junto al pivote (palanca izquierda, dist ~0.05 m).
- **M9** también → `Brake_wheel.ace` (rueda derecha) con **`lever_pivot_at_mesh_center`**: rota sobre su centro, no sobre el pivote CVF lejano.
- **M10** → segunda `Controls.ace` (sub 9 prim 8), excluyendo la ya asignada a M9.

**Problema colateral:** la misma matriz M9 anima **dos** entidades (palanca + rueda) con distinto pivote espacial pero mismo valor de freno — correcto para paridad OR.

---

### 5. Inversor M4 enlazado a placa minúscula

**Síntoma:** `Controls.ace` sub 4 (radio ~8 mm) enlazada a M4; no se veía movimiento de palanca.

**Causa:** placa de etiqueta, no la palanca FWD/OFF/REV de las capturas OR.

**Fix:** enlazar `switch panel.ace` más cercana al pivote M4 (sub 1 prim 10, dist ~0.37 m).

**Sim:** `driver_direction` 0 = REV, 0.5 = NEU, 1 = FWD; teclas `[` / `]` / `\`.

---

### 6. Conflicto M9 vs M10 al elegir `Controls.ace`

**Síntoma:** la palanca más cercana a M9 se asignaba a M10 porque el código comprobaba M10 antes que M9.

**Fix:** orden M9 → M10; picker M10 excluye prims ya reclamadas por M9 (`pick_exclusive_controls_lever_excluding`).

---

### 7. Sin bloque `animations` en el `.s`

**Síntoma:** Open Rails tampoco animaría estas matrices por keyframes en el shape Pullman.

**Mitigación:** `fallback_lever_rotation` — rotación procedural según telemetría (throttle %, brake %, direction).

---

## Teclas live (cabina)

| Tecla | Efecto | Matrices afectadas |
|-------|--------|-------------------|
| ↑ / ↓ | Regulador / freno | M8, M9, M10 (+ rueda M9) |
| `[` / `]` / `\` | Inversor REV / FWD / NEU | M4 |
| Panel **C** | HUD `INV REV/FWD/NEU` | — |

---

## Verificación

```bash
cargo test -p openrailsrs-viewer3d pullman_cab_lever_bindings_when_content_present
cargo test -p openrailsrs-viewer3d pullman_cvf_lever_binding_diagnostics -- --nocapture
```

Requiere Content OR: `…/RF_Blue_Pullman/Cabview3d/PULLMAN_GR.s`.

---

## Glosario rápido

| Término | Definición |
|---------|------------|
| **Matriz / bone** | Transform 4×3 en el `.s`; pivote de animación CVF |
| **MAIN (M0)** | Raíz; casi todo el mesh estático cuelga de aquí |
| **Lever** | Mando que rota (throttle, freno, inversor) |
| **Multi-state** | Instrumento con estados discretos (aguja, dígitos) |
| **Rebase** | Vértices pasados a espacio local del pivote antes de animar |
| **CVF** | Cab View File — define mandos 2D y nombres de control |

---

## Cómo lo hace Open Rails (código fuente)

Referencia en el repo: `openrails/Source/RunActivity/Viewer3D/RollingStock/MSTSLocomotiveViewer.cs` (`ThreeDimentionCabViewer`), `AnimatedPart.cs`, `Shapes.cs`.

### 1. No hay heurística mesh ↔ matriz

OR **no adivina** qué primitiva 3D corresponde a M8 o M6. Recorre `shape.MatrixNames[]` por índice:

```csharp
for (int iMatrix = 0; iMatrix < MatrixNames.Count; ++iMatrix) {
    matrixName = MatrixNames[iMatrix];  // p.ej. "THROTTLE:0:0"
    typeName = matrixName.Split('-')[0]; // quita sufijo "-PartN"
    // → clave CVF (THROTTLE, order 0)
    tmpPart.AddMatrix(iMatrix);          // registra el ÍNDICE, no un mesh
}
```

Convención de nombre en el `.s`:

```text
TYPE:Order:Parameter-PartN
```

Ejemplo: `ASPECT_SIGNAL:0:0-1` y `:0:0-2` son dos sub-partes del mismo control.

### 2. Un solo shape animado, no entidades sueltas

OR mantiene `PoseableShape.XNAMatrices[]` — un array global de transforms. Al renderizar, **cada primitiva** multiplica la cadena de jerarquía:

```csharp
hi = shapePrimitive.HierarchyIndex;
while (hi >= 0)
    xnaMatrix *= animatedXNAMatrices[hi];
    hi = Hierarchy[hi];
```

La animación es `AnimateOneMatrix(iMatrix, key)` → interpola keyframes del bloque `animations` del `.s` (slerp_rot / linear_key).

En `openrailsrs` usamos **entidades Bevy separadas** por primitiva con rebase local — enfoque distinto, necesario cuando todo el mesh cuelga de M0.

### 3. Tipos de control 3D en OR

| Tipo CVF | Clase OR | Comportamiento 3D |
|----------|----------|-------------------|
| **Dial** estilo `NEEDLE` (POINTER) | `AnimatedPartMultiState` | Rota matriz(es) vía keyframes |
| **Dial** barra / digital | `ThreeDimCabGaugeNative` | **Dibuja** geometría procedural en la matriz (no rota mesh existente) |
| **Digit** | `ThreeDimCabDigit` | Quads de dígitos en la matriz |
| **Lever / TwoState / TriState** | `AnimatedPartMultiState` | `GetRangeFraction()` o `GetDrawIndex()` → frame de animación |
| **HORN** | En CVF Pullman: `TwoState` 2D | Sprite `hornlever.ace` en panel — no M5 3D |
| **EXTERNALWIPERS** | `AnimatedPartMultiState` | `UpdateLoop(Wiper)` con keyframes |

`PrepareFrame` actualiza telemetría → `SetFrameClamp` → `AnimateMatrix` → render con `MatrixVisible[]` para multi-estado discreto.

### 4. Pullman: sin bloque `animations` en el `.s`

Condición crítica en el constructor de `ThreeDimentionCabViewer`:

```csharp
if (TrainCarShape != null && TrainCarShape.SharedShape.Animations != null)
{
    // … solo aquí se registran M1–M17 en AnimateParts / Gauges
}
```

`PULLMAN_GR.s` **no trae keyframes**. En OR:

- El constructor **no registra** matrices CVF para animación 3D.
- `AnimateOneMatrix` retorna de inmediato si no hay `animations`.
- Los mandos visibles son el **CVF 2D** (`PULLMAN_GR.cvf`): Dial, Lever, TwoState sobre sprites `.ace`.

Contenido real del CVF Pullman (extracto):

| Control CVF | Tipo 2D | Gráfico |
|-------------|---------|---------|
| HORN | TwoState | `hornlever.ace` |
| SPEEDOMETER | Dial NEEDLE | `KMHNeedle.ace` |
| AMMETER | Dial NEEDLE | `cab.ace` |
| MAIN_RES | Dial NEEDLE | `KPANeedleRed.ace` |
| BRAKE_CYL / BRAKE_PIPE | Dial NEEDLE | `cab.ace` |
| THROTTLE | Lever | `Throttle.ace` |
| DIRECTION | TriState | `Reverser.ace` |
| TRAIN_BRAKE | Lever | `BrakeHandle.ace` |
| WIPERS | TwoState | `cab.ace` |

Las matrices M1–M17 en el `.s` son **pivotes lógicos** para shapes que sí traen animación; en Pullman OR las ignora y usa el panel 2D.

### 5. Implicación para `openrailsrs`

| Enfoque OR | Nuestro enfoque actual |
|------------|------------------------|
| Índice de matriz = nombre en `.s` | Heurística textura + proximidad al pivote |
| Requiere `animations` + jerarquía | Fallback rotación procedural sin keyframes |
| Gauges barra = geometry procedural | Intentar rotar mallas `Instruments.ace` |
| Pullman 3D = estático + CVF 2D | Animar palancas 3D enlazadas a mano (M4/M8/M9/M10) |

**Por qué falla “implementar todos los M” con proximidad:** M6 (`MAIN_RES`) no tiene malla dedicada enlazada por nombre; compite con M1–M7 por las mismas piezas `Instruments.ace`. OR no resuelve eso en 3D — dibuja la aguja en 2D sobre el CVF.

### 6. Camino alineado con OR (si se quiere paridad)

1. **Corto plazo (Pullman):** mantener fallback 3D en palancas ya validadas; panel HUD/CVF 2D para gauges (como OR).
2. **Medio plazo:** overlay CVF 2D en Bevy UI — ✅ `cab_cvf_overlay.rs` (Dial/Lever/TwoState/MultiState desde `.cvf` + ACE).
3. **Largo plazo:** si el shape trae `animations`, aplicar `XNAMatrices[i]` en cadena de jerarquía al render (como `PrepareFrame` OR), no entidades sueltas por primitiva.

Archivos OR clave:

| Archivo | Rol |
|---------|-----|
| `MSTSLocomotiveViewer.cs` ~3360 | `ThreeDimentionCabViewer` — registro matrices por nombre |
| `AnimatedPart.cs` | `AddMatrix`, `SetFrameClamp`, `AnimateMatrix` |
| `Shapes.cs` ~336 | `AnimateOneMatrix` — keyframes |
| `Shapes.cs` ~2537 | `PrepareFrame` — jerarquía × `XNAMatrices` |
| `MSTSLocomotiveViewer.cs` ~4029 | `ThreeDimCabGaugeNative` — gauges no-POINTER |

# Cabina 3D (`CABVIEW3D`)

Vista conductor en `viewer3d --live` (Pullman Chiltern: `RF_Blue_Pullman` / `PULLMAN_GR.s` + `.cvf`).

## Estado

| Pieza | Estado |
|-------|--------|
| `DriverCam` (**1** / Alt+1, paridad OR) | ✅ |
| Mesh `CABVIEW3D/*.s` + ACE | ✅ |
| Shader `or_cab` (TexDiff) | ✅ |
| Cámara `ORTS3DCabHeadPos` | ✅ |
| Dirección inicial / FOV Open Rails | ✅ `StartDirection` en espacio de la cabina + 45° vertical |
| Ocultar exterior en L1 | ✅ |
| CVF overlay en cabina 3D | ❌ off (#151) |
| Matrices nativas `TYPE:orden[:p1[:p2]]` (#157) | ✅ parse + MultiState/Dial + Digit/GaugeNative quads |
| Pantalla ETCS / `ScreenDisplay` (#158–#163) | ✅ DMI + TCS Rust (`BasicEtcsTcs`: supervisión/TTI/menús) |
| Vista cabina 2D (**Alt+1** si preferís 3D; o **1** con prefer 2D) | ✅ ACE + CVF (#152) |
| Cab2d Digital / MouseControl / Direction / NIGHT | ✅ |
| Mandos 3D (acelerador/freno/inversor/bocina) | ✅ animación `.s`; fallback CVF sobre el pivote authored si faltan controladores |
| Panel HUD (tecla **C**) | ✅ (solo cabina 3D; cámara = **1**) |
| UV canónicas OR (#165) | ✅ smoke Pullman Chiltern OK (2026-07) |
| Jerarquía / SortIndex / winding (#166) | ✅ bake cab = OR `totalPrimitiveIndex`; opacos Back-cull |
| Oclusores al mirar (#167) | ✅ diag `OPENRAILSRS_CAB_DEBUG=occluder`; opacos sin double-sided |
| Goldens cab multi-vista (#170 slice) | 🔶 `visual_regression_chiltern.sh` (frente/arriba/izq/der) |

## UV (#165)

Conversión única en `shape_uv_to_bevy` (`openrailsrs-bevy-scenery`): coords OR authored `(u, v)` → Bevy `Vec2`, **sin** V-flip global ni allowlist UV180 por nombre de atlas (`Instruments2`, etc.).

- Debug opcional: `OPENRAILSRS_DEBUG_FLIP_U` / `_V` / `_UV` / `OPENRAILSRS_DEBUG_NO_UV_FLIP`.
- Tests: anti-resurrección de helpers UV180-by-name; fixture asimétrico Instruments/Instruments2.
- Smoke: `viewer3d --live` Chiltern → tecla **1** → pupitre/instrumentos correctos (confirmado).

Cola visual: **#166** / **#167** cerrados en bake+diag; **#170** slice cab multi-vista en `./scripts/visual_regression_chiltern.sh` (chase/orbit/máscaras OR quedan abiertos).

### SortIndex / winding (#166)

`build_mesh_parts_from_shape_lod_cab` avanza `SortIndex` como OR (`++totalPrimitiveIndex`) aunque un prim se descarte. Opacos `OrCabMaterial` hacen **Back cull** (overlays sin depth-write siguen double-sided).

### Oclusores (#167)

Con `OPENRAILSRS_CAB_DEBUG=occluder`, el HUD/log atribuye el primer AABB de cabina bajo el rayo de mirada (`prim_state`, textura, distancia, `EYE_INSIDE`). No son sombras Bevy (`NotShadowCaster`).

## Matrices CVF (Pullman)

| Matriz | Rol típico |
|--------|------------|
| M0 | Raíz / body cab |
| M4 | Inversor / selector |
| M5 | Bocina |
| M8–M10 | Palancas thr/brk (bindings `.cvf`) |

Detalle de bindings: `cab_cvf.rs` + tests Pullman. Si el `.s` trae controladores MSTS, se respetan sus keyframes. En cabinas como `PULLMAN_GR.s`, que declaran los huesos `DIRECTION`, `HORN`, `THROTTLE` y `TRAIN_BRAKE` pero no incluyen ningún bloque de animación, el viewer aplica un fallback suave sobre la matriz y el pivote authored: acelerador y freno recorren su arco, el inversor centra neutral entre avance/retroceso y la bocina se deprime mientras está activa. Otros huesos desconocidos permanecen estáticos.

Debug: `OPENRAILSRS_CAB_DEBUG=uv|albedo|vcolor`.

## Env

| Variable | Default | Efecto |
|----------|---------|--------|
| `OPENRAILSRS_CAB_ALBEDO` | `1.0` | Tint |
| `OPENRAILSRS_CAB_SUN` | on | Sol/ambiente OR en TexDiff (`0` apaga) |
| `OPENRAILSRS_CAB_OR_LIKE` | off | Brillo fijo legacy (debug) |
| `OPENRAILSRS_CAB_MIN_BRIGHT` | `0.55` | Piso de brillo (techo/placas); `0` = estricto OR |
| `OPENRAILSRS_CAB_BRIGHTEN` | off | Levantar ACE oscuros (`1` si aún se ven apagados) |
| `OPENRAILSRS_CAB_FOV` | `45` | FOV vertical; equivale al `ViewingFOV` predeterminado de Open Rails |
| `OPENRAILSRS_FOLLOW` | — | `driver`/`cab3d` → 3D; `cab`/`cab2d` → 2D |
| `OPENRAILSRS_CAB_NIGHT` | off | Forzar ACE `NIGHT/` en Cab2d |

Teclas Cab2d: **←/→** vista (Direction CVF) · click/arrastre en palancas con `MouseControl`.

Cab2d y cabina 3D componen el mismo forward: `.eng` `StartDirection` + `CabView.Direction` del CVF (X positivo = mirar abajo). La cámara que sigue al coche usa directamente el eje −Z de la malla convertida, que apunta al parabrisas; la corrección −90° shape→tren queda reservada al fallback sin entidad de coche. Así no se duplica el cambio de base cuando la orientación física del tren ya está aplicada. Cab2d no anula el pitch/yaw authored (#169).

El valor de referencia es FOV vertical 45°: `UserSettings.ViewingFOV` de Open Rails declara `Default(45)`. Puede sobrescribirse con `--cab-fov DEG` o `OPENRAILSRS_CAB_FOV`. Open Rails conserva el mouse-look al volver a la cabina; por eso una captura guardada puede tener una elevación distinta de `StartDirection` aunque use el mismo eyepoint.

Cabina 3D: mirada con **RMB** (límites amplios; no se aplica `RotationLimit` del `.eng`, como OR). **LMB** en la pantalla ETCS (`ScreenDisplay`) activa soft keys del DMI (scroll, scale, Main/Data/Sett. → subventanas, ack mensajes).

Parámetro CVF `mode`: `full` (default) · `SpeedArea` · `PlanningArea` · `GaugeOnly`.

TCS: `openrailsrs_sim::etcs::BasicEtcsTcs` (sin scripts C#). Soft keys / menús desde defs del TCS; supervisión y TTI desde límite + distancia a parada + curva de frenado.

Símbolos ERA: `Content/ETCS` (o `OPENRAILSRS_ETCS_CONTENT` / fixtures `docs/fixtures/etcs`).

Instrumentos (`Instruments*.ace`): mips ACE completos; agujas con offset 1.5 mm. Pullman marca casi todo `ZBufMode=1` (OR dibuja la cabina en un pase tardío); en Bevy los materiales **opacos** escriben depth para que el WORLD no tape pupitre/suelo. MSAA no se activa al entrar en cabina (toggle en runtime rompe pipelines Bevy 0.19).

## Rendimiento de cabina

La cabina conserva su respuesta física a la frecuencia de simulación/render, pero evita trabajo visual que no produce un cambio:

- Entrar en `DriverCam` resuelve y parsea `.eng`/`.cvf`/shape una sola vez; el estado ya cargado se reutiliza mientras la cabina siga activa.
- La actualización de palancas usa por referencia el runtime CVF y sólo marca `Transform`/`Visibility` como cambiados cuando el valor resultante difiere.
- `GaugeNative` sólo reconstruye geometría cuando cambia su fracción visible; `Digit` sólo cuando cambia el texto formateado.
- La pantalla ETCS mantiene lógica e input por frame, pero limita el repintado y la subida de su imagen RGBA de 640×480 a **20 Hz**.
- Cambiar la visibilidad del tren/cabina recorre la jerarquía únicamente al cambiar de cámara o aparecer una nueva entidad.

Toda la geometría de la cabina, sus instrumentos y la cámara lleva la exclusión de emisión de sombra. La cámara no contiene un `Mesh3d`, por lo que no puede proyectar una sombra; un borde que se desplace con ella debe diagnosticarse como límite de cascada, no como geometría de cámara.

## Teclas (paridad Open Rails)

| Tecla | OR / ORRS |
|-------|-----------|
| **1** | Entrar cabina (3D por defecto, como `Use3DCab`) |
| **Alt+1** | Alternar cabina 2D ↔ 3D |
| **Ctrl+Shift+1** | Ciclar eyepoint 3D |
| **2** | Exterior frontal (chase) |
| **3** | Exterior órbita |
| **5** | Pasajero (asiento; cicla vagones con `Inside`) |
| **Ctrl+Shift+5** | Ciclar asientos del vagón actual |
| **8** | Cámara libre (fly) |
| **A** / **D** | Throttle − / + |
| **;** / **'** | Freno tren − / + |
| **W** / **S** | Reverser FWD / REV |
| **\\** | Reverser neutral |
| **Space** | Bocina |
| **V** | Limpiaparabrisas |
| **Backspace** | Emergencia (freno a fondo) |

## Arranque

```bash
cargo run --release -p openrailsrs-viewer3d -- \
  --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
# Full scenery: omitir --run-corridor
```

Setup: [`CHILTERN.md`](CHILTERN.md).

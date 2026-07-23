# Cabina 3D (`CABVIEW3D`)

Vista conductor en `viewer3d --live` (Pullman Chiltern: `RF_Blue_Pullman` / `PULLMAN_GR.s` + `.cvf`).

## Estado

| Pieza | Estado |
|-------|--------|
| `DriverCam` (**1** / Alt+1, paridad OR) | ✅ |
| Mesh `CABVIEW3D/*.s` + ACE | ✅ |
| Shader `or_cab` (TexDiff) | ✅ |
| Cámara `ORTS3DCabHeadPos` | ✅ |
| Ocultar exterior en L1 | ✅ |
| CVF overlay en cabina 3D | ❌ off (#151) |
| Matrices nativas `TYPE:orden[:p1[:p2]]` (#157) | ✅ parse + MultiState/Dial + Digit/GaugeNative quads |
| Pantalla ETCS / `ScreenDisplay` (#158–#161) | ✅ DMI Full + soft keys clicables (LMB → UV) |
| Vista cabina 2D (**Alt+1** si preferís 3D; o **1** con prefer 2D) | ✅ ACE + CVF (#152) |
| Cab2d Digital / MouseControl / Direction / NIGHT | ✅ |
| Panel HUD (tecla **C**) | ✅ (solo cabina 3D; cámara = **1**) |

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
| `OPENRAILSRS_CAB_MIN_BRIGHT` | `0.55` | Piso de brillo (techo/placas); `0` = estricto OR |
| `OPENRAILSRS_CAB_BRIGHTEN` | off | Levantar ACE oscuros (`1` si aún se ven apagados) |
| `OPENRAILSRS_FOLLOW` | — | `driver`/`cab3d` → 3D; `cab`/`cab2d` → 2D |
| `OPENRAILSRS_CAB_NIGHT` | off | Forzar ACE `NIGHT/` en Cab2d |

Teclas Cab2d: **←/→** vista (Direction CVF) · click/arrastre en palancas con `MouseControl`.

Cab2d `Direction` usa el mismo signo Bevy que `StartDirection` (X positivo = mirar abajo).

Cabina 3D: mirada con **RMB** (límites amplios; no se aplica `RotationLimit` del `.eng`, como OR). **LMB** en la pantalla ETCS (`ScreenDisplay`) activa soft keys del DMI (scroll mensajes, scale planning, menú).

Símbolos ERA: `Content/ETCS` (o `OPENRAILSRS_ETCS_CONTENT` / fixtures `docs/fixtures/etcs`).

Instrumentos (`Instruments*.ace`): mips ACE completos; agujas con offset 1.5 mm. Pullman marca casi todo `ZBufMode=1` (OR dibuja la cabina en un pase tardío); en Bevy los materiales **opacos** escriben depth para que el WORLD no tape pupitre/suelo. MSAA no se activa al entrar en cabina (toggle en runtime rompe pipelines Bevy 0.19).

UV 180° en caras grandes de atlas “invertidos” tras el V-flip MSTS (`Instruments2`, `Cab2`, `Loudaphone2`, `handbook`, techo, asiento, etc.). Sin rotar: `Instruments`, `Cab1`, `DESK1`, `Controls` (ya correctos).

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

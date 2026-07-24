# Viewer 3D — estado y gaps

App jugable Bevy (`openrailsrs-viewer3d`). Arquitectura: [`BEVY.md`](BEVY.md). Tests/comandos: [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

## Pipeline (resumen OR → Bevy)

1. **Datos:** `.trk` / TDB / WORLD `.w` / shapes `.s` + ACE → parsers en `formats` + `MstsRouteCatalog`.
2. **Espacios:** MSTS world → render (resta foco + `height_origin`) → view (floating origin XZ). Detalle: [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md).
3. **Stream:** radio `OPENRAILSRS_VIEW_RADIUS_M` (default 2000 m) + tilebundles; unload con histéresis. Densidad scenery OR: `OPENRAILSRS_WORLD_OBJECT_DENSITY` (0–99, default 49) vs `StaticDetailLevel` (#141).
4. **Spawn:** terreno chunked, WORLD (instancing opacos #58), Transfer/Forest/agua/señales/carspawn, tren + cabina.
5. **Materiales:** Standard + OrScenery/OrTerrain/OrForest; fog on by default (`F` pone densidad 0, no quita el componente).

## Estado fases jugables

| Fase | Tema | Estado |
|------|------|--------|
| A | `--live` + física en viewer | ✅ |
| B | Terreno + WORLD + stream | ✅ (paridad visual residual) |
| C | Cabina 3D + CVF | 🔶 #165–#167 ✅; goldens cab slice #170; resto [`CABVIEW3D.md`](CABVIEW3D.md) |
| D | Audio en viewer | 🔲 |
| E | Vía TDB/peralte vs grafo | 🔶 [`TRACK_MSTS.md`](TRACK_MSTS.md) |
| F | Activity / señales sin assume-clear | 🔶 |
| G | Modo juego completo | 🔲 |

## Gaps residuales (map rendering)

Casi todo el lote P0–P2 de map rendering (2026-07) está **cerrado** (issues #25–#83, #109–#125). Residuales típicos:

- Paridad pixel OR (checklist manual / goldens Chiltern locales #71).
- Pop-in vs `ViewingDistance` OR.
- Cabina: palancas CVF parciales; puertas/panto sim → visual (#81).
- Cast/receive sombras instanced ✅ (#72); VSM completo solo en render3d.

### Alpha / sorting / instancing / night (cerrados)

| Tema | Notas |
|------|--------|
| SortIndex (#102) | Bake conserva orden de archivo; `depth_bias` nudge en viewer3d + render3d |
| Dual-pass BlendATex* (#101) | Mask(250)+Blend en ACE/DDS scenery (StandardMaterial); cab single-pass |
| Instancing light model (#138) | Batch GPU solo TexDiff/Unknown sin unlit/emissive ni PBR metálico fuerte; Tex→FullBright, `metallic>0.1`, textura metallic-roughness y resto → entity path |
| Affine Matrix3x3 (#139/#174) | Shear = `linear_requires_affine` (no `linear.is_some`); GPU Mat4 si N≥4, else bake mesh + TRS traslación |
| Night/Underground (#142) | Flag Underground; selector sol/túnel; Night local→padre DDS→ACE; `OPENRAILSRS_SCENERY_NIGHT` |
| Streaming A→B→A (#144) | Test de membresía load/unload en `stream.rs` |
| PAT `start_offset_m` (#132) | Ancla = cabeza; TrackPDP ignora `DistanceDownPath` |
| Pose por coche (#128) | `update_consist_car_track_poses` — chainage absoluto (incluye `start_offset_m`) e individual en curvas |
| Inicio live + cámara | ID `eNNNN` validado espacialmente; chase cercano sobre los coches delanteros |

#### Materiales metálicos e instancing

El shader WORLD instanciado transporta únicamente albedo y corte alpha; no transporta
`metallic`, `roughness` ni `reflectance`. Iluminar como Lambert un material fuertemente
metálico satura a blanco las cabezas de riel (`RailHead_*.ace`, `ukfs_rail.ACE`) bajo
el sol HDR. Por eso los materiales con `metallic > 0.1` o textura
metallic-roughness usan automáticamente el camino entity/PBR. El resto de cada shape
puede continuar instanciado.

Para diagnóstico, `OPENRAILSRS_WORLD_INSTANCING=0` mantiene disponible el opt-out
global, con mayor cantidad de entidades y draw calls.

#### Iluminación HDR de instancias WORLD

Las luces exteriores usan unidades físicas (sol y ambiente en lux) junto con
`Exposure::SUNLIGHT`. El shader instanciado debe aplicar tanto la exposición de la
cámara como la normalización Lambert `1/π`. Multiplicar directamente el albedo por
los valores físicos —el comportamiento anterior— saturaba a blanco edificios,
andenes y otras shapes repetidas aunque sus texturas ACE estuvieran cargadas.

El camino instanciado conserva sombras y niebla, pero ahora calcula:
`albedo × exposure × (ambient + sun × NdotL/π × shadow)`. Los materiales PBR
fuertemente metálicos continúan en el camino entity/PBR descrito arriba.

#### Continuidad de vías al cambiar LOD

Las bandas LOD de una shape MSTS no conservan necesariamente la posición de sus
submallas. Una vía puede pasar de `[balasto, lateral, sillas, cabeza, durmientes]`
a `[balasto, lateral, cabeza, durmientes]`; usar el índice del vector hacía que
`cabeza` se reemplazara por `durmientes` y el riel pareciera terminar en seco al
cruzar el umbral de distancia.

El cambio de LOD WORLD identifica ahora cada parte por
`(sub_object_idx, prim_state_idx)`, tanto en entidades como en grupos GPU. La
geometría base se crea desde la banda más detallada para no perder partes que
puedan reaparecer al acercar la cámara. Si una banda omite deliberadamente una
parte, se oculta y vuelve a mostrarse cuando esa identidad existe otra vez.

Había además una pérdida de precisión anterior al floating origin: convertir
directamente la posición absoluta MSTS (~12–30 millones de metros en Chiltern) a
`f32` cuantizaba X/Z en pasos de 1–2 m. Cada `WorldObject` conserva ahora el
residuo submétrico de la conversión y lo suma sólo después de restar
`RouteFocus`, cuando la posición ya está cerca de cero. Así dos tramos
consecutivos mantienen las coordenadas decimales escritas en `.w`.

Para aislar visualmente un problema de LOD se puede forzar la banda más detallada
con `OPENRAILSRS_LOD_BIAS=100`.

#### Tren y cámara al iniciar una actividad live

El ID numérico de una arista del grafo no se acepta automáticamente como el mismo
vector TDB. Sus extremos deben coincidir espacialmente; si están lejos, la pose usa
el centro de vía TDB más cercano al punto del grafo. Esto evita que el consist
aparezca en otro sector de la ruta aunque `eNNNN` exista en ambas fuentes.

La posición de cada coche se calcula desde el *chainage* absoluto de la cabeza,
incluido `start_offset_m`, y luego aplica su desplazamiento dentro del consist. La
orientación convierte explícitamente `TrackPose +Z` a `train +X`, por lo que la
carrocería queda longitudinal sobre el riel.

En `--live` el modo inicial es `chase`: la cámara apunta unos metros delante del
coche de cabeza y queda elevada sobre los primeros vehículos. Así permanece dentro
de estaciones cubiertas como Paddington. La dirección se deriva de las posiciones
reales del primer y último coche; `OPENRAILSRS_FOLLOW` continúa permitiendo
sobrescribir el modo inicial.

## Comando rápido

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"
OPENRAILSRS_VIEW_RADIUS_M=300 cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Setup Wine/OR: [`CHILTERN.md`](CHILTERN.md). Física vs OR: [`OR_PARITY.md`](OR_PARITY.md).

## Troubleshooting ventana / GPU

### Wayland + GPU híbrida (AMD iGPU + NVIDIA)

Síntoma típico tras cargar el mundo:

```text
failed to import supplied dmabufs: Could not bind the given EGLImage to a CoglTexture2D
Protocol error 7 on object @0
winit event loop returned an error: Exit Failure: 1
```

Causa habitual: Bevy/Vulkan renderiza en **RADV (AMD)** mientras Mutter presenta con **NVIDIA**, o el driver NVIDIA está en **Driver/library version mismatch** (`nvidia-smi` falla). El compositor no puede importar los dmabufs.

Mitigaciones (en orden):

1. **Reiniciar** para alinear módulo kernel NVIDIA y userspace (`nvidia-smi -L` debe listar la GPU sin mismatch).
2. Sesión **Ubuntu on Xorg** (no Wayland).
3. Forzar Mutter/primary GPU AMD si el escritorio usa NVIDIA.
4. Present mode: `OPENRAILSRS_PRESENT_MODE=fifo` (default es `auto_vsync`). El workspace ya habilita features Bevy `wayland` + `x11`.

El viewer imprime un aviso al arrancar si detecta varios `/dev/dri/renderD*` y `nvidia-smi` roto.

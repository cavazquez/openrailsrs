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
| C | Cabina 3D + CVF | 🔶 [`CABVIEW3D.md`](CABVIEW3D.md) |
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
| Instancing light model (#138) | Batch GPU solo TexDiff/Unknown sin unlit/emissive; Tex→FullBright y resto → entity path |
| Affine Matrix3x3 (#139) | `linear: Mat3` en pose + GPU instance Mat4 (shear); Transform TRS solo aproximación |
| Night/Underground (#142) | Flag Underground; selector sol/túnel; Night local→padre DDS→ACE; `OPENRAILSRS_SCENERY_NIGHT` |
| Streaming A→B→A (#144) | Test de membresía load/unload en `stream.rs` |
| PAT `start_offset_m` (#132) | Ancla = cabeza; TrackPDP ignora `DistanceDownPath` |
| Pose por coche (#128) | `update_consist_car_track_poses` — chainage individual en curvas |

## Comando rápido

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"
OPENRAILSRS_VIEW_RADIUS_M=300 cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Setup Wine/OR: [`CHILTERN.md`](CHILTERN.md). Física vs OR: [`OR_PARITY.md`](OR_PARITY.md).

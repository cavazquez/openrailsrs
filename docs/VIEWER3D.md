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
- Alpha/sorting vs doble paso OR; pop-in vs `ViewingDistance` OR.
- Cabina: palancas CVF parciales; puertas/panto sim → visual (#81).
- Cast/receive sombras instanced ✅ (#72); VSM completo solo en render3d.

## Comando rápido

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export CHILTERN_ROUTE="$OPENRAILSRS_MSTS_CONTENT/Chiltern/ROUTES/Chiltern"
OPENRAILSRS_VIEW_RADIUS_M=300 cargo run --release -p openrailsrs-viewer3d -- \
  --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
```

Setup Wine/OR: [`CHILTERN.md`](CHILTERN.md). Física vs OR: [`OR_PARITY.md`](OR_PARITY.md).

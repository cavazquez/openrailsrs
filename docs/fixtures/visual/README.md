# Goldens visuales

| Fixture | Script |
|---------|--------|
| `smoke_orbit.png` | `./scripts/visual_regression_smoke.sh` (#43, CI) |
| `chiltern/` | `./scripts/visual_regression_chiltern.sh` (#71, local) |
| `or_reference/` | Capturas OR manuales (no CI) |

`UPDATE_GOLDEN=1` regenera. Diff: `openrailsrs-visual-diff`. Ver [`VIEWER3D_TESTING.md`](../../VIEWER3D_TESTING.md).

La referencia `smoke_orbit.png` se revisó el 2026-10-02 con Bevy 0.19.1 y
Vulkan lavapipe. Se actualizó por la visibilidad de cielo despejado independiente
del radio de carga y la fuente DejaVu con tildes y símbolos en toda la interfaz.
Ambas imágenes se inspeccionaron: la diferencia respecto de la referencia anterior
fue del 9,348 % de los píxeles (tolerancia 16).
Se conserva la tolerancia 16 y el máximo 2 %, con hashes y motivo en
`smoke_orbit.provenance.json`. Esta referencia sólo cubre el smoke interno;
las capturas originales de Open Rails no se han actualizado.

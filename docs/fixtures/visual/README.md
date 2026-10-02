# Goldens visuales

| Fixture | Script |
|---------|--------|
| `smoke_orbit.png` | `./scripts/visual_regression_smoke.sh` (#43, CI) |
| `chiltern/` | `./scripts/visual_regression_chiltern.sh` (#71, local) |
| `or_reference/` | Capturas OR manuales (no CI) |

`UPDATE_GOLDEN=1` regenera. Diff: `openrailsrs-visual-diff`. Ver [`VIEWER3D_TESTING.md`](../../VIEWER3D_TESTING.md).

La referencia `smoke_orbit.png` se revisó el 2026-10-02 con Bevy 0.19.1 y
Vulkan lavapipe. La captura antigua conservaba el espacio de una barra de
progreso oculta; el código de HEAD ya retiraba ese espacio con `Display::None`.
Por encima del pie del HUD, la diferencia era del 0,0063 % de los píxeles.
Se conserva la tolerancia 16 y el máximo 2 %, con hashes y motivo en
`smoke_orbit.provenance.json`. Esta referencia sólo cubre el smoke interno;
las capturas originales de Open Rails no se han actualizado.

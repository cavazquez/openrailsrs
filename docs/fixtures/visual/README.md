# Goldens visuales

| Fixture | Script |
|---------|--------|
| `smoke_orbit.png` | `./scripts/visual_regression_smoke.sh` (#43, CI) |
| `chiltern/` | `./scripts/visual_regression_chiltern.sh` (#71, local) |
| `or_reference/` | Capturas OR manuales (no CI) |

`UPDATE_GOLDEN=1` regenera. Diff: `openrailsrs-visual-diff`. Ver [`VIEWER3D_TESTING.md`](../../VIEWER3D_TESTING.md).

La referencia `smoke_orbit.png` se revisó el 2026-10-04 con Bevy 0.19.1 y
Vulkan lavapipe. La cámara anterior quedaba debajo del terreno; ahora el pitch
positivo y la distancia explícita de 180 m se conservan tras el encuadre
automático. También se verifican las dependencias de todos los shaders antes
de capturar. La escena sintética usa colores de diagnóstico: no representa
los materiales de Chiltern. Ambas imágenes se inspeccionaron; el cambio de
encuadre afecta el 85,541 % de los píxeles (tolerancia 16).
Se conserva la tolerancia 16 y el máximo 2 %, con hashes y motivo en
`smoke_orbit.provenance.json`. Esta referencia sólo cubre el smoke interno;
las capturas originales de Open Rails no se han actualizado.

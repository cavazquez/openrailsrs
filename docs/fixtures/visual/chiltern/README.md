# Goldens Chiltern (#71 / #170 cab slice)

| Archivo | Vista |
|---------|--------|
| `birmingham_exterior.png` | Orbit cerca −6080/14925 |
| `birmingham_cabina.png` | Driver frente (`LOOK_YAW/PITCH=0`) |
| `birmingham_cabina_up.png` | Driver mirando arriba (`LOOK_PITCH=0.55`) |
| `birmingham_cabina_left.png` | Driver izquierda (`LOOK_YAW=0.7`) |
| `birmingham_cabina_right.png` | Driver derecha (`LOOK_YAW=-0.7`) |

Resolución baseline: 640×360. Params fijados en `scripts/visual_regression_chiltern.sh`.

```bash
export OPENRAILSRS_MSTS_CONTENT=…
UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh
./scripts/visual_regression_chiltern.sh   # compara
```

OR manual opcional: `docs/fixtures/visual/or_reference/{desdeafuera,cabina}.png` (no CI; resoluciones distintas).

Pendiente #170: chase/orbit, cab2d, máscaras estructurales, diff OR automático.

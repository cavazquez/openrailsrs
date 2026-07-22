# Goldens Chiltern (#71)

| Archivo | Vista |
|---------|--------|
| `birmingham_exterior.png` | Orbit cerca −6080/14925 |
| `birmingham_cabina.png` | `OPENRAILSRS_FOLLOW=driver` |

```bash
export OPENRAILSRS_MSTS_CONTENT=…
UPDATE_GOLDEN=1 ./scripts/visual_regression_chiltern.sh
```

OR manual opcional: `docs/fixtures/visual/or_reference/{desdeafuera,cabina}.png`.

# SCE — Baseline Open Rails

Actividad: **MT_MT_0930 Edinburgh-Glasgow Queen Street** (Demo Model 1)
OR versión: 1.6.1, Wine, Linux

## Archivos

| Archivo | Descripción |
|---------|-------------|
| `or_evaluation_speed.csv` | CSV de evaluación de OR (TrainSpeed, Throttle, Brake, Distance) |

## Capturar el baseline

```bash
SCE_ACT="C:\\users\\TU_USUARIO\\Documents\\Open Rails\\Content\\Demo Model 1\\ROUTES\\SCE\\ACTIVITIES\\MT_MT_0930 Edinburgh-Glasgow Queen Street.act"

WINEPREFIX=~/wine64-OpenRails DISPLAY=:0 wine \
  "$WINEPREFIX/drive_c/Program Files/Open Rails/RunActivity.exe" \
  -start -activity "$SCE_ACT"
```

Dejar correr ~6 minutos de tiempo simulado (hasta que el tren esté en velocidad crucero).
Cerrar OR. Copiar el CSV generado:

```bash
WINEPREFIX=~/wine64-OpenRails
cp "$WINEPREFIX/drive_c/users/$USER/AppData/Roaming/Open Rails_MT_MT_0930 Edinburgh-Glasgow Queen StreetSpeed.csv" \
   examples/baselines/sce_glasgow/or_evaluation_speed.csv
```

## Comparación con openrailsrs

```bash
cd examples/sce

openrailsrs or-eval-driver ../baselines/sce_glasgow/or_evaluation_speed.csv \
  --out driver_or.csv

openrailsrs sim scenario.toml --driver driver_or.csv
```

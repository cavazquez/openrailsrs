# SCE — baseline Open Rails

Actividad `MT_MT_0930 Edinburgh-Glasgow Queen Street`. Archivo: `or_evaluation_speed.csv`. Paridad: [`docs/OR_PARITY.md`](../../../docs/OR_PARITY.md).

```bash
# ~6 min sim en OR, luego:
cp "$WINEPREFIX/drive_c/users/$USER/AppData/Roaming/Open Rails_MT_MT_0930 Edinburgh-Glasgow Queen StreetSpeed.csv" \
   examples/baselines/sce_glasgow/or_evaluation_speed.csv

cd examples/sce
openrailsrs or-eval-driver ../baselines/sce_glasgow/or_evaluation_speed.csv --out driver_or.csv
openrailsrs sim scenario.toml --driver driver_or.csv
openrailsrs compare-or ../baselines/sce_glasgow/or_evaluation_speed.csv run.csv
```

Detalle del escenario: [`../../sce/README.md`](../../sce/README.md).

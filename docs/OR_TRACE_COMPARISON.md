# Comparar trazas Open Rails ↔ openrailsrs

## Flujo

1. Capturar CSV en OR (actividad Chiltern / script Wine — [`CHILTERN.md`](CHILTERN.md)).
2. Correr sim: `openrailsrs sim examples/chiltern/scenario.toml`.
3. Comparar:

```bash
openrailsrs compare-or \
  --ours examples/chiltern/run.csv \
  --or path/to/or_dump.csv \
  --map docs/…   # o flags de columnas; ver --help
```

Umbrales: `max_velocity_rms`, etc. Exit 1 si falla.

## Columnas típicas

Mapear tiempo, velocidad, odometría/posición. OR dumps varían; usar TOML de mapa de columnas si hace falta (CLI `--help`).

Paridad física y baselines: [`OR_PARITY.md`](OR_PARITY.md).

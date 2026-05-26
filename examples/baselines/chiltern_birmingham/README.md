# Baseline Open Rails — Chiltern / Birmingham Pullman

Captura manual desde Open Rails 1.6.1 (Wine, Linux) el 2025-05-25.

| Campo | Valor |
|-------|--------|
| Ruta | Chiltern |
| Actividad | `RS_Let's go to Birmingham` |
| Consist | Birmingham Pullman |
| Inicio | Paddington Pfm 6 → Beyond Banbury |
| Sim time | 10:00:00 → 10:02:24 (~144 s wall, ~2.5 min sim) |
| Throttle peak | ~80 %, ~7 mph |

## Archivos

| Archivo | Descripción |
|---------|-------------|
| `openrails_dump.csv` | `OpenRailsDump.csv` del Data Logger OR (formato rendimiento/física, no `Time`/`Speed`/`Distance` estándar) |
| `openrails_log.txt` | `OpenRailsLog.txt` de la misma sesión |

## Notas

- Open Rails escribe en el Escritorio de Wine:  
  `/home/cristian/wine64-OpenRails/drive_c/users/cristian/Desktop/`
- `compare-or` MVP espera columnas `Time`, `Speed`, `Distance`; este dump requiere parser extendido o captura con **Evaluación → registro de velocidad** (sin rendimiento/física/vapor).
- En Wine, **Registro de datos de rendimiento** puede crashear con `pdh.dll.PdhFormatFromRawValue` (desactivar en Opciones → Registrador de datos).

## Uso

```bash
# Inspección rápida
head -1 examples/baselines/chiltern_birmingham/openrails_dump.csv
wc -l examples/baselines/chiltern_birmingham/openrails_dump.csv
```

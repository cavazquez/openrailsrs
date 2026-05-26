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
| `or_evaluation_speed.csv` | Registro **Evaluación → velocidad del tren** (`*Speed.csv` en `%APPDATA%`, compatible con `compare-or`) |
| `openrails_log.txt` | `OpenRailsLog.txt` de la misma sesión |

## Notas

- Open Rails escribe en el Escritorio de Wine:  
  `/home/cristian/wine64-OpenRails/drive_c/users/cristian/Desktop/`
- El registro de **evaluación** (velocidad del tren) va a `%APPDATA%` con nombre  
  `Open Rails_<Actividad>Speed.csv`, p. ej.  
  `/home/cristian/wine64-OpenRails/drive_c/users/cristian/AppData/Roaming/Open Rails_RS_Let's go to BirminghamSpeed.csv`
- `compare-or` detecta automáticamente el header `TIME,TRAINSPEED,…` de OR 1.6.x (pestaña Evaluación).
- El dump de rendimiento (`openrails_dump.csv`) requiere parser distinto; no usar con `compare-or` v1.
- En Wine, **Registro de datos de rendimiento** puede crashear con `pdh.dll.PdhFormatFromRawValue` (desactivar en Opciones → Registrador de datos).

## Uso

```bash
# Inspección rápida (dump rendimiento)
head -1 examples/baselines/chiltern_birmingham/openrails_dump.csv
wc -l examples/baselines/chiltern_birmingham/openrails_dump.csv

# Comparar evaluación OR vs corrida openrailsrs (ajusta umbrales según escenario alineado)
openrailsrs compare-or \
  examples/baselines/chiltern_birmingham/or_evaluation_speed.csv \
  examples/smoke/run.csv
```

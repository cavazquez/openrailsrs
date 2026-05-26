# Baseline Open Rails — Chiltern / Birmingham Pullman

Captura manual desde Open Rails 1.6.1 (Wine, Linux) el 2025-05-25.

| Campo | Valor |
|-------|--------|
| Ruta | Chiltern |
| Actividad | `RS_Let's go to Birmingham` |
| Consist | Birmingham Pullman |
| Inicio | Paddington Pfm 6 → Beyond Banbury |
| Sim time (evaluación) | 10:00:00 → 10:01:01 (~61 s sim, sesión 23:14) |
| Sim time (rendimiento) | 10:00:00 → 10:02:24 (~144 s sim, sesión ~22:53) |
| Throttle peak | ~80 %, ~2.4 mph (evaluación) |

## Archivos

### Sesión evaluación (23:14 — compatible con `compare-or`)

| Archivo | Origen OR |
|---------|-----------|
| `or_evaluation_speed.csv` | `%APPDATA%/Open Rails_<Actividad>Speed.csv` |
| `openrails_evaluation_log.txt` | `%APPDATA%/Open Rails/RS_… 2026-05-25 23.14.42.txt` |
| `openrails_evaluation.replay` | `%APPDATA%/Open Rails/RS_… 2026-05-25 23.14.42.replay` |
| `openrails_desktop_log.txt` | Escritorio Wine `OpenRailsLog.txt` (misma sesión) |
| `openrails_desktop_dump.csv` | Escritorio Wine `OpenRailsDump.csv` (casi vacío; sin evaluación activa en desktop) |

### Sesión rendimiento (~22:53 — referencia / no usar con `compare-or` v1)

| Archivo | Origen OR |
|---------|-----------|
| `openrails_dump.csv` | Escritorio Wine `OpenRailsDump.csv` (rendimiento/física, ~9961 filas) |
| `openrails_log.txt` | Escritorio Wine `OpenRailsLog.txt` (corrida larga) |

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

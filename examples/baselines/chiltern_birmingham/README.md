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
- El dump de rendimiento (`openrails_dump.csv`) usa parser posicional (`Speed (mph),Time (M),…`).
- En Wine, **Registro de datos de rendimiento** puede crashear con `pdh.dll.PdhFormatFromRawValue` (desactivar en Opciones → Registrador de datos).

## Umbrales calibrados (Chiltern eval, loose)

| Métrica | Umbral | Notas |
|---------|--------|--------|
| `max_velocity_rms` | 2.0 m/s | Física stub vs vapor OR |
| `max_position_max` | 150 m | Posición inicial no alineada aún |
| `max_throttle_rms` | 0.20 | Controles OR vs scripted driver |

Ver `[validate]` en `examples/chiltern/scenario.toml`.

## Uso

```bash
cd examples/chiltern
openrailsrs or-eval-driver ../baselines/chiltern_birmingham/or_evaluation_speed.csv --out driver_or.csv
openrailsrs sim scenario.toml --driver driver_or.csv

openrailsrs compare-or \
  ../baselines/chiltern_birmingham/or_evaluation_speed.csv \
  run.csv \
  --max-velocity-rms 50 \
  --max-position-max 500
```

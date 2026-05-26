# Baseline Open Rails — Chiltern / Birmingham Pullman

Captura manual desde Open Rails 1.6.1 (Wine, Linux) el 2025-05-25.

Instalación Wine/OR y flujo completo: [`docs/CHILTERN_OR_SETUP.md`](../../docs/CHILTERN_OR_SETUP.md).

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

## Umbrales calibrados (Chiltern eval)

Definidos en `examples/chiltern/scenario.overlay.toml` (fusionado tras `import-msts`):

| Métrica | Umbral | Notas |
|---------|--------|--------|
| `max_velocity_rms` | 4.5 m/s | Diesel ORTS por notch; RMS actual ~3.3 m/s |
| `max_position_max` | 55 m | Posición alineada PAT; max actual ~46 m |
| `max_throttle_rms` | 0.25 | Controles OR vs scripted driver |
| `max_brake_rms` | 50.0 | Escala freno `--brake-full-scale 27` |

Objetivo estricto futuro: 0.3 m/s / 25 m (RPM, scripts cab).

## Capturar baseline más largo

El CSV versionado tiene **~61 s** porque la sesión original se cortó ahí. Para validar más allá de t=61 (p. ej. 120–180 s):

```bash
# 1. Lanzar OR (actividad AUTO_SIGNAL, Evaluation logging ON)
./scripts/capture_chiltern_birmingham_or.sh

# 2. Tras cerrar OR, instalar en el repo (mín. 120 s por defecto)
./scripts/install_chiltern_birmingham_baseline.sh

# Duración mínima distinta (p. ej. 180 s):
# MIN_DURATION_S=180 ./scripts/install_chiltern_birmingham_baseline.sh
```

Origen OR: `%APPDATA%/Open Rails_RS_Let's go to BirminghamSpeed.csv`

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

# Baseline Chiltern — throttle 50 % (30 s)

Experimento **E** de calibración: aislar dinámica de motor (RPM) y curvas F(v) sin ambigüedad del driver.

## Captura en Open Rails (Wine)

### ⚠️ No uses `RunActivity.exe -help`

Ese flag **no existe**. OR intenta inicializar Direct3D/DXVK igual y Wine muestra el crash genérico *"Couldn't get first exception"* sin backtrace útil.

Usá el script del repo o el menú `OpenRails.exe`.

### Pasos

1. **Opciones OR** (antes de correr):
   - Options → **Evaluation** → train speed logging **ON**
   - Options → **Data Logger** → Performance/Physics **OFF** (evita `pdh.dll` en Wine)

2. **Lanzar Explorer** (mismo PAT/consist que Birmingham Pullman):

```bash
export WINEPREFIX="$HOME/wine64-OpenRails"
export DISPLAY=:0
./scripts/capture_chiltern_throttle50_or.sh
```

Equivalente manual:

```bash
wine "$WINEPREFIX/drive_c/Program Files/Open Rails/RunActivity.exe" \
  -start -explorer \
  "C:\users\cristian\Documents\Open Rails\Content\Chiltern\ROUTES\Chiltern\PATHS\RS_Let's go to Birmingham.pat" \
  "C:\users\cristian\Documents\Open Rails\Content\Chiltern\TRAINS\CONSISTS\Birmingham Pullman.con" \
  10:00 1 0
```

3. **En cabina** (modo Explorer) — teclas OR/MSTS por defecto (layout US; confirmá con **F1**):

   | Control | Tecla |
   |---------|-------|
   | Reverser adelante / atrás | `W` / `S` |
   | Subir / bajar throttle | `D` / `A` |
   | Aplicar freno de tren | `'` (apóstrofo) |
   | Soltar freno de tren | `;` (punto y coma) |
   | Soltar todo (parado) | `Shift` + `/` (Initialize brakes) |

   La coma `,` es **freno dinámico**, no el freno de tren.

   - `P` — pausa al cargar
   - `W` — reverser adelante si hace falta
   - `;` repetido — freno de tren **completamente suelto** (HUD: BRAKEPRESSURE `-001`)
   - `D` repetido — throttle **50 %** (~notch 4/8; HUD ~050)
   - Despausa, **30 s** simulados, salir

4. **Instalar baseline**:

```bash
./scripts/install_chiltern_throttle50_baseline.sh
```

OR escribe en:

```text
$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_explorerSpeed.csv
```

## Simulación openrailsrs

```bash
cd examples/chiltern
openrailsrs sim scenario_throttle50.toml --driver driver_throttle50.csv
openrailsrs compare-or ../baselines/chiltern_throttle50/or_evaluation_speed.csv run_throttle50.csv
cargo test -p openrailsrs-cli --test chiltern_throttle50
```

## Qué mirar

| Síntoma | Causa probable |
|---------|----------------|
| Sim más lenta al inicio | RPM sqrt / run-up DMBSH |
| Sim más rápida a 30 s | Curvas F(v) o cap P/v |
| Divergencia solo tras ~15 s | Calentamiento motor / adhesión |

## Nota sobre CSV existente

Si ya tenés `Open Rails_explorerSpeed.csv` con throttle ~12–22 %, **no sirve** para este baseline. Renombralo y recapturá:

```bash
mv "$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_explorerSpeed.csv" \
   "$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_explorerSpeed.csv.bak"
```

El script `install_chiltern_throttle50_baseline.sh` **rechaza** CSV con THROTTLEPERC promedio fuera de 45–55 %.

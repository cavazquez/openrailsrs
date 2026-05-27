# Baseline Chiltern — throttle 100 % (120 s)

Experimento **B** de calibración: aceleración a pleno throttle desde parado en vía plana (Blue Pullman, **8 vehículos** en OR).

> Baseline OR = multi-cuerpo; sim openrailsrs default = masa puntual. Revisión con `multi_body` **hecha** — arranque ~0.47 m/s RMS (≈ masa puntual). Ver [`docs/OR_PARITY_ROADMAP.md`](../../../docs/OR_PARITY_ROADMAP.md).

```bash
openrailsrs sim scenario_throttle100_multi_body.toml --driver driver_throttle100.csv
cargo test -p openrailsrs-cli --test chiltern_fullthrottle_multi_body
```

## Captura en Open Rails (Wine)

### Pasos

1. **Opciones OR** (antes de correr):
   - Options → **Evaluation** → train speed logging **ON**
   - Options → **Data Logger** → Performance/Physics **OFF**

2. **Lanzar Explorer**:

```bash
export WINEPREFIX="$HOME/wine64-OpenRails"
export DISPLAY=:0
./scripts/capture_chiltern_fullthrottle_or.sh
```

3. **En cabina** (modo Explorer):

   | Control | Tecla |
   |---------|-------|
   | Reverser adelante | `W` |
   | Subir throttle | `D` |
   | Soltar freno de tren | `;` |
   | Soltar todo (parado) | `Shift` + `/` |

   - `P` — pausa al cargar
   - `W` — reverser adelante
   - `;` — freno suelto (`BRAKEPRESSURE -001`)
   - `D` repetido — throttle **100 %** (`THROTTLEPERC ~100`)
   - Despausa y dejá correr **120 s de tiempo simulado** (mirá el **reloj OR** en pantalla, no el cronómetro del sistema; con pausa activa el sim no avanza)
   - El CSV registra ~1 fila/segundo: necesitás ver el reloj pasar de `10:00:00` a al menos `10:02:00` tras el primer 100 % throttle

4. **Instalar baseline**:

```bash
./scripts/install_chiltern_fullthrottle_baseline.sh
```

El instalador exige THROTTLEPERC medio ≥ 95 % en régimen y recorta una ventana de 120 s desde el primer instante con throttle pleno y freno suelto.

OR escribe en:

```text
$WINEPREFIX/drive_c/users/cristian/AppData/Roaming/Open Rails_explorerSpeed.csv
$WINEPREFIX/.../Open Rails_explorerSpeed_01.csv   # si el principal ya existía
```

El instalador toma el **`Open Rails_explorerSpeed*.csv` más reciente** (sin `.bak`).

## Simulación openrailsrs

```bash
cd examples/chiltern
openrailsrs sim scenario_throttle100.toml --driver driver_throttle100.csv
openrailsrs compare-or ../baselines/chiltern_fullthrottle/or_evaluation_speed.csv run_throttle100.csv
cargo test -p openrailsrs-cli --test chiltern_fullthrottle
```

## Qué mirar (OR-P3 / curvas F(v))

| Ventana | Objetivo |
|---------|----------|
| 0–30 s | Arranque: rampas de tracción, RPM, run-up DMBSH |
| 30–120 s | Transición P/v, velocidad de equilibrio parcial |

| Síntoma | Causa probable |
|---------|----------------|
| Sim más lenta 0–30 s | RPM / run-up / rampas OR-P3 |
| Sim más rápida 0–30 s | Curvas F(v) o cap P/v demasiado altos |
| Divergencia > 30 s | Davis agregado, resistencia, límites continuos DMBSH |

## Nota

Si `$SRC_CSV` es una captura del Experimento E (throttle ~50 %), **renombrala** antes de recapturar — el instalador rechaza THROTTLEPERC fuera de 95–100 %.

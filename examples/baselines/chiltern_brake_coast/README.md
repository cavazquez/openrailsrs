# Baseline Chiltern — frenada + costa libre (180 s)

Experimento **A** (OR-P6): aceleración a pleno throttle, **frenada fuerte breve** (5 s), soltar freno y **costa libre** — validar μ(v), skid limit y bleed del aire vs Open Rails.

> **Modelo:** OR simula 8 coches acoplados; openrailsrs (default) usa masa puntual. Tras OR-P4 conviene re-ejecutar este experimento con `multi_body = true` (prioridad alta — propagación de freno). Ver [`docs/OR_PARITY_ROADMAP.md`](../../../docs/OR_PARITY_ROADMAP.md).

> Distinto del “Experimento A” de `CALIBRATION.md` (coast-down sin frenos para calibrar Davis). Este es el criterio del roadmap OR-P6.

## Perfil del driver (`driver_brake_coast.csv`)

| Fase | Tiempo (s) | Throttle | Freno |
|------|------------|----------|-------|
| Aceleración | 0–100 | 1.0 | 0 |
| Frenada fuerte | 100–105 | 0 | 1.0 |
| Costa libre | 105–180 | 0 | 0 |

Criterio de aceptación: fase **105–180 s** con velocidad RMS ≤ **0.5 m/s** vs OR.

## Captura en Open Rails (Wine)

```bash
export WINEPREFIX="$HOME/wine64-OpenRails"
export DISPLAY=:0
./scripts/capture_chiltern_brake_coast_or.sh
```

Seguí la secuencia del script (100 s acelerando, 5 s freno pleno, ≥75 s costeando). Mirá el **reloj OR**, no el cronómetro del sistema.

El script de captura **respalda automáticamente** cualquier `Open Rails_explorerSpeed*.csv` previo en el Roaming de Wine (sufijo `.bak.YYYYMMDDHHMMSS`) antes de lanzar OR.

### Controles de freno (Pullman + Wine)

| Acción | Tecla OR (layout US) | Notas |
|--------|----------------------|-------|
| Aplicar freno de tren | `'` | En Wine/teclado latam suele no responder |
| Soltar freno de tren | `;` | Idem |
| Freno emergencia | `Backspace` | No confundir con la última posición del mouse |
| Freno dinámico | `,` | **No** es freno de tren |

El `.eng` del Pullman tiene **5 detentes**: release → lap → suppression → **full service** → emergency. La palanca por mouse tiene muchas frames visuales, pero la **última posición (emergency) a menudo no frena más** en marcha; la útil es la **anteúltima (full service)**. En capturas recientes el freno efectivo fue con mouse (`BRAKEPRESSURE` ~45), no con teclado.

Antes de capturar: **F1 → Key Commands** o **Options → Keyboard**; remapear Increase/Decrease a **PageDown/PageUp** si `'`/`;` no mueven el HUD.

```bash
./scripts/install_chiltern_brake_coast_baseline.sh
```

El instalador exige:

- t=0 con throttle ≥ 95 % y freno suelto
- BRAKEPRESSURE alto entre 100–105 s
- Costa libre con throttle bajo y freno suelto tras 105 s
- Ventana de 180 s

## Simulación openrailsrs

```bash
cd examples/chiltern
openrailsrs sim scenario_brake_coast.toml --driver driver_brake_coast.csv
openrailsrs compare-or ../baselines/chiltern_brake_coast/or_evaluation_speed.csv run_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast
```

### Multi-cuerpo (`multi_body = true`)

Misma ventana y driver; compara propagación de freno/costa por vehículo vs OR multi-cuerpo nativo:

```bash
openrailsrs sim scenario_brake_coast_multi_body.toml --driver driver_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast_multi_body
```

| Fase | Masa puntual vs OR | Multi-cuerpo vs OR |
|------|-------------------|-------------------|
| 115–180 s (costa) | ~0.07 m/s RMS | ~**0.10** m/s RMS (`coupler_kind = "pullman"`, `multi_body_scalar_coast_below_v_mps = 16`) |

Ambos pasan umbral OR-P6 (≤0.50 m/s); multi-cuerpo costa mejorada vs ~0.16 m/s con acopladores puros (A2).

Sin baseline OR versionado, el test `validate_against_or_baseline` se omite.

## Qué mirar

| Ventana | Objetivo |
|---------|----------|
| 0–100 s | Aceleración (misma base que Exp B) |
| 100–105 s | Respuesta de freno / presión cilindro |
| 105–180 s | Decaimiento en costa libre (Davis + residual air) |

| Síntoma | Causa probable |
|---------|----------------|
| Sim frena más que OR en 100–105 s | μ(v), skid, escala PSI cilindro |
| Sim más lenta en costa 105–180 s | Davis, resistencia rodadura, bleed aire |
| Sim más rápida en costa | Freno residual, μ(v) bajo |

`run.csv` incluye **`brake_f_head_n`**, **`brake_f_train_air_n`** (primer vagón train-air) y **`brake_f_tail_n`** cuando hay cilindros registrados.

Análisis A1 (propagación cabeza → vagón sin EP):

```bash
openrailsrs sim scenario_brake_coast.toml --driver driver_brake_coast.csv
python3 ../../scripts/analyze_brake_propagation.py run_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast chiltern_brake_coast_a1_script_passes
```

En Pullman ambos motores (DMBSA/DMBSH) son **EP**; el retardo de tubo se ve en **`brake_f_train_air_n`**, no head vs tail.

En `scenario_brake_coast_multi_body.toml`: `coupler_kind = "pullman"` (acoplador estable con tope de fuerza y sub-pasos adaptativos en tracción/freno) y `multi_body_scalar_coast_below_v_mps = 16.0` para la costa (115–180 s ~**0.10** m/s RMS vs OR; antes ~0.13 con freight + v<12).

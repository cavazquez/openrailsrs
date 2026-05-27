# Calibración y estado del simulador

Última actualización: 2026-05-26 (baseline Chiltern 136 s, AUTO_SIGNAL)

**Roadmap de paridad con Open Rails:** [`docs/OR_PARITY_ROADMAP.md`](docs/OR_PARITY_ROADMAP.md)

---

## Resumen de avances

| Etapa | Estado | Detalle |
|-------|--------|---------|
| Parseo básico `.eng`/`.wag`/`.con` | ✅ | Masa, fuerza, potencia, velocidad, freno |
| Curvas de tracción `ORTSMaxTractiveForceCurves` | ✅ (bug corregido) | Ver bugs abajo |
| Modelo diesel (DieselPowerTab + ThrottleRPMTab + RPM lag) | ✅ | `DieselEngineParams` con time constant 2 s |
| Parseo `ORTSDieselEngines { Diesel { ... } }` | ✅ | Primer bloque Diesel; fallback a campos raíz |
| Tracción multi-motor (`diesel_engines` + `diesel_rpm[]`) | ✅ | Suma F y P de todos los `Engine` del consist |
| Modelo legacy P/v (sin ORTSMaxTractiveForceCurves) | ✅ | `DieselTractionModel::from_power_and_effort` + `RunUpTimeToMaxForce` |
| Davis por vehículo (`ORTSDavis_A/B/C` sumados) | ✅ | `Consist::aggregate_davis()` al cargar `.con` |
| Driver desde CSV de Open Rails (`or-eval-driver`) | ✅ | Activity mode y Explorer mode |
| Comparación automática (`compare-or`) | ✅ | RMS y max; fases vía `phase_bounds` en `[validate]` |
| Modelo longitudinal default | 🔶 Masa puntual | Baselines OR = multi-cuerpo; ver [`docs/OR_PARITY_ROADMAP.md`](docs/OR_PARITY_ROADMAP.md) |
| Multi-cuerpo opt-in (`multi_body`) | 🔶 | Chiltern ~0.39 m/s RMS global; sub-pasos acoplador; arranque ~0.54 m/s |
| Ejemplo SCE (Class 47) — velocidad RMS | ✅ 0.35 m/s | Con baseline de 100 s (masa puntual) |
| Ejemplo Chiltern (Blue Pullman) — 136 s OR | ✅ ~0.39 m/s RMS | Masa puntual vs OR multi-cuerpo; `assume_signals_clear` |
| Ejemplo Chiltern — posición max | ✅ ~23 m | 0–136 s vs OR eval AUTO_SIGNAL |

---

## Bugs corregidos (sesión 2026-05-26)

### Bug 1 (crítico): eje de velocidad en `ORTSMaxTractiveForceCurves` convertido incorrectamente

**Archivo:** `crates/openrailsrs-formats/src/typed/engine.rs`, función `parse_orts_curve_points`

**Síntoma:** El tren Blue Pullman alcanzaba solo ~1.9 mph a 12 % de throttle, cuando OR llega a 29.4 mph.

**Causa:** La función `parse_orts_curve_points` aplicaba `kmh_to_mps(v)` a los valores de velocidad del eje X de las curvas, dividiendo por 3.6. Pero los valores en los archivos `.inc` de MSTS/OR ya están en **m/s** (el default de OR para Speed cuando no hay sufijo de unidad). Esto comprimía todo el eje de velocidad 3.6×, haciendo que las curvas cayeran a cero de fuerza a ~12.7 m/s (28 mph) en vez de ~46 m/s (103 mph).

**Verificación en código fuente OR:**
- `MSTSLocomotive.cs` línea 1056: `case "engine(ortsmaxtractiveforcecurves": TractiveForceCurves = new InterpolatorDiesel2D(stf, false);`
- La clase `InterpolatorDiesel2D` usa STFReader con su unidad default para Speed = **m/s**.
- El manual OR (appendices.rst) confirma: _"Speed: m/s (default), km/h, mph, kph"_ — el default es m/s.

**Fix:** Eliminar la llamada a `kmh_to_mps()` en esa función; usar `v` directamente.

---

### Bug 2 (crítico): fuerzas en curvas multiplicadas por 4.44822 cuando ya están en Newtons

**Archivo:** `crates/openrailsrs-formats/src/typed/engine.rs`, función `orts_curve_force_n`

**Causa:** La heurística `if value < 100_000 { value * 4.44822 } else { value }` intentaba convertir de lbf a N, pero los archivos `.inc` reales (`BR_class47_2580hp.inc`, `RF_WP_DMBSA.eng`) usan valores sin sufijo de unidad. Según el STFReader de OR, **bare numbers para Force = Newtons** (el default SI). La heurística multiplicaba por 4.44× todos los valores menores a 100 000 N (que es la mayoría en el Blue Pullman).

Ejemplo concreto (Blue Pullman, notch 100%, stall):
- Valor real en archivo: 86 073 N
- Con bug: 86 073 × 4.44822 = 382 769 N (4.44× inflado)

**Fix:** La función `orts_curve_force_n` ahora devuelve el valor sin transformación.

**Nota:** El test `parse_orts_notch_curves_converts_lbf_to_newtons` fue renombrado y corregido; ahora verifica que 86 073 se almacene como 86 073 N.

---

## Issues conocidos (pendientes)

### Issue 1: ~~Blue Pullman tiene dos motores — solo se usa uno~~ (resuelto)

**Fix aplicado:** `Consist::diesel_traction_models()` recolecta un `DieselTractionModel` por cada `Engine` del consist. La física suma fuerzas y potencias (`TrainPhysics::diesel_engines`, `TrainSimState::diesel_rpm: Vec<f64>`).

- **DMBSA:** curvas ORTS completas (Darwin Smith) + `DieselEngineParams` si hay tablas.
- **DMBSH:** stub legacy (`MaxPower 1000 kW`, `MaxForce 150.65 kN`) → modelo sintético P/v.

Resultado Chiltern (65 s, `driver_or.csv`): posición max **~39 m** (antes ~122 m), odómetro **~300 m** (antes ~113 m).

---

### Issue 2: ~~Davis resistance calibrada a mano~~ (resuelto en assets)

`ORTSDavis_A/B/C` se parsean por `.eng`/`.wag` y se suman en `Consist::aggregate_davis()` (~3455 N en A para Birmingham Pullman: DMBSA + 6 vagones). El override manual en `scenario.tmp.toml` ya no se usa en el escenario principal.

**Pendiente opcional:** DMBSH (cola) no trae `ORTSDavis_*` en el content OR — usa solo masa + modelo legacy P/v.

---

### Issue 3: ~~`ThrottleRPMTab` en bloque `ORTSDieselEngines`~~ (resuelto)

Se parsea el primer bloque `Diesel(` dentro de `ORTSDieselEngines` y se usa como fallback cuando los campos raíz están vacíos. El stub DMBSA con tablas a nivel raíz sigue funcionando (SCE, Chiltern).

---

### Issue 4: Comparación de posición usa odómetro integrado, no posición de ruta

El `compare-or` compara la distancia acumulada en ambas trazas. Cualquier discrepancia de velocidad temprana se acumula como error de posición. Esto hace que el umbral de posición sea muy sensible a errores de velocidad en los primeros segundos (p.ej. el período de frenos al inicio).

---

## Experimentos sugeridos para correr en Open Rails

### ¿Para qué sirven?

Para tener datos de calibración independientes con los que verificar los modelos de física.

---

### Experimento A: Costa libre (free roll) — calibrar Davis

**Objetivo:** medir la resistencia real al rodamiento del tren.

**Procedimiento:**
1. Abrir OR en **modo explorer** con el Blue Pullman en Chiltern.
2. En el HUD, nota la velocidad inicial.
3. Activar la grabación (Options → Evaluation → Speed).
4. Llevar el tren a una velocidad estable (ej. 40 mph = 64 km/h) en vía plana.
5. Soltar el throttle a 0 (tecla `A` hasta llegar a 0 notches).
6. **No aplicar frenos.** Dejar que el tren decelere solo.
7. Grabar la velocidad cada segundo durante ~60 segundos.
8. Copiar el `*Speed.csv` resultante a `examples/baselines/`.

**Qué medir:** `a = F_resist / m_total` en la zona donde la deceleración es constante.
Con `F = m × a`, si el tren pesa ~440 t y decelera 0.01 m/s², la resistencia total es 4400 N.

> **Nota:** En el roadmap OR-P6, “Experimento A” también designa **frenada fuerte + costa libre** (validar μ(v) y bleed del freno). Ese perfil está en `examples/chiltern/scenario_brake_coast.toml` y `examples/baselines/chiltern_brake_coast/` — distinto del free-roll Davis de arriba.

---

### Experimento A (OR-P6): Frenada + costa libre — validar frenos

**Objetivo:** comparar decaimiento post-freno vs OR (P6b μ(v), P6c skid, bleed cilindro).

**Driver:** 100 s throttle 100 % → 5 s freno pleno → 75 s costa libre (`driver_brake_coast.csv`).

```bash
./scripts/capture_chiltern_brake_coast_or.sh
./scripts/install_chiltern_brake_coast_baseline.sh
cargo test -p openrailsrs-cli --test chiltern_brake_coast
```

Criterio: fase **105–180 s**, velocidad RMS ≤ **0.5 m/s** vs OR.

---

### Experimento B: Aceleración a pleno throttle — verificar curvas de tracción

**Objetivo:** verificar que las curvas `ORTSMaxTractiveForceCurves` del Blue Pullman son correctas.

**Procedimiento:**
1. Partir desde parado en vía plana.
2. Aplicar throttle al **100%** (notch 8 / HUD ~100).
3. Grabar con `*Speed.csv` durante 120 segundos.
4. Instalar baseline:

```bash
./scripts/capture_chiltern_fullthrottle_or.sh
# … en cabina OR: freno suelto, D hasta 100 %, 120 s simulados …
./scripts/install_chiltern_fullthrottle_baseline.sh
```

Destino versionado: `examples/baselines/chiltern_fullthrottle/or_evaluation_speed.csv`

Validación openrailsrs:

```bash
cargo test -p openrailsrs-cli --test chiltern_fullthrottle
```

**Qué medir:** curva de aceleración v(t). Con F_motor(v) conocida y Davis calibrado, comparar `dv/dt × m_total` con `F_motor(v) - F_davis(v)`.

---

### Experimento C: Throttle fijo a distintos porcentajes — equilibrio en velocidad

**Objetivo:** para cada notch, encontrar la velocidad de equilibrio donde F_motor = F_resist.

**Procedimiento:** Repetir con throttle = 20 %, 40 %, 60 %, 80 % en vía plana.
Esperar a que el tren alcance velocidad constante y notar:
- Velocidad de equilibrio
- RPM de motor (visible en OR HUD con tecla `F5`)

**Qué se aprende:** la intersección entre la curva de fuerza y la resistencia a cada notch.

---

### Experimento D: Verificar segundo motor

**Objetivo:** confirmar que OR usa ambos motores del Blue Pullman.

**Procedimiento:**
1. En el consist, desconectar temporalmente el segundo motor editando el `.con` para tener solo un Engine entry.
2. Repetir el Experimento B.
3. Comparar la aceleración con la del experimento original de dos motores.

**Qué se aprende:** si la aceleración se reduce a la mitad, confirma que OR suma ambos motores.

---

### Experimento E: Baseline corto con throttle conocido (para depuración rápida)

**Objetivo:** un baseline de 30 segundos con condiciones perfectamente controladas.

**Procedimiento:**
1. Partir desde parado, vía plana.
2. A t=0, soltar frenos y poner throttle al **50%**.
3. Mantener el 50% exacto durante 30 segundos.
4. No tocar nada más.
5. Copiar `*Speed.csv`.

**Lo que evita:** ambigüedad en el driver input — sabemos exactamente que el throttle es 0.5 todo el tiempo.

---

## Consultas al código fuente de Open Rails

### ¿Dónde está la física principal?

| Componente | Archivo en OR |
|------------|---------------|
| Locomotora diesel | `Source/Orts.Simulation/Simulation/RollingStocks/MSTSDieselLocomotive.cs` |
| Locomotora base | `Source/Orts.Simulation/Simulation/RollingStocks/MSTSLocomotive.cs` |
| Vagón (resistencia Davis) | `Source/Orts.Simulation/Simulation/RollingStocks/TrainCar.cs` |
| Motor diesel (RPM, power tab) | `Source/Orts.Simulation/Simulation/RollingStocks/SubSystems/PowerSupplies/DieselEngine.cs` |
| Parser STF | `Source/Orts.Parsers.Msts/STFReader.cs` |

### Hallazgos clave del código fuente OR

1. **`ORTSMaxTractiveForceCurves`** usa `InterpolatorDiesel2D(stf, false)`. Este objeto lee pares `(speed, force)` con **STFReader.UNITS default** → **m/s y N**. Los valores sin sufijo NO se convierten.

2. **`TractiveForceCurves.Get(t, v)`** devuelve directamente la fuerza en N. Luego OR aplica un factor `(1 - PowerReduction)` y opcionalmente limita por adhesión.

3. **RPM spin-up** en `DieselEngine.cs` usa: `dRPM = clamp(sqrt(2 * RateOfChangeUpRPMpSS * throttleAccFactor * (DemandedRPM - RealRPM)), 0.01 * ChangeUpRPMpS, ChangeUpRPMpS)`. Nuestra implementación usa la aproximación de primer orden (`exp(-dt/τ)`) que es similar pero no idéntica.

4. **Unidades en `.inc` files**: valores sin sufijo = unidad SI (N para fuerza, m/s para velocidad, W para potencia, kg para masa). Valores con sufijo como `62000lbf` o `90mph` son convertidos por el STFReader.

---

## Roadmap de calibración (Chiltern / Blue Pullman)

Última revisión: 2026-05-26.

### Hecho (T1–T10)

| # | Tarea | Estado |
|---|--------|--------|
| T1 | Escala freno OR (0–100) vs sim (0–1) en `compare-or` | ✅ |
| T2 | Stub DMBSA completo (`ORTSDieselEngines`, tablas OR) | ✅ |
| T3 | Experimento A (free roll) + ajuste Davis manual | ✅ código |
| T4 | Parseo `ORTSDavis_A/B/C` por vehículo y suma | ✅ |
| T5 | Calibración DMBSH legacy vs OR (Exp. B/D) | ✅ código |
| T6 | RPM sqrt OR (`RateOfChangeUpRPMpSS`, etc.) | ✅ |
| T7 | Parseo flat `ORTSDieselEngines` + masas `64t-uk` | ✅ |
| T8 | Adhesión Curtius-Kniffler + calentamiento motor | ✅ |
| T9 | Frenos EP en locomotoras (sin retardo de tubería) | ✅ código |
| T10 | Experimento E — throttle 50 % / 30 s + baseline OR | ✅ |

### Métricas actuales vs objetivo

| Corrida | Métrica | Actual | Objetivo estricto | Estado |
|---------|---------|--------|-------------------|--------|
| Birmingham 61 s | posición max | ~16 m | < 55 m | ✅ |
| Birmingham 61 s | vel RMS global | **~0.36 m/s** | ~0.3 m/s | ✅ (umbral 0.5) |
| Birmingham 40–61 s | vel RMS | **~0.42 m/s** | bajar | ✅ (umbral 0.5) |
| Experimento E 30 s | vel RMS | ~2.4 m/s | ≤ 3.0 | ✅ |

### Experimentos OR (baselines)

| Exp | Objetivo | Sim / código | Baseline OR |
|-----|----------|--------------|-------------|
| A | Costa libre → Davis | parcial (T3/T4) | ❌ falta capturar |
| B | Full throttle → F(v) | `scenario_throttle100.toml` + test | ❌ capturar con scripts |
| C | Equilibrio 20/40/60/80 % | ❌ | ❌ |
| D | Un motor vs dos | parcial (T5) | ❌ falta capturar |
| E | Throttle 50 % / 30 s | ✅ | ✅ |

---

### Pendiente (orden sugerido)

| Prioridad | Qué falta | Notas |
|-----------|-----------|--------|
| **1** | Cerrar gap de velocidad en corrida Birmingham (65 s), sobre todo **40–65 s** | ✅ ~0.42 m/s global; stub DMBSA con `ORTSDieselEngines` + Davis; DMBSH `MaxContinuousForce` 130 kN |
| **2** | Capturar baselines OR de experimentos **A, B, D** (y **C** si hace falta) | Ver procedimientos más abajo (Exp. A–E) |
| **3** | Calibrar **DMBSH** contra OR | Hoy es modelo P/v sintético (stub legacy) |
| **4** | Validar **frenos EP** vs OR en corrida real | T9 implementado; falta contraste en Birmingham completo |
| **5** | **Umbrales estrictos** Chiltern (`0.3 m/s` / `25 m`) en `scenario.toml` | Solo cuando P1–P4 estén cerrados |
| **6** | **Pendiente de vía** (`grade_m_per_m`) en física | Mediano plazo; `track.toml` ya tiene `grade_percent` |
| **7** | Señales / límites / driver automático respetando señales | Largo plazo; hoy el driver viene del CSV de OR |

### Mediano / largo plazo (referencia)

- Integrar distancia en trazas OR **Explorer** (integrar `TRAINSPEED` cuando `DISTANCETRAVELLED` = 0).
- Energía vs OR en `compare-or`.
- Comparación topológica (`edge_id` además de odómetro).
- Pendiente variable a lo largo del recorrido (más allá de `grade_percent` por arista).

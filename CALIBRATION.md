# Calibración y estado del simulador

Última actualización: 2026-05-26 (multi-motor diesel)

---

## Resumen de avances

| Etapa | Estado | Detalle |
|-------|--------|---------|
| Parseo básico `.eng`/`.wag`/`.con` | ✅ | Masa, fuerza, potencia, velocidad, freno |
| Curvas de tracción `ORTSMaxTractiveForceCurves` | ✅ (bug corregido) | Ver bugs abajo |
| Modelo diesel (DieselPowerTab + ThrottleRPMTab + RPM lag) | ✅ | `DieselEngineParams` con time constant 2 s |
| Parseo `ORTSDieselEngines { Diesel { ... } }` | ✅ | Primer bloque Diesel; fallback a campos raíz |
| Tracción multi-motor (`diesel_engines` + `diesel_rpm[]`) | ✅ | Suma F y P de todos los `Engine` del consist |
| Modelo legacy P/v (sin ORTSMaxTractiveForceCurves) | ✅ | `DieselTractionModel::from_power_and_effort` |
| Driver desde CSV de Open Rails (`or-eval-driver`) | ✅ | Activity mode y Explorer mode |
| Comparación automática (`compare-or`) | ✅ | RMS y max para velocidad, posición, throttle, freno |
| Ejemplo SCE (Class 47) — velocidad RMS | ✅ 0.35 m/s | Con baseline de 100 s |
| Ejemplo Chiltern (Blue Pullman) — posición max | ✅ ~39 m | Dual motor + `driver_or.csv` (era ~122 m) |
| Ejemplo Chiltern — velocidad RMS | ⚠️ ~5.3 m/s | Mejor posición; calibrar Davis / perfiles motor |

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

### Issue 2: Davis resistance calibrada a mano, sin validación rigurosa

Los parámetros Davis en `scenario.tmp.toml` (`a_n`, `b_n_per_mps`, `c_n_per_mps2`) fueron estimados, no derivados de los datos reales del vehículo. Los archivos `.eng` y `.wag` de OR tienen `ORTSDavis_A`, `ORTSDavis_B`, `ORTSDavis_C` **por vagón**, que OR suma. Nuestro sim usa un Davis único para todo el tren.

**Fix necesario:** parsear `ORTSDavis_A/B/C` de cada vehículo en el consist y sumar la resistencia total.

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

---

### Experimento B: Aceleración a pleno throttle — verificar curvas de tracción

**Objetivo:** verificar que las curvas `ORTSMaxTractiveForceCurves` del Blue Pullman son correctas.

**Procedimiento:**
1. Partir desde parado en vía plana.
2. Aplicar throttle al **100%** (notch 10).
3. Grabar con `*Speed.csv` durante 120 segundos.
4. Copiar el CSV a `examples/baselines/chiltern_fullthrottle/`.

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

## Próximos pasos (roadmap)

### Corto plazo (bugs + calibración)

- [ ] Parsear `ORTSDieselEngines { Diesel { ... } }` para extraer DieselPowerTab/ThrottleRPMTab del Blue Pullman
- [ ] Soportar múltiples motores en consist (sumar fuerzas de todos los `Engine` entries)
- [ ] Parsear `ORTSDavis_A/B/C` por vehículo y sumar al Davis del tren
- [ ] Correr Experimentos A-E para obtener datos de calibración independientes

### Mediano plazo (precisión)

- [ ] Implementar pendiente de vía en la física (`grade_m_per_m` del perfil de elevación)
- [ ] Calibrar `rpm_time_constant_s` contra el RPM spin-up real de OR (`RateOfChangeUpRPMpSS`)
- [ ] Verificar el modelo de frenos contra el sistema EP del Blue Pullman (EP brakes, no aire normal)

### Largo plazo (completitud)

- [ ] Parsear señales y velocidades límite del itinerario
- [ ] Modo de conductor automático que respeta las señales (actualmente usa el driver de OR)
- [ ] Soporte para vía con pendiente variable a lo largo del recorrido

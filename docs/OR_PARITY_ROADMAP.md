# Roadmap de paridad con Open Rails

Plan de implementación para cerrar las diferencias entre **openrailsrs** y el simulador físico de [Open Rails](https://github.com/openrails/openrails), identificadas en el análisis de código fuente (2026-05-26).

**Estado de calibración actual (baseline):**

| Escenario | Duración | RMS velocidad | Notas |
|-----------|----------|---------------|-------|
| Chiltern Birmingham | 136 s | ~0.39 m/s | Masa puntual vs OR multi-cuerpo; `assume_signals_clear` |
| Chiltern multi_body | 136 s | ~0.39 m/s | `multi_body` + sub-pasos acoplador, `time_step = 1.0` |
| Chiltern full-throttle (Exp B) | 120 s | ~0.47 m/s (0–30 s) | Masa puntual; OR-P13 + run-up |
| SCE Glasgow | 100 s | ≤1.0 m/s (umbral) | Masa puntual; crucero 27 % throttle |

Este documento **no reemplaza** [`ROADMAP.md`](../ROADMAP.md) ni [`CALIBRATION.md`](../CALIBRATION.md); los complementa con trabajo específico de paridad física.

---

## Modelo físico: OR vs openrailsrs (importante para baselines)

Los CSV en `examples/baselines/` se capturan en **Open Rails**, que siempre simula el **consist completo** como coches acoplados (`TrainCar` + acopladores + frenos por vehículo).

En **openrailsrs**, hasta activar `[simulation] multi_body = true`, la dinámica longitudinal usa **masa puntual**: una sola velocidad, Davis agregado, tracción sumada — aunque el `.con` tenga 8 coches. Los cilindros de freno sí pueden ser por vehículo (`BrakeSystem`), pero el tren no “se estira” ni transmite esfuerzo por holgura.

| | Open Rails | openrailsrs (default) | openrailsrs (`multi_body = true`) |
|---|------------|----------------------|-----------------------------------|
| Consist Chiltern | DMBSA + 6 Pullman + DMBSH (**8**) | Mismo `.con`, **1 velocidad** | 8 masas + acopladores |
| Consist SCE | Class 47 + 6 MK2 (**7**) | Mismo `.con`, **1 velocidad** | 7 masas + acopladores |
| Davis | Por `TrainCar` | Suma en `train.davis` | Por vehículo (`vehicle_davis`) |
| Baselines OR | Multi-cuerpo nativo | Comparación **mixta** ⚠️ | Más comparable con OR |

**Implicación:** los RMS publicados (Chiltern ~0.39 m/s, costa ~0.07 m/s, etc.) calibran **masa puntual vs OR multi-cuerpo**. En crucero la diferencia suele ser pequeña; en **arranque, frenada fuerte y propagación de aire** puede ocultar error físico compensado por otros ajustes.

**Estado multi-cuerpo (2026-05):** cableado + Davis por vehículo; sub-pasos acoplador (`MULTI_BODY_MAX_SUBSTEP_S = 0.05`) → Chiltern `time_step = 1.0` estable; RMS ~0.52 m/s vs OR. Escenario: `examples/chiltern/scenario_multi_body.toml`.

---

## Leyenda

| Símbolo | Significado |
|---------|-------------|
| ✅ | Hecho |
| 🔶 | Parcial / calibrado con atajos |
| 🔲 | Pendiente |
| P0 | Crítico para validación OR existente |
| P1 | Alto impacto en realismo general |
| P2 | Contenido específico o largo plazo |

**Referencias OR (lectura obligatoria por fase):**

- `Source/Orts.Simulation/Simulation/RollingStocks/MSTSDieselLocomotive.cs`
- `Source/Orts.Simulation/Simulation/RollingStocks/Subsystems/PowerSupply/DieselEngine.cs`
- `Source/Orts.Simulation/Simulation/RollingStocks/MSTSLocomotive.cs`
- `Source/Orts.Simulation/Simulation/RollingStocks/TrainCar.cs`
- [Manual OR — Physics](https://open-rails.readthedocs.io/en/latest/physics.html)

---

## Visión general

```mermaid
flowchart LR
    subgraph wave1["Ola 1 — Diesel eléctrico-hidráulico"]
        A1[OR-P1 Throttle aparente + P/v]
        A2[OR-P2 Auto-friction CN]
        A3[OR-P3 Rampas tracción]
    end
    subgraph wave2["Ola 2 — Tren como sistema"]
        B1[OR-P4 Multi-cuerpo]
        B2[OR-P5 Davis por vehículo]
        B3[OR-P6 Frenos MSTS]
    end
    subgraph wave3["Ola 3 — Mundo y señales"]
        C1[OR-P7 Señales + driver]
        C2[OR-P8 Resistencia ambiental]
    end
    subgraph wave4["Ola 4 — Especializados"]
        D1[OR-P9 Gearbox mecánico]
        D2[OR-P10 Freno dinámico]
        D3[OR-P11 Vapor avanzado]
    end
    wave1 --> wave2 --> wave3 --> wave4
```

| Ola | Fases | Objetivo | Regresión esperada |
|-----|-------|----------|-------------------|
| 1 | OR-P1 … OR-P3 | Paridad diesel Class 47 / Blue Pullman | Chiltern + SCE deben seguir pasando |
| 2 | OR-P4 … OR-P6 | Longitud del tren y frenado real | Nuevos baselines OR (frenada completa) |
| 3 | OR-P7 … OR-P8 | Actividades MSTS sin `assume_signals_clear` | Chiltern sin override de señales |
| 4 | OR-P9+ | Contenido mecánico / vapor / DB | Por escenario |

---

## OR-P1 — Throttle aparente y tope P/v diesel (P0)

**Objetivo:** Replicar la cadena OR en locos diesel-eléctricos con `ORTSMaxTractiveForceCurves` (Class 47, DMBSA).

**Gap actual:**

- OR limita el throttle efectivo con `ReverseThrottleRPMTab[RealRPM]` → `ApparentThrottleSetting` antes de consultar curvas (`MSTSLocomotive.UpdateTractionForce`).
- OR limita fuerza con `LocomotiveMaxRailOutputPowerW × throttle × DieselEngineFractionPower`, no con `DieselPowerTab(RPM)` escalado heurísticamente.
- Nuestro `effective_power_w()` usa escalado idle→tab (< 50 % lineal, ≥ 50 % pleno) — calibración SCE, no paridad literal.

**Implementación:**

| Paso | Crate / archivo | Trabajo |
|------|-----------------|---------|
| 1 | `openrailsrs-formats` | Parsear `ReverseThrottleRPMTab`, `LocomotiveMaxRailOutputPowerW`, `ORTSTractiveForceIsPowerLimited`, `UnloadingSpeedMpS` |
| 2 | `openrailsrs-train/diesel.rs` | `apparent_throttle(rpm) -> f64`; cap P/v con `rail_power_w × t × run_fraction` |
| 3 | `openrailsrs-sim/physics.rs` | Usar `min(t_driver, t_apparent)` en curvas; separar HUD-power de rail-power cap |
| 4 | Escenarios | Flag `[simulation] legacy_power_cap = true` para transición; retirar hack 50 % cuando P1 pase tests |

**Criterios de aceptación:**

- [x] Test unitario: RPM bajo → fuerza limitada aunque throttle driver = 1.0 (`or_p1_diesel_eng`, `golden_assets` / Class 47 `.eng`)
- [ ] `audit_sce_cruise` y `audit_chiltern_forces` pasan sin overrides de potencia
- [ ] SCE 100 s: RMS ≤ 1.0 m/s; crucero 27 % dentro de ±0.5 mph vs OR
- [ ] Chiltern 136 s: RMS ≤ 0.35 m/s (no empeorar >10 % vs baseline actual)

**Referencias OR:** `DieselEngine.cs` L1366–1416, `MSTSLocomotive.cs` L2555–2635, `MSTSDieselLocomotive.cs` L635–698.

**Estimación:** 3–5 días.

---

## OR-P2 — Auto-friction OR completa (P0)

**Objetivo:** Sustituir `davis_est.rs` simplificado por las fórmulas Davis 1926 / CN 1992 de OR cuando falten `ORTSDavis_*`.

**Gap actual:**

- OR calcula A/B desde `ORTSBearingType`, masa, número de ejes; C desde área frontal y `ORTSDavisDragConstant`.
- Nosotros escalamos 502.8 / 1.55 / 1.43 por masa relativa a un coach de 34 t.

**Implementación:**

| Paso | Trabajo |
|------|---------|
| 1 | Parsear `ORTSBearingType`, `ORTSWagonFrontalArea`, `ORTSDavisDragConstant`, conteo de ejes (`WheelAxles` / `NumWheels`) en `.eng`/`.wag` |
| 2 | Nuevo módulo `openrailsrs-train/auto_friction.rs`: `calc_davis_a/b/c(bearing, mass, axles, frontal_area, drag_const, wagon_type)` portando lógica de `TrainCar.UpdateTrainBaseResistance_*` |
| 3 | Reemplazar `estimate_davis_coefficients()`; mantener override `[train.davis]` en escenario |
| 4 | Test con vagones SCE MK2 (sin ORTSDavis en content) vs valores OR logueados en `-verboseconfig` |

**Criterios de aceptación:**

- [x] Test unitario: valores golden auto-friction (bearing, masa/ejes, C por área) — `auto_friction_golden`, `golden_assets`
- [x] Vagón SCE MK2 + Pullman PSG/DMBSA: A/B/C vs fórmula OR o ORTSDavis explícito en `.eng`/`.wag`
- [x] SCE 100 s sigue pasando sin `[train.davis]` en escenario
- [x] Chiltern puede quitar override manual cuando Pullman + 6 coaches coincidan con OR (`chiltern_validate` sin `[train.davis]`)

**Referencias OR:** PR [#1207 auto_friction](https://github.com/openrails/openrails/commit/d01848a), manual Physics § Davis.

**Estimación:** 4–6 días.

**Depende de:** ninguna (paralelo a OR-P1).

---

## OR-P3 — Rampas de tracción y fuerza continua (P1)

**Objetivo:** Transitorios suaves y protección térmica continua como OR.

**Gap actual:**

- OR: `TractionForceRampUpNpS`, `TractionForceRampDownNpS`, `TractionPowerRampUpWpS`, `AverageForceN` + `ContinuousForceTimeFactor`.
- Nosotros: solo `RunUpTimeToMaxForce` en modelos legacy.

**Implementación:**

| Paso | Trabajo |
|------|---------|
| 1 | Parsear rampas desde `.eng` / bloque Diesel |
| 2 | Estado en `TrainSimState`: `traction_force_n`, `average_force_n` por motor |
| 3 | `UpdateForceWithRamp` equivalente en `physics.rs` antes del paso de velocidad |
| 4 | Reducir fuerza cuando `AverageForceN` supera rating continuo |

**Criterios de aceptación:**

- [x] Experimento B (aceleración 100 % throttle): baseline OR + `chiltern_fullthrottle` (vel RMS 0–30 s ~0.47 m/s vs OR; audit ≤0.5). OR-P13 + run-up MSTS solo bajo notch completo.
- [x] No overshoot de velocidad >2 m/s vs OR en arranque Chiltern (`chiltern_startup_overshoot`)

**Estimación:** 3–4 días.

**Depende de:** OR-P1 (misma ruta de tracción).

---

## OR-P4 — Cablear simulación multi-cuerpo (P1)

**Objetivo:** Activar `multi_body_step()` en corridas normales, no solo en tests.

**Gap actual:**

- `coupler.rs` y `VehicleState` existen; `runner.rs` nunca inicializa `state.vehicles` → siempre masa puntual.
- `BrakeSystem::from_vehicles` ya usa posiciones del consist.

**Implementación:**

| Paso | Crate | Trabajo |
|------|-------|---------|
| 1 | `openrailsrs-sim/runner.rs`, `multi_runner.rs` | Inicializar `vehicles`, `couplers`, `vehicle_masses` desde `Consist` (longitudes, masas, offsets) |
| 2 | `openrailsrs-train` | Exponer `Consist::vehicle_layout() -> Vec<VehicleLayout>` (posición, masa, length_m) |
| 3 | `physics.rs` | Aplicar tracción solo a vehículo 0; resistencia Davis **por vehículo** (prep. OR-P5) | ✅ |
| 4 | `[simulation] multi_body = true` en escenario (default false hasta validado) | ✅ flag + init en runner |

**Criterios de aceptación:**

- [x] Test integración: tren 2 coches — retraso de aceleración del vagón vs loco visible vía `physics::step()` (`multi_body_integration`)
- [x] Chiltern 136 s con multi_body + `time_step = 0.05`: RMS velocidad vs OR ~**0.52 m/s** (arranque 0–30 s ~0.26); test `chiltern_multi_body` (umbral 0.55)
- [x] Chiltern 136 s multi_body: RMS ≤ 0.40 m/s vs OR (~0.39 con sub-pasos, `time_step = 1.0`)
- [x] Frenada A1: cabeza EP adelanta primer vagón train-air — **`brake_f_head_n` / `brake_f_train_air_n` / `brake_f_tail_n`** + `scripts/analyze_brake_propagation.py` + test `chiltern_brake_coast_a1_script_passes` (sin baseline OR por cilindro)
- [x] Experimento A costa multi-cuerpo: 115–180 s ~0.16 m/s RMS vs OR (`chiltern_brake_coast_multi_body`; masa puntual ~0.07)

**Estimación:** 4–5 días.

**Depende de:** OR-P5 parcial (resistencia por vehículo recomendada).

---

## OR-P5 — Resistencia por vehículo en el paso físico (P1)

**Objetivo:** OR calcula `FrictionForceN` por `TrainCar`; nosotros sumamos Davis agregado.

**Implementación:**

| Paso | Trabajo |
|------|---------|
| 1 | `TrainPhysics`: `Vec<DavisCoefficients>` paralelo a vehículos | ✅ `vehicle_davis` + `Consist::per_vehicle_davis` |
| 2 | En multi-cuerpo: `f_resist_i` por vehículo; en masa puntual: suma (comportamiento actual) | ✅ |
| 3 | Opcional fase 1: resistencia en curva `(mass × μ × (gauge + wheelbase)) / (2 × radius)` desde `PathData.curve_radius_m` |

**Criterios de aceptación:**

- [x] Suma de resistencias por vehículo = resistencia agregada actual ±1 % en Chiltern/SCE (`multi_body_davis`)
- [ ] Con curva en track: deceleración en curva medible vs OR (Experimento free-roll en curva)

**Estimación:** 2–3 días (+2 días si incluye curva).

**Depende de:** OR-P2 (coeficientes por vehículo).

---

## OR-P6 — Frenos MSTS / OR completos (P1)

**Objetivo:** Ir más allá del proxy spring-ramp hacia el modelo de presión de OR.

**Gap actual:**

- OR: `MSTSBrakeSystem`, tipos de zapata (Karwatzki), blending DB/fricción, skid, `BrakeShoeCoefficientFriction` vs velocidad.
- Nosotros: cilindros con rampa fija + `BrakeCommandMapping` (121 PSI → cilindro); μ(v) opcional vía `brake_shoe_speed_factor` (P6b).

**Implementación (incremental):**

| Sub-fase | Alcance |
|----------|---------|
| P6a | Presión de cilindro como estado (0–max PSI), no fuerza directa; mapeo driver → reducción tubo |
| P6b | ✅ Coeficiente de zapata vs velocidad (`ORTSBrakeShoeType` / `ORTSBrakeShoeFriction`, μ(v)/μ(0) en cilindros) |
| P6c | ✅ Skid limit: `min(F, m·g·μ_adhesion)` por vehículo (`brake_skid_limit`) |
| P6d | Blending con freno dinámico (requiere OR-P10) |

**Criterios de aceptación:**

- [x] Experimento A (costa libre tras frenada fuerte): perfil v(t) post-suelta freno ≤ 0.5 m/s RMS vs OR — `scenario_brake_coast.toml` + `chiltern_brake_coast` (coast 115–180 s ~0.07 m/s RMS)
- [ ] Chiltern fase 0–40 s (frenos al inicio): mejora posición max sin empeorar velocidad global
- [x] OR-P6a parcial: precarga cilindros + lap hold train-air + `train_air_full_release_s` (Chiltern global ~0.39 m/s; 0–30 s ~0.54; 40–65 s ~0.33)
- [x] OR-P6b: parseo shoe type/curva + `F_efectiva(v) = F_cilindro × μ(v)/μ(0)`; flag `brake_shoe_speed_factor` en Chiltern (sin regresión en umbrales actuales)
- [x] OR-P6c: cap por adherencia rueda-carril (`brake_skid_limit`, μ=0.25 default OR) por cilindro

**Estimación:** P6a–c: 1–2 semanas.

**Depende de:** OR-P4 (distribución de fuerza por vehículo).

---

## OR-P7 — Señales y driver OR sin atajos (P1)

**Objetivo:** Validar Chiltern con señales reales y driver que respete aspectos.

**Gap actual:**

- Chiltern usa `assume_signals_clear = true`.
- `or-eval-driver` replay puro; no reacciona a Stop/Caution.
- OR Activity mode obedece señales y límites de velocidad del path.

**Implementación:**

| Paso | Trabajo |
|------|---------|
| 1 | Import MSTS: mapear `SignalItem` + aspectos dinámicos desde actividad (parcial en Fase 25b) |
| 2 | `ScriptedDriver` opcional: reducir throttle / aplicar freno ante Stop en bloque adelante |
| 3 | `[route] assume_signals_clear = false` en Chiltern cuando scripts + posiciones sean correctos |
| 4 | Baseline OR Activity (no Explorer) para misma ventana temporal |

**Criterios de aceptación:**

- [ ] Chiltern 136 s sin `assume_signals_clear`: RMS ≤ 0.5 m/s vs baseline Activity OR
- [ ] Ninguna violación de Stop (velocidad > 0.5 m/s en bloque rojo)

**Estimación:** 1–2 semanas (mucho contenido/ruta).

**Depende de:** scripts de señal ya en Fase 22; import actividad Fase 25b.

---

## OR-P8 — Resistencia ambiental (P2)

**Objetivo:** Curva, túnel, viento, temperatura de rodamientos.

**Implementación:**

| Componente | Fuente OR | Prioridad |
|------------|-----------|-----------|
| Resistencia en curva | `TrainCar` base curve resistance | Alta |
| Túnel | `TunnelCrossSection`, perimeter | Media |
| Viento | `WindDependency` | Baja |
| Starting resistance | `Friction0N` vs Davis extendido | Media |

**Criterios de aceptación:**

- [ ] Escenario con curva R=500 m: delta velocidad vs OR en costa libre ≤ 1 m/s RMS

**Estimación:** 1 semana por componente mayor.

---

## OR-P9 — Transmisión mecánica (gearbox + DieselTorqueTab) (P2)

**Objetivo:** Locomotoras `DieselTransmissionType = Mechanic` y DMUs con caja de cambios.

**Gap:** OR usa `GearBox.TractiveForceN`, embrague, `DieselTorqueTab`; rama completamente ausente en openrailsrs.

**Implementación:**

| Paso | Trabajo |
|------|---------|
| 1 | Parsear `DieselTorqueTab`, `GearBox` params, `DieselTransmissionType` |
| 2 | Subsistema `GearBoxState`: marchas, RPM motor vs velocidad rueda, slip embrague |
| 3 | Bypass curvas F(v,t) cuando `HasGearBox && Mechanic` |

**Criterios de aceptación:**

- [ ] Escenario de prueba con loco mecánico MSTS: velocidad máxima y aceleración dentro de 15 % vs OR

**Estimación:** 2–3 semanas.

---

## OR-P10 — Freno dinámico (P2)

**Objetivo:** `DynamicBrakeForceCurves`, `MaximumDynamicBrakePowerW`, blending.

**Implementación:** Parseo + rama en `physics.rs` cuando throttle = 0 y DB activo; integración con P6d.

**Estimación:** 1 semana.

---

## OR-P11 — Adhesión avanzada y wheel slip (P2)

**Objetivo:** `UseAdvancedAdhesion`, clima, `AdhesionFactor`, wheel slip dinámico más allá de Curtius estático.

**Estimación:** 1 semana.

---

## OR-P12 — Vapor avanzado OR (P2)

**Objetivo:** Sustituir / complementar `steam_step()` simplificado con Advanced Steam OR para contenido histórico.

**Estado:** Fase 24 ✅ modelo básico; sin paridad OR.

**Estimación:** 3+ semanas (proyecto separado).

---

## OR-P13 — Segundo motor Chiltern (DMBSH) (P1)

**Objetivo:** Cerrar gap del stub legacy P/v del DMBSH (cola Blue Pullman).

**Opciones (en orden de preferencia):**

1. Curvas `ORTSMaxTractiveForceCurves` reales en el `.eng` DMBSH (content).
2. Calibración dedicada `DieselTractionModel` desde baseline OR Experimento D.
3. Heredar curvas DMBSA con factor de escala por `MaxPower`/`MaxForce`.

**Criterios de aceptación:**

- [x] Chiltern fase 40–65 s: RMS ≤ 0.35 m/s (trail ORTS + run-up ~13 s + OR-P6a freno arrancada)
- [x] Experimento B 0–30 s: ~0.47 m/s RMS (audit ≤0.5; antes ~0.98)

**Estimación:** 2–4 días (depende de content).

**Depende de:** OR-P1, OR-P2.

---

## OR-P14 — Parámetros eléctricos y supply scripting (P2)

**Objetivo:** `ScriptedLocomotivePowerSupply`, límites de potencia auxiliar, EMU multi-coche.

**Estimación:** 2 semanas.

---

## OR-P15 — Eliminación de overrides de calibración (P0 cierre)

**Objetivo:** Escenarios validables solo con assets MSTS/OR, sin `[train.davis]` ni `[simulation]` hacks.

**Checklist final:**

- [ ] `examples/chiltern/scenario.toml` — sin Davis manual ni `assume_signals_clear`
- [ ] `examples/sce/scenario.toml` — sin overlay de velocidad/freno salvo contenido MSTS incorrecto documentado
- [ ] Umbrales estrictos: Chiltern 0.30 m/s RMS / 25 m posición; SCE 0.35 m/s
- [ ] CI: `chiltern_validate`, `sce_validate`, tests de auditoría diesel

---

## Estrategia de validación transversal

Cada fase OR-P* debe incluir:

1. **Tests unitarios** — función pura portada de OR (tabla entrada/salida desde logs `-verboseconfig`).
2. **Tests de auditoría** — `crates/openrailsrs-train/tests/audit_*.rs` con `--nocapture`.
3. **Baseline OR** — CSV en `examples/baselines/`; ventana temporal documentada en README del escenario.
4. **compare-or** — `[validate]` en `scenario.toml`; no mergear si regresión > umbral acordado.
5. **Feature flag** — `[simulation] or_parity_level = 0|1|2` para activar comportamiento nuevo sin romper CI durante transición.

### Experimentos OR (desde CALIBRATION.md)

Todos los baselines OR usan el **consist completo** en simulación multi-cuerpo. Las corridas openrailsrs en la columna “Sim actual” usan **masa puntual** salvo que se indique `multi_body`.

| ID | Propósito | Consist | Sim openrailsrs | ¿Revisar con `multi_body`? | Prioridad |
|----|-----------|---------|-----------------|----------------------------|-----------|
| — | Chiltern eval 136 s | Pullman ×8 | Masa puntual | **Sí** — en curso (`scenario_multi_body.toml`) | Alta |
| A (freno+costa) | Pullman ×8 | Masa puntual | **Sí** — `scenario_brake_coast_multi_body.toml` (~0.16 m/s costa) | Alta ✅ |
| B | Aceleración 100 % | Pullman ×8 | Masa puntual | **Sí** — `scenario_throttle100_multi_body.toml` (~0.47 m/s arranque) | Media ✅ |
| C | Crucero 75 % notch | Pullman ×8 | Masa puntual | Opcional — régimen casi uniforme | Baja |
| E | Throttle 50 % (30 s) | Pullman ×8 | Masa puntual | Opcional — mismo motivo | Baja |
| — | SCE eval 100 s | 47 + MK2 ×6 | Masa puntual | **Sí** — tras estabilizar acopladores Chiltern | Media |
| CALIBRATION A | Coast-down Davis puro | — | — | No aplica (sin baseline OR multi vs single) | — |

**Criterio de revisión:** no invalidar baselines OR (siguen siendo verdad OR). Re-ejecutar openrailsrs con `multi_body = true` + `time_step ≤ 0.05`, comparar RMS y decidir si el umbral del test sube temporalmente o si se afina acoplador antes. **No hace falta recapturar Wine** salvo que cambiemos el driver o la ventana temporal.

| ID | Fase que consume |
|----|------------------|
| A (freno+costa) | OR-P2, OR-P5, OR-P6 |
| B | OR-P1, OR-P3 ✅ |
| C | OR-P1 ✅ parcial (E 50 % ✅; C 75 % infra + baseline OR pendiente) |
| D | OR-P13 |
| E (50 %) | OR-P1 / diesel notch |

---

## Cronograma sugerido

| Mes | Entregables |
|-----|-------------|
| **1** | OR-P1 + OR-P2 + OR-P13 → Chiltern/SCE estables sin hacks de potencia/Davis |
| **2** | OR-P3 + OR-P4 + OR-P5 → multi-cuerpo opt-in; resistencia por vehículo |
| **3** | OR-P6a–c + OR-P7 → frenos y señales Chiltern Activity |
| **4+** | OR-P8 … OR-P12 según contenido objetivo (gearbox, vapor, etc.) |

---

## Matriz de trazabilidad (gap → fase)

| Gap del análisis | Fase |
|------------------|------|
| ApparentThrottle / ReverseThrottleRPMTab | OR-P1 |
| Cap P/v rail power vs DieselPowerTab | OR-P1 |
| Auto-friction CN / bearing type | OR-P2 |
| Rampas tracción / fuerza continua | OR-P3 |
| Multi-cuerpo no cableado | OR-P4 |
| Davis agregado vs por vehículo | OR-P5 |
| Frenos MSTS completos | OR-P6 |
| assume_signals_clear / driver | OR-P7 |
| Curva / túnel / viento | OR-P8 |
| Gearbox + DieselTorqueTab | OR-P9 |
| Freno dinámico | OR-P10 |
| Adhesión avanzada / wheel slip | OR-P11 |
| Vapor avanzado | OR-P12 |
| DMBSH stub | OR-P13 |
| UnloadingSpeed | OR-P1 |
| DieselEngineFractionPower | OR-P1 |
| TractiveForcePowerLimited | OR-P1 |
| Overrides `[train.davis]` | OR-P15 |

---

## Riesgos y mitigaciones

| Riesgo | Mitigación |
|--------|------------|
| Regresión Chiltern al quitar hacks | Feature flag + baseline congelado; OR-P15 solo al final |
| Baselines OR vs sim masa puntual | Documentado arriba; revisar Exp A/B y Chiltern con `multi_body` tras sub-pasos acoplador |
| Content MSTS inconsistente (DMBSH) | Documentar desviaciones; tests por escenario, no global |
| Complejidad frenos MSTS | Entrega incremental P6a→d; mantener proxy como fallback |
| Gearbox mecánico amplio | Fase separada; no bloquea diesel-eléctrico |
| Logs OR difíciles de reproducir | Scripts Wine/documentados en `examples/*/README.md` |

---

## Próximo paso recomendado

1. **OR-P4:** calibrar acopladores → Chiltern multi-cuerpo RMS ≤ **0.40 m/s** vs OR.
2. **Revisión experimentos (tabla arriba):** Exp **A** (freno+costa) y **B** (100 %) con `multi_body`.
3. **OR-P1 cierre:** auditorías SCE/Chiltern sin overrides de potencia (en paralelo, no bloqueado por multi).

Cuando una fase esté en progreso, actualizar el estado en este archivo y enlazar el PR en la tabla de la fase correspondiente.

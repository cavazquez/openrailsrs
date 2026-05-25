# Roadmap openrailsrs

Orden de trabajo para un **simulador ferroviario headless-first** que evoluciona a videojuego de simulación.

**Restricciones transversales:** Rust estable, Linux-first, CSV para series temporales, TOML para escenarios y metadata, sin motor gráfico (Bevy/wgpu) en el stack headless.

---

## Leyenda

| Símbolo | Significado |
|---------|-------------|
| ✅ | Implementado y con tests |
| 🔶 | Base funcional, profundizable |
| 🔲 | Planeado (no iniciado) |

---

## Fase 0 — Bootstrap ✅

**Objetivo:** Workspace Cargo reproducible y documentado.

**Implementado:**
- Workspace multi-crate bajo `crates/`, binario `openrailsrs-cli`.
- `rust-toolchain.toml` (stable), `edition = "2024"`, `rust-version = "1.85"`.
- `check.sh` local + GitHub Actions CI (`cargo fmt`, `clippy -D warnings`, `cargo test`).
- Cobertura con `cargo-llvm-cov` → Codecov.
- Dependabot mensual para Cargo.

---

## Fase 1 — Parsers MSTS / Open Rails ✅

**Objetivo:** Tokenizer + parser para S-expressions de `.trk`, `.eng`, `.wag`, `.con`.

**Implementado:**
- `openrailsrs-formats`: lexer, AST genérico, adaptadores tipados (`EngineFile`, `WagonFile`, `ConsistFile`, `RouteFile`).
- Conversiones de unidades MSTS → SI (`lb → kg`, `kW`, `mph → m/s`, `kN → N`).
- Dispatch por extensión de archivo (`parse_by_extension`).
- CLI `openrailsrs inspect <file>`.
- Tests con fixtures `.eng`, `.wag`, `.con`, `.trk`.

---

## Fase 2 — Datos y configuración del juego ✅

**Objetivo:** Esquema TOML para escenarios jugables con validación explícita.

**Implementado:**
- `openrailsrs-scenarios`: `scenario.toml` con `[route]`, `[train]`, `[gameplay]`, `[simulation]`, `[output]`.
- Paradas intermedias `[[route.stops]]` con `arrive_s`, `depart_s`, `dwell_s`.
- Override de resistencia Davis `[train.davis]`.
- Agujas por escenario `[[route.switches]]` (`straight` / `diverging`).
- Multi-tren `[[extra_trains]]` con id, consist, start_time_s, output_csv.
- `penalty_per_second_late` en `[gameplay]` para penalizaciones graduales.
- Validación con mensajes de error explícitos.

---

## Fase 3 — Modelo lógico ferroviario ✅

**Objetivo:** Grafo de vía con nodos, aristas, señales y agujas.

**Implementado:**
- `openrailsrs-track`: grafo tipado, `NodeKind` (Plain / Switch / Station), `EdgeId`, `NodeId`.
- Señales (`TrackSignal`): `id`, `edge_id`, `position_m`, `aspect` (Stop/Caution/Clear), `clear_after_s`.
- Agujas: `SwitchPosition` (Straight/Diverging), `set_switch`, `switch_position`, `NotASwitch`.
- `openrailsrs-route`: carga de `track.toml` con `grade_percent`, `[[signals]]`, `default_position`.
- BFS switch-aware en `path.rs`: filtra aristas según posición activa de cada nodo Switch.
- Export DOT: `openrailsrs graph <route> --out route.dot`.

---

## Fase 4 — Modelo físico del tren ✅

**Objetivo:** Consists realistas con curva de tracción, Davis y pendientes.

**Implementado:**
- `openrailsrs-train`: `Locomotive`, `Wagon`, `Consist`.
- `DavisCoefficients` (a_N, b_N·s/m, c_N·s²/m²) configurables por consist o escenario.
- `TractiveCurve`: puntos (v → F) interpolados en piecewise-linear; fallback a ley P/v.
- `TrainPhysics`: agrega curvas de tracción e impulso de freno del consist completo.
- Conversiones de unidades MSTS desde adaptadores tipados.

---

## Fase 5 — Simulación headless ✅

**Objetivo:** Ejecutar el tren sobre el grafo sin gráficos; salidas reproducibles.

**Implementado:**
- `openrailsrs-sim`: `physics::step` con `TractiveCurve` + Davis + grade.
- Máquina de estados por tren: `Normal → Approaching → Dwelling → AwaitingSignal`.
- `AutoDriver` (control automático) y `ScriptedDriver` (replay desde CSV `time_s,throttle,brake`).
- `run_from_scenario_file`, `run_from_scenario_file_with_driver`.
- Salida: `run.csv` (series temporales) + `run.toml` (metadata).
- Tests de integración: physics_headless, run_smoke_example, scripted_driver, signal_enforcement, switch_pathfinding.
- Benchmark Criterion `sim_step`.

---

## Fase 6 — Capa de videojuego (headless) ✅

**Objetivo:** Reglas jugables: objetivos, horarios, scoring, penalizaciones, eventos.

**Implementado:**
- `openrailsrs-game`: `evaluate`, `PlayOutcome`, `StopResult`, `TimelineEvent`.
- Penalizaciones: `missed_stop`, `late_stop` (graduado × `penalty_per_second_late`), `early_departure`, `overspeed`, `late_arrival`.
- `PlayOutcome.punctuality_pct` y `PlayOutcome.total_delay_s`.
- `play_headless_from_scenario_file`; CLI `openrailsrs play-headless`.
- **Multi-tren sincronizado** (`multi_runner.rs`): `BlockMap` (una arista = un bloque), `AgentPhase::WaitingForBlock`, `BlockWait`/`BlockClear` en `SimEvent`.
- `run_scenario_multi_train`; CLI `openrailsrs sim-multi`.

---

## Fase 7 — Validación y comparación ✅

**Objetivo:** Cuantificar diferencias entre corridas.

**Implementado:**
- `openrailsrs-validate`: comparación cuantitativa de dos `run.csv`.
- CLI `openrailsrs compare run1.csv run2.csv`.

**Profundizado:**
- `ValidationConfig`: tolerancias por columna (`max_velocity_rms`, `max_velocity_max`, `max_position_*`, `max_energy_*`).
- `ComparisonReport` con `pass`/`fail` por columna y `pass` global.
- CLI `compare` con flags `--max-velocity-rms`, `--max-position-max`, etc.; sale con exit code 1 si falla.
- 8 tests: idéntico/perturbado/strict config/smoke self-compare.

**Comparación Open Rails (Fase 7 ext.):**
- Parser `dump.csv` OR → traza normalizada; remuestreo lineal; CLI `compare-or`.
- Fixtures sintéticos en `openrailsrs-validate/tests/fixtures/`; docs en `docs/OR_TRACE_COMPARISON.md`.
- Sección opcional `[validate]` en `scenario.toml` (umbrales + `baseline_or` metadata).

**Pendiente:** Baseline OR real versionado; comparación topológica; energía vs OR.

---

## Fase 8 — Debug sin gráficos ✅

**Objetivo:** Depurar rutas y partidas sin viewer.

**Implementado:**
- `openrailsrs-export`: DOT, GeoJSON, mapa ASCII, replay textual, **replay animado** ANSI.
- `animated_replay_from_csv`: panel multi-línea, barra de progreso, velocidad configurable (`--speed`).
- CLI: `graph`, `export-geojson`, `ascii-map`, `replay`, `replay --watch`, `batch` (rayon).

---

## Fase 9 — Optimización ✅

**Objetivo:** Escenarios largos y lotes sin cuellos de botella.

**Implementado:**
- Benchmark Criterion `sim_step` en `openrailsrs-sim`.
- `rayon` en `batch`; `indexmap` para iteración determinista en el grafo.

**Profundizado:**
- `PathData`: pre-computa `Vec<PathEdgeData>` (`length_m`, `speed_limit_mps`, `grade_percent`) antes del bucle; `physics::step` usa indexación directa en lugar de `HashMap::get` en cada tick.
- Benchmarks Criterion: `physics_step_100` (micro), `full_scenario_smoke` (escenario completo), `full_scenario_multi_train` (multi-tren).

**Pendiente:** Profiling con escenarios más grandes (>50 km); paralelización de escenarios batch.

---

## Fase 10 — Viewer 2D animado ✅

**Objetivo:** Primera visualización 2D en crate separado, sin acoplar `sim`.

**Implementado:**
- `openrailsrs-viewer`: ventana `minifb` 1024×768.
- Topología: aristas naranjas gruesas con etiqueta, nodos coloreados por tipo (Plain/Switch/Station).
- **Señales** renderizadas como diamantes con su aspecto actual (rojo/amarillo/verde), poste y etiqueta.
- **Trenes animados**: interpolación de posición desde `run.csv` vía `edge_id + pos_on_edge_m`; glow proporcional a velocidad; paleta de 8 colores para multi-tren.
- **HUD**: nombre, tiempo simulado, barra de progreso, velocidad por tren, controles.
- Teclado: `Space` pausar · `R` reiniciar · `+`/`-` velocidad · `Esc` salir.
- Acepta `<route_dir>` (estático) o `<scenario.toml>` (animado).

---

## Fase 11 — Modo cabina jugable 🔲

**Objetivo:** El jugador controla el tren en tiempo real directamente desde el viewer.

**Ideas:**
- Teclas `↑`/`↓` (throttle), `B`/`N` (freno) en el viewer minifb.
- Feedback inmediato: límite de velocidad actual, aspecto de señal próxima, penalización acumulada.
- Tick de simulación sincronizado con el reloj de pared (modo real-time, no replay).
- Score mostrado en HUD.

---

## Fase 12 — Panel de despacho 🔲

**Objetivo:** Control de tráfico interactivo: cambiar señales y agujas en tiempo real.

**Ideas:**
- Click (o teclas) sobre señales/nodos en el viewer para cambiar su estado.
- Log de eventos visible en el HUD.
- Modo "dispatcher": el jugador gestiona múltiples trenes y evita colisiones.
- Penalización por bloqueos prolongados y colisiones.

---

## Fase 13 — Importar rutas reales ✅

**Implementado:**
- Nuevo crate `openrailsrs-import`: importa Overpass API JSON → `track.toml`.
- Algoritmo: indexado de nodos OSM, detección de junctions (nodos compartidos por ≥2 ways), segmentación de ways, proyección equirectangular, distancias Haversine, speed limit desde tag `maxspeed`.
- Soporta `railway=rail`, `light_rail`, `subway`, `tram`; estaciones (`railway=station`) como `NodeKind::Station`.
- Aristas **bidireccionales** por defecto (`bidirectional: true`); opción `--one-way` para importación unidireccional.
- CLI: `openrailsrs import-osm overpass.json --out routes/myroute/track.toml --route-id myroute`.
- Fixture del Badner Bahn (Viena) en `examples/osm/` + plantilla de query Overpass lista para usar.
- 11 tests incluyendo round-trip con `openrailsrs-route` (TOML generado → `TrackGraph`).
- Línea Mitre (Buenos Aires) importada en `examples/routes/mitre/` — 2133 nodos, 4926 aristas, 172 estaciones.

**Pendiente:** Importador simplificado de `.trk` de MSTS (topología sin splines 3D). **Parcial ✅** — `import-msts` emite `track.toml` compatible con `openrailsrs-route` (`[route]`, `speed_limit_kmh`, switches `{ stem_edge, diverging_edge }`, `RouteID` desde `.trk`).

---

## Fase 11/14 — Escenario real + Modo cabina ✅

**Implementado:**
- Escenario real **Retiro → Victoria** sobre la Línea Mitre importada de OSM: 22.9 km, 13 estaciones, 8 paradas intermedias con horario.
- Consist CAF 6000 (EMU eléctrico de 6 coches, 270 t, 900 kW) en `examples/routes/mitre/consists/`.
- Comando `openrailsrs cab <scenario.toml> [--speed N]`: modo cabina interactivo en terminal.
  - W/↑ acelerador, S/↓ freno, Espacio = freno de emergencia, Q = salir.
  - HUD en pantalla completa: velocidad actual / límite, barra de acelerador/freno, barra de progreso animada, tiempo y energía acumulada.
  - Velocidad de simulación configurable (por defecto 10× para sentir la inercia real).

---

## Fase 15 — Motor de campaña 🔲

**Objetivo:** Progresión de misiones con estado persistente.

**Ideas:**
- `campaign.toml`: lista ordenada de escenarios con requisitos de score para desbloquear.
- `progress.json`: almacena puntuaciones históricas y misiones completadas.
- CLI `openrailsrs campaign status` / `openrailsrs campaign play <mission_id>`.
- Dificultad dinámica: el motor ajusta parámetros según nivel.

---

---

## Fase 12/15 — Panel de despacho + Motor de campaña ✅

**Implementado:**
- Nuevo crate `openrailsrs-campaign`: `CampaignFile` + `MissionDef` + `Progress` + `MissionResult`; lógica de unlock basada en `min_pass_score`; persistencia en `progress.json`; `record_result` preserva el mejor score entre reintentos. 8 tests.
- `examples/mitre_campaign/campaign.toml`: 5 misiones progresivas (Tutorial → Retiro→Olivos → Retiro→San Isidro → Retiro→Victoria → Servicio duplo); dificultad, `bonus_threshold` y `sim_speed` configurables por misión.
- CLI `openrailsrs campaign status <campaign.toml>`: tabla con estado 🔒/▶/✅, mejor score y estrella bonus.
- CLI `openrailsrs campaign play <campaign.toml> <mission_id>`: ejecuta la misión, calcula score desde `PlayOutcome`, guarda progreso y muestra APROBADA/BONUS.
- CLI `openrailsrs campaign reset <campaign.toml>`: borra `progress.json`.
- CLI `openrailsrs dispatch <scenario.toml> [--speed N]`: panel de despacho interactivo multi-tren con `ratatui` + `LiveMultiSim`; tabla por tren (estado, velocidad coloreada, odómetro, progreso animado, energía neta, energía regen), log de eventos con bloqueos y llegadas, pausa/reanudar con Espacio, ajuste de velocidad con +/-.

---

## Cuatro mejoras combinadas (iteración actual) ✅

**Implementado:**

### D — Tracción regenerativa y consumo diésel
- `TrainPhysics` + `TrainSimState` extendidos con `regen_factor` y `diesel_sfc_g_per_kwh`.
- `step()` calcula: energía de tracción bruta → resta regen → acumula combustible diésel.
- CSV enriquecido: columnas `regen_energy_kwh` y `fuel_consumption_l`.
- `EngineFile` lee `(RegenFactor …)` y `(SpecificFuelConsumption …)` del AST MSTS.
- CAF 6000 configurado con `RegenFactor 0.70` (EMU moderno, recupera 70 % del freno).

### C — Brecha Victoria → Tigre cerrada
- 2 nodos sintéticos (`n_talar`, `n_delta`) + 6 aristas (3 forward + 3 reverse) añadidos a `track.toml`.
- Trayecto Retiro → Tigre: 78 hops, 28.1 km, BFS exitoso.
- Nuevo escenario `examples/routes/mitre/scenario_retiro_tigre.toml` con 12 paradas.

### B — HUD de puntualidad en modo cabina
- `cab.rs` pre-calcula la distancia acumulada a cada parada al cargar el escenario.
- HUD muestra: próxima parada, tiempo restante (o retraso en rojo), penalizaciones acumuladas, paradas pasadas.
- Línea de energía ampliada con `regen kWh` cuando el tren la tiene disponible.

### A — Dispatch multi-tren + misión duplo
- `LiveMultiSim` expuesto en `multi_runner.rs`: inicialización desde archivo, `step_frame(steps)` frame-by-frame, `LiveTrainSnapshot` por tren, `all_arrived()`.
- `dispatch.rs` refactorizado para usar `LiveMultiSim`; tabla dinámica con una fila por tren; detecta y registra bloqueos, liberaciones y llegadas en el log.
- Escenario `retiro_victoria_duo.toml`: dos CAF 6000, el segundo sale 3 minutos después.
- Misión "Servicio duplo" añadida a `mitre_campaign/campaign.toml` (requiere haber completado Retiro→Victoria).

---

## Fases 16, 17 y 18 ✅

### Fase 16 — Carga de pasajeros y masa variable
- `passengers_on`/`passengers_off` y `max_capacity` en modelo de escenario (`StopDef`, `TrainSection`).
- `passengers: u32` + `extra_mass_kg: f64` en `TrainSimState`; actualizados al salir de cada parada.
- `step()` usa `effective_mass = train.mass_kg + state.extra_mass_kg`.
- HUD de cabina: "Pasajeros N / capacidad (+X kg)" con color según ocupación.
- Columna `passengers` en CSV; fixture de ejemplo: `scenario_retiro_victoria.toml` con datos de boarding.

### Fase 17 — Audio sintetizado en modo cabina
- `AudioEngine::try_start()` lanza hilo con `rodio` 0.21 (ondas sinusoidales puras, sin archivos externos).
- CI-safe: devuelve `None` si no hay dispositivo de audio.
- Motor: volumen proporcional a velocidad. Freno: sine 800 Hz proporcional a brake. Bocina: sine 440 Hz 500 ms con tecla `H`.

### Fase 18 — Timetable multi-tren desde archivo TOML
- `TimetableFile` / `TimetableEntry` en `openrailsrs-scenarios` + `load_timetable()`.
- `LiveMultiSim::from_timetable(path)`: N agentes desde timetable, grafo compartido.
- `openrailsrs timetable run <timetable.toml>`: tabla de resultados + métricas de red (% llegados, bloqueos, energía media).
- Ejemplo: `examples/mitre_timetable.toml` (4 servicios Retiro → Victoria).
- Tests: `timetable_load.rs` (carga modelo) + `timetable_run.rs` (2 trenes llegan).

### Fase 19 — Física de frenos avanzada (freno de aire) ✅
- `BrakeSystem` + `BrakeCylinder`: tubería Westinghouse a ~200 m/s, retardo real por posición de vehículo.
- `physics::step()` usa `brake_system.total_force_n()` cuando hay cilindros; fallback escalar si `cylinders.is_empty()`.
- `runner.rs` y `multi_runner.rs` construyen el sistema desde el consist en carga.
- Test `brake_propagation.rs`: cilindro trasero (30 m) frena ~0.15 s después del frontal.

### Fase 20 — Dinámica de enganche (coupler forces) ✅
- `coupler.rs`: `VehicleState`, `CouplerState` (rigidez 2e6 N/m, amortiguación 1e5 N·s/m, holgura 0.05 m), `multi_body_step()`.
- `TrainSimState` tiene `vehicles`, `couplers`, `vehicle_masses`; vacíos → modo masa puntual (retrocompatible).
- `physics::step()` delega al solver multi-cuerpo si `state.vehicles` no está vacío.
- Test `coupler_forces.rs`: locomotora arranca, vagón quieto hasta que se tensa el enganche.

### Fase 22 — Señalización dinámica con scripts TOML ✅
- `SignalScript` en `TrackSignal`: `on_block_ahead`, `on_second_block_ahead`, `default`.
- `TrackGraph::evaluate_signals(block_map)`: BFS de 2 pasos, prioridad Stop > Caution > Clear.
- `runner.rs` y `multi_runner.rs` llaman `evaluate_signals` cada ~1 s de simulación.
- Formato `track.toml` extendido con `[signals.script]` inline; retrocompatible.
- 4 tests unitarios en `signal_script.rs`.

### Fase 24 — Tracción vapor ✅
- `SteamParams` en `openrailsrs-train`: cilindros, rueda, presión, evaporación, carbón/agua inicial.
- `BoilerState` + `steam_step()` en `openrailsrs-sim::steam`: F_te = n×(π/4)×bore²×stroke×P_mep/r; ODE de caldera; inyector automático.
- `physics.rs`: rama condicional steam (retrocompatible con tracción eléctrica/diesel).
- CSV extendido con `boiler_pressure_bar`, `water_kg`, `coal_kg` cuando loco es vapor.
- `steam_loader.rs`: loader TOML nativo `[engine]+[steam]`, detección automática vs MSTS S-expr.
- Ejemplo `examples/steam/`: 2-8-0 Consolidation 16 bar, 50 km con parada intermedia.
- 11 tests en `steam_physics.rs`.

### Fase 25 — Compatibilidad MSTS / Open Rails ✅
- `openrailsrs-formats`: `EngineFile` con `traction_curve`, `WagonFile` con `length_m`.
- Nuevos parsers: `TrackDbFile` (`.tdb`), `PathFile` (`.pat`), `ActivityFile` (`.act`).
- Crate `openrailsrs-msts`: `import_route` (TDB → `track.toml`), `import_activity` (ACT+PAT → `scenario.toml`).
- CLI: subcomando `import-msts <route_dir>` con auto-detección de `.act`.
- `From<EngineFile> for Locomotive` propaga curva de tracción real desde el `.eng`.
- 5 tests de integración con fixtures `minimal.tdb/.pat/.act`.

### Fase 25b — Compatibilidad MSTS completa ✅
- Encoding UTF-16 / Latin-1 en el lexer.
- Señales desde `TrItemTable` (`SignalItem` + `TrItemRefs` → `[[signals]]` con `edge_id` y aspecto inicial).
- `TrafficService` y paths múltiples (`Service_Definition` + `.pat`) → `[[extra_trains]]` en `scenario.toml`.
- Eventos de actividad: `ActivityObject` (→ `[[route.stops]]`), `FailedSignals` (→ `aspect="stop"` forzado), `RestrictedSpeedZones` (→ `speed_limit_mps` mínimo por edge).
- Metadata `StartTime` y `Season` → `[scenario].start_time_s` y `[scenario].season` (campos opcionales, retrocompatibles).
- API pública nueva: `import_route_with_activity(route_dir, act_path)` aplica overrides de actividad sobre el track.
- Fuera de alcance headless (planificados):
  - Shapes `.s` ASCII ✅ — parser en `openrailsrs-formats` (`ShapeFile`: puntos, normales, UVs, prim_states, LODs, jerarquía) + subcomando `shape-dump [--json]`. Binary tokenized devuelve `FormatError::UnsupportedBinaryShape` (queda para Fase 23).
  - Texturas `.ace` ✅ — crate `openrailsrs-ace` con decoder mip 0 (RGBA8 + DXT1/3/5 vía `texpresso`) + subcomando `ace-decode` que escribe PNG. Mips adicionales → Fase 23.
  - World tiles `.w` ASCII ✅ — parser en `openrailsrs-formats` (`WorldFile`: Static / Forest / TrackObj / Signal / Dyntrack) preservando posiciones locales + subcomando `world-dump [--csv]`. Resolución a coordenadas globales → Fase 23.
  - `SoundRegions` ✅ — `SoundSourceItem` + bloque `SoundRegions` en `.act` → `[[sound_regions]]`; detección en cabina; crate `openrailsrs-audio`.

### Viewer 3D (issue #8 / Fase 23) — prioridad #1 del plan en `docs/OPEN_RAILS_VIEWER_3D.md`
- Crate **`openrailsrs-viewer3d`**: app Bevy 0.18 (solo X11 en Linux por defecto), ventana 1280×720, plano + grilla + ejes, cámara orbit (`F1`) / fly (`F2`), cursor confinado en fly con botón derecho.
- **Grafo 3D desde `track.toml` ✅** — cilindros naranja (aristas), esferas por tipo de nodo, plano/grilla/cámara encuadrados al bounding box; CLI `openrailsrs-viewer3d [route_dir]`.
- **Marcador de tren desde CSV ✅** — `openrailsrs-viewer3d scenario.toml` + replay animado; HUD en pantalla (Bevy UI).
- **Pulido D ✅** — cámara follow (`T`), señales 3D coloreadas, modo compact automático en rutas >800 aristas (gizmo lines).
- **Objetos `.w` como cajas ✅** — tiles `WORLD/` → cubos coloreados por tipo en posición global.
- **Shape `.s` → mesh Bevy ✅** — LOD más cercano desde `ShapeFile`; `Static` con shape en `SHAPES/`.
- **Textura `.ace` en material ✅** — mip 0 vía `openrailsrs-ace` → `Image` Bevy; `TEXTURES/` en `StandardMaterial`; fallback magenta si falta.
- **Terreno heightfield ✅** — `.y` + `_Y.RAW` → mesh Bevy por tile; parches OR 17×17; demo smoke con colina junto a `yard_a`.
- **Terreno PR2 ✅** — TERRTEX dual-textura, UV afín por parche, `_F.RAW` agujeros, tile vecino smoke.
- **Vía dinámica (básica) ✅** — `Dyntrack` en `.w` → segmento orientado (durmientes + rieles); teletransporte `G`; coords en HUD.
- **Rolling stock PR1–PR3 ✅** — consist, orientación/escala, multi-tren desde escenario.
- **Bosque / agua / lluvia / pulido visual ✅** — órdenes 11 completas en viewer3d.

---

## Fase 15 — Editor de rutas 🔲

**Objetivo:** Crear y editar `track.toml` de forma interactiva.

**Ideas:**
- Subcomando `openrailsrs edit <route_dir>` que abre el viewer en modo edición.
- Click para agregar nodos; drag para conectar aristas.
- Panel de propiedades: editar `length_m`, `speed_limit_kmh`, `grade_percent`.
- Colocar señales y agujas visualmente.
- Guardar directamente el `track.toml` resultante.

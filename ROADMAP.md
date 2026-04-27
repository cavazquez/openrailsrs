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

**Pendiente:** Comparación sistemática con trazas de Open Rails.

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

**Pendiente:** Importador simplificado de `.trk` de MSTS (topología sin splines 3D).

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

## Fase 15 — Editor de rutas 🔲

**Objetivo:** Crear y editar `track.toml` de forma interactiva.

**Ideas:**
- Subcomando `openrailsrs edit <route_dir>` que abre el viewer en modo edición.
- Click para agregar nodos; drag para conectar aristas.
- Panel de propiedades: editar `length_m`, `speed_limit_kmh`, `grade_percent`.
- Colocar señales y agujas visualmente.
- Guardar directamente el `track.toml` resultante.

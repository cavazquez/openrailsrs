# Roadmap openrailsrs

Orden de trabajo para un **simulador ferroviario headless-first** que evoluciona a videojuego de simulación: primero núcleo físico/lógico y herramientas de consola; la visualización queda al final y en crates separados.

**Restricciones transversales:** Rust estable, Linux-first, CSV para series temporales, TOML para escenarios y metadata, sin motor gráfico (Bevy/wgpu) en el stack headless hasta la fase de viewer.

---

## Fase 0 — Bootstrap

**Objetivo:** Tener un workspace Cargo reproducible y documentado.

**Entregables:** Workspace `openrailsrs`, crates bajo `crates/`, binario `openrailsrs`, dependencias base (`serde`, `anyhow`, `thiserror`, `clap`, `toml`, `csv`), README.

**Criterio:** `cargo build` en Linux con toolchain estable.

---

## Fase 1 — Parsers MSTS / Open Rails (S-exp)

**Objetivo:** Tokenizer + parser AST genérico para textos tipo S-expression usados en `.trk`, `.eng`, `.wag`, `.con`.

**Entregables:** Crate `openrailsrs-formats`; CLI `openrailsrs inspect <file>`.

**Criterio:** Fixtures mínimos y tests que parseen sin error.

**Profundidad futura:** Ampliar cobertura de tokens y archivos reales; adaptadores por tipo de archivo encima del AST genérico.

---

## Fase 2 — Datos y configuración del juego

**Objetivo:** Esquema TOML para escenarios jugables (`scenario.toml`) con validación explícita.

**Entregables:** Crate `openrailsrs-scenarios`; mensajes de error claros en carga/validación.

**Criterio:** Cargar `scenario.toml` y fallar con errores entendibles ante datos inválidos.

---

## Fase 3 — Modelo lógico ferroviario

**Objetivo:** Representar ruta/vía como grafo (nodos, aristas, límites de velocidad; base para agujas, estaciones y señales).

**Entregables:** `openrailsrs-track`, `openrailsrs-route` (p. ej. `track.toml`); export DOT para depuración; CLI `openrailsrs graph <route> --out route.dot`.

**Criterio:** Grafo exportable a DOT válido para Graphviz.

**Profundidad futura:** Tiles MSTS, agujas y señales alineadas con contenido real.

---

## Fase 4 — Modelo físico del tren

**Objetivo:** Locomotoras, vagones, consists; masa, potencia, frenado (y más adelante tracción/resistencia/pendiente finos).

**Entregables:** `openrailsrs-train` leyendo desde AST de `.eng` / `.wag` / `.con`.

**Criterio:** Construir un consist desde fixtures y usarlo en simulación.

**Profundidad futura:** Unidades MSTS reales (lb, kW, etc.) y paridad con Open Rails donde aplique.

---

## Fase 5 — Simulación headless

**Objetivo:** Ejecutar el tren sobre el grafo sin gráficos; salidas reproducibles.

**Entregables:** `openrailsrs-sim`; `run.csv` (series) + `run.toml` (metadata); CLI `openrailsrs sim scenario.toml`; semilla documentada en metadata.

**Criterio:** Misma entrada → misma salida (determinismo en el bucle actual); tests de aceleración/frenado.

**Profundidad futura:** RNG solo donde haga falta; orden determinista en toda la simulación.

---

## Fase 6 — Capa de videojuego (headless)

**Objetivo:** Reglas jugables sobre la simulación: objetivos, horarios, scoring, penalizaciones, eventos, éxito/fracaso.

**Entregables:** `openrailsrs-game`; CLI `openrailsrs play-headless scenario.toml`; resultado serializado (p. ej. `outcome.toml`).

**Criterio:** Partida resoluble por consola con `success` / `score` / `penalties` / `timeline`.

---

## Fase 7 — Validación y comparación

**Objetivo:** Cuantificar diferencias entre corridas (propias o frente a exportaciones externas).

**Entregables:** `openrailsrs-validate`; CLI `openrailsrs compare run1.csv run2.csv`.

**Criterio:** Métricas agregadas (p. ej. velocidad, posición/odómetro, energía).

**Profundidad futura:** Comparación sistemática con trazas de Open Rails.

---

## Fase 8 — Debug sin gráficos

**Objetivo:** Depurar rutas y partidas sin viewer.

**Entregables:** En `openrailsrs-export` (y CLI): DOT, GeoJSON, mapa ASCII, replay textual; logs detallados vía `tracing` donde corresponda.

**Criterio:** Inspección útil de topología y de series temporales solo con archivos y consola.

---

## Fase 9 — Optimización

**Objetivo:** Escenarios largos y lotes de ejecuciones sin cuellos de botella obvios.

**Entregables:** Benchmarks (p. ej. Criterion en `openrailsrs-sim`); `rayon` donde aporte; CLI batch de escenarios.

**Criterio:** Documentar cómo medir y ejecutar batch; mejoras incrementales según perfiles.

---

## Fase 10 — Viewer mínimo

**Objetivo:** Primera visualización 2D en crate **separado**, sin acoplar `sim` al render.

**Entregables:** `openrailsrs-viewer` (binario propio); el juego/sim headless sigue siendo la fuente de verdad.

**Criterio:** Ver topología 2D; 3D y UX rica quedan para iteraciones posteriores.

---

## Leyenda de estado (opcional)

Las fases 0–10 tienen una **línea base** implementada en el repo (workspace, CLI, sim headless, game headless, validate, export, bench, viewer mínimo). Las sub-bullets “Profundidad futura” marcan trabajo continuo, no un bloque único de “done”.

Para comandos concretos, ver [README.md](README.md).

# Roadmap: simulación 3D jugable (trenes, cabina, mundo)

Documento de referencia para pasar de **viewer 3D + replay CSV** a un **simulador visual jugable** alineado con Open Rails, sin romper el núcleo headless.

Relacionado:

- Arquitectura OR vs Bevy: [`OPEN_RAILS_VIEWER_3D.md`](OPEN_RAILS_VIEWER_3D.md)
- Vía MSTS / Track Viewer vs `--track-dev`: [`TRACKVIEWER_STUDY.md`](TRACKVIEWER_STUDY.md)
- Instalación contenido MSTS/OR: [`CHILTERN_OR_SETUP.md`](CHILTERN_OR_SETUP.md)
- Issue GitHub: [#8](https://github.com/cavazquez/openrailsrs/issues/8)
- Fase 23 en [`ROADMAP.md`](../ROADMAP.md) (lista histórica; este doc es la fuente de verdad para fases **jugables**)

---

## 1. Principios de diseño

| Principio | Implicación |
|-----------|-------------|
| **Sim autoritativo** | Toda la física vive en `openrailsrs-sim`. Bevy solo presenta y captura input. |
| **Headless primero** | `openrailsrs-cli sim`, tests y CI no dependen de Bevy/X11. |
| **Crates separados** | `openrailsrs-viewer3d` (o futuro `play3d`) depende de `sim`, no al revés. |
| **Contenido MSTS externo** | El repo no redistribuye rutas completas ni meshes Pullman; el usuario apunta a su carpeta OR/MSTS. |
| **Paridad incremental** | Cada fase tiene criterio de “hecho” medible (visual + tests donde aplique). |

```mermaid
flowchart TB
  subgraph headless["Núcleo headless"]
    sim["openrailsrs-sim"]
    train["openrailsrs-train"]
    scenarios["openrailsrs-scenarios"]
    game["openrailsrs-game"]
  end
  subgraph presentacion["Presentación"]
    v3d["openrailsrs-viewer3d"]
    cab_tty["openrailsrs cab TTY"]
    audio["openrailsrs-audio"]
  end
  subgraph contenido["Contenido usuario"]
    msts["MSTS / OR Content"]
  end
  msts -->|SHAPES ACE WORLD TERRAIN| v3d
  msts -->|eng wag con| train
  scenarios --> sim
  train --> sim
  sim -->|LiveDriveSession| v3d
  sim -->|step| cab_tty
  cab_tty --> audio
  game --> sim
```

---

## 2. Estado actual del código (inventario)

### 2.1 Viewer 3D (`openrailsrs-viewer3d`)

| Módulo | Archivo | Qué hace |
|--------|---------|----------|
| Entrada | `src/main.rs` | `route_dir`, `scenario.toml` (replay CSV), o **`--live scenario.toml`** |
| Mundo | `world.rs`, `terrain.rs`, `forest.rs`, `water.rs`, `sky.rs`, `precipitation.rs` | Tiles `.w`, `.y`/RAW, bosque, agua, cielo, lluvia |
| Vía | `track.rs`, `dyntrack.rs`, `signals.rs` | Grafo `track.toml`, dyntrack básico, marcadores de señal |
| Trenes | `train.rs`, `rolling_stock.rs`, **`live.rs`** | Replay CSV o sim en vivo; meshes `.s` por vehículo |
| Assets | `shapes.rs` | `.s` ASCII → mesh; `.ace` mip 0 → textura Bevy |
| UI | `hud.rs`, `camera.rs`, `teleport.rs` | HUD, orbit/fly/follow, teletransporte |

**Modos de lanzamiento:**

```bash
# Solo topología / mundo (sin tren)
cargo run -p openrailsrs-viewer3d -- examples/smoke/routes/test

# Replay de sim headless previa
cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml
cargo run -p openrailsrs-viewer3d -- examples/smoke/scenario.toml

# Sim en tiempo real (Fase A+B ✅): W/S, H bocina, HUD paradas/penalización
cargo run -p openrailsrs-viewer3d -- --live examples/smoke/scenario.toml
```

### 2.2 Sim en vivo (`openrailsrs-sim`)

| API | Archivo | Rol |
|-----|---------|-----|
| `LiveDriveSession` | `live_drive.rs` | Misma inicialización que `run_scenario_headless` (consist, freno, `start_offset`, diesel RPM); `step_realtime(dt)` |
| `step` | `physics.rs` | Integración longitudinal; diesel ORTS, freno aire, vapor |
| `LiveMultiSim` | `multi_runner.rs` | Multi-tren frame-a-frame (usado por `dispatch`, no por viewer3d) |

### 2.3 Cabina hoy (`openrailsrs cab`)

- **Solo terminal** (`crates/openrailsrs-cli/src/cab.rs`): ratatui, W/S/Space, física idéntica a `sim`.
- **Audio** vía `openrailsrs-audio` + `RegionTracker` (`openrailsrs-scenarios/src/sound_regions.rs`; también en `--live`).
- **Sin ventana 3D** ni cabview MSTS.

### 2.4 Parsers de assets (headless)

| Formato | Crate | Viewer 3D | Limitación |
|---------|-------|-----------|------------|
| `.s` shape ASCII | `openrailsrs-formats` | ✅ mesh | `.s` binario tokenized → error |
| `.ace` textura | `openrailsrs-ace` | ✅ mip 0 | Sin mips altos; BGRA MSTS parcial |
| `.w` world | `openrailsrs-formats` | ✅ cubos/meshes | |
| `.y` + RAW terreno | `openrailsrs-formats` | ✅ heightfield + TERRTEX | |
| `cabview` / `cabview3d` | — | 🔶 | Carga mesh + 37/39 `.ace`; ver [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md) |
| `.sms` / `.wav` (TDB) | parseo en `track_db.rs` | ❌ playback | Solo metadata → `[[sound_regions]]` |

### 2.5 Qué está en git vs qué debe instalar el usuario

| En el repositorio | Fuera del repo (usuario) |
|-------------------|---------------------------|
| `examples/smoke/routes/test/` — demo 3D completa (SHAPES, TERRTEX, TERRAIN, WORLD) | Carpeta de ruta MSTS/OR completa (Chiltern, SCE, etc.) |
| `examples/chiltern/track.toml` + stubs `.eng`/`.wag` (solo física, **sin** `WagonShape`) | `ROUTES/Chiltern/WORLD`, `SHAPES`, sonidos, cabview3d del trainset |
| Baselines CSV OR en `examples/baselines/` | Capturas nuevas con Wine + OR 1.6.x |
| Fixtures MSTS en `crates/*/tests/fixtures/` | |

Scripts relevantes:

- `scripts/sync_chiltern_assets.sh` → copia **física** Pullman desde OR Content a `examples/chiltern/trains/` (no meshes ni cabina).
- `openrailsrs import-msts` → `track.toml` + `scenario.toml` desde `.tdb`/`.act`.

---

## 3. Imágenes y texturas: de dónde salen

### 3.1 Jerarquía MSTS / Open Rails

Open Rails usa la misma disposición de carpetas que MSTS. El viewer resuelve paths relativos al **directorio de ruta** pasado a `openrailsrs-viewer3d` (o al `route.path` del escenario).

```
ROUTES/MiRuta/
├── MiRuta.tdb          # topología (import → track.toml en openrailsrs)
├── TERRAIN/            # *.y, *_y.raw, *_f.raw
├── TERRTEX/            # grass.ace, microtex.ace, …
├── WORLD/              # *.w (objetos estáticos, bosque, agua, dyntrack)
├── SHAPES/             # *.s (geometría 3D)
└── TEXTURES/           # *.ace (texturas referenciadas por shapes y world)

TRAINS/TRAINSET/MiTren/
├── MiLoco.eng
├── MiVagon.wag
├── SHAPES/             # a veces duplicado por trainset
├── TEXTURES/
└── CABVIEW3D/          # cabina 3D OR (no soportada aún)
    └── *.ace, *.cvf
```

### 3.2 Cómo el viewer carga cada capa

| Capa visual | Origen en disco | Código |
|-------------|-----------------|--------|
| Terreno | `TERRAIN/*.y` + `*_Y.RAW` + parches en `.y` | `terrain.rs`, shader `assets/shaders/terrain.wgsl` |
| Texturas suelo | `TERRTEX/*.ace` | `terrain_material.rs` |
| Objetos fijos | `WORLD/*.w` → `Static` con `FileName` → `SHAPES/foo.s` | `world.rs` + `shapes.rs` |
| Bosque / agua | `Forest`, `HWater` en `.w` | `forest.rs`, `water.rs` |
| Vía decorativa | `Dyntrack` en `.w` | `dyntrack.rs` (sin perfiles TSection del `.tdb`) |
| Material rodante | `WagonShape` / `Shape (...)` en `.eng`/`.wag` del consist | `rolling_stock.rs` busca en `route_dir` y `scenario_dir/SHAPES` |
| Fallback | Sin shape o shape binario | Cubo coloreado |

**Convención de búsqueda** (`shapes.rs`): `route_dir/SHAPES`, `route_dir/shapes`, y carpetas del escenario.

### 3.3 Demo commiteada vs Chiltern real

| Escenario | Meshes 3D en viewer | Motivo |
|-----------|---------------------|--------|
| `examples/smoke` | ✅ `test.s`, `yard_shed.s`, terreno texturizado | Assets mínimos **en git** |
| `examples/chiltern` (solo repo) | ❌ cubos | Stubs de `sync_chiltern_assets.py` sin `WagonShape` |
| Chiltern + Content OR | ✅ si apuntás el viewer a la carpeta de ruta MSTS | Ej.: `ROUTES/Chiltern` con WORLD/SHAPES reales |

Para ver el Pullman en 3D hace falta **una de**:

1. Apuntar `openrailsrs-viewer3d` al directorio de ruta MSTS/OR que tenga `WORLD/` y `SHAPES/` del trainset Blue Pullman, **o**
2. Extender `sync_chiltern_assets.py` (fase D) para copiar también shapes/texturas (licencia/redistribución a criterio del usuario).

### 3.4 Formatos imagen soportados hoy

| Formato | Uso | Herramienta |
|---------|-----|-------------|
| `.ace` | Texturas MSTS/OR (DXT/RGBA) | `openrailsrs ace-decode in.ace out.png` |
| PNG | Salida de debug; no entrada directa en viewer | `ace-decode` |
| GLTF/OBJ | Mencionado en ROADMAP futuro | **No implementado** |

---

## 4. Sonidos: de dónde salen (y qué falta)

### 4.1 Open Rails / MSTS (referencia)

En OR, el sonido real proviene de:

- **Locomotora/vagón:** entradas en `.eng`/`.wag` (streams, `.wav` bajo el trainset).
- **Ruta:** `SoundSourceItem` en `.tdb` referenciando `.sms` o `.wav`.
- **Actividad:** bloque `SoundRegions` en `.act` (override de tipo/volumen).
- **Cabina:** eventos ligados a controles en scripts/C# y cabview (no portados).

Estructura típica en Content:

```
ROUTES/Chiltern/SOUND/          # o similar según ruta
TRAINS/TRAINSET/RF_Blue_Pullman/*.wav
```

### 4.2 Qué hace openrailsrs hoy

| Capa | Implementación | Archivos |
|------|----------------|----------|
| Motor cab | **`openrailsrs-audio`** — sinusoides vía `rodio` | `crates/openrailsrs-audio/src/lib.rs` |
| Motor | Frecuencia ~60 Hz; volumen ∝ velocidad | `SetVelocity` |
| Freno | ~800 Hz × intensidad freno | `SetBraking` |
| Bocina | 440 Hz, 500 ms | `Horn` |
| Ambiente | Loop por `[[sound_regions]]`; `kind` → Hz (tunnel 90, urban 320, …) | `EnterRegion` / `LeaveRegion` |
| Import regiones | TDB `SoundSourceItem` + overrides `.act` | `import_activity.rs` → `build_sound_regions` |
| Playback `.sms`/`.wav` | **No** | Parseado en TDB; nombre `.sms` no se usa al reproducir |

**Integración actual:** `openrailsrs cab` (TTY) y **`openrailsrs-viewer3d --live`** (motor/freno/bocina/regiones).

### 4.3 De dónde sacar sonido “real” en el futuro

| Fuente | Fase sugerida | Notas |
|--------|---------------|-------|
| Seguir con sintético mejorado | B (rápido) | Pitch/RPM, ruido banda, sin licencias |
| WAV del trainset MSTS | C+/D | Resolver paths desde `.eng`; `rodio`/`kira` decode |
| `.sms` de MSTS | D+ | OR `SoundManagement.cs`; alternativa OpenBVE [`SmsParser.cs`](../../OpenBVE/source/Plugins/Train.MsTs/Sound/SmsParser.cs) — ver [`OPENBVE_REFERENCE.md`](OPENBVE_REFERENCE.md) |
| Pack libre (CC0) | Opcional | Bocina/motor genéricos si no hay Content |

---

## 5. Fases del roadmap jugable

Leyenda: ✅ hecho · 🔶 parcial · 🔲 planificado

### Resumen ejecutivo

| Fase | Nombre | Estado | Entregable visible |
|------|--------|--------|-------------------|
| **0** | Viewer mundo + replay | ✅ | Mundo MSTS + tren desde CSV |
| **A** | Live link sim ↔ Bevy | ✅ | `--live`, conducir en 3D |
| **B** | Conducción pulida + audio + juego | ✅ | Sonido, señales, paradas en HUD |
| **C** | Cabina (interior / panel) | 🔶 | Panel CAB (C3); cabina 3D parcial — [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md) |
| **D** | Assets MSTS hardening | 🔶 | `.s` binario (básico), LOD, sync `--with-shapes` |
| **E** | Vía visual avanzada | 🔲 | Peralte, TSection, splines |
| **F** | Modo juego completo | 🔲 | Campaña, scoring visual, multi-tren live |
| **G** | Editor de ruta 3D | 🔲 | ROADMAP Fase 15 |

---

### Fase 0 — Viewer 3D + replay (issue #8, órdenes 1–11) ✅

**Objetivo:** Demostrar que el pipeline MSTS → Bevy funciona sin acoplar el sim.

**Hecho en:** `openrailsrs-viewer3d` (ver checklist en [`OPEN_RAILS_VIEWER_3D.md`](OPEN_RAILS_VIEWER_3D.md)).

**Assets necesarios:** carpeta de ruta con `TERRAIN/`, `WORLD/`, `SHAPES/`, `TEXTURES/` (demo: `examples/smoke/routes/test`).

**No incluye:** sim en vivo, cabina, sonido real, shapes binarios.

---

### Fase A — Enlace sim en vivo ✅

**Objetivo:** Misma física que `cab`/`sim`, con el tren moviéndose en la ventana 3D.

**Código:**

- `openrailsrs-sim/src/live_drive.rs` — `LiveDriveSession`
- `openrailsrs-viewer3d/src/live.rs` — input W/S/Space, spawn/update consist
- `main.rs` — flag `--live`

**Criterios de hecho:**

- [x] `step_realtime` avanza `TrainSimState` sin escribir CSV
- [x] Posición en grafo (`edge_id`, `pos_on_edge_m`) → transform Bevy
- [x] HUD: velocidad, throttle, freno, límite, tiempo
- [x] Follow camera (`T`) con tren live
- [x] Test unitario `live_session_advances_time_on_smoke_scenario`

**Limitaciones conocidas:**

- Controles de driver solo en cámara **orbit** (en fly, WASD mueven la cámara).
- Sin parada automática en plataforma (dwell); el jugador frena manualmente.
- Scoring simplificado en HUD (no `openrailsrs-game` post-hoc completo).
- Un tren (`[train]` del escenario).
- Chiltern con stubs del repo → cubos; smoke o Content OR → meshes.

---

### Fase B — Conducción pulida, audio y reglas de juego ✅

**Objetivo:** Experiencia “actividad jugable” en exterior 3D, no solo sandbox físico.

| Entrega | Descripción | Estado |
|---------|-------------|--------|
| **B1 Audio en viewer** | `AudioEngine` en `--live` (velocidad, freno, bocina `H`) | ✅ `live.rs` |
| **B2 Regiones de sonido** | `[[sound_regions]]` + `RegionTracker` compartido | ✅ `openrailsrs-scenarios/src/sound_regions.rs` |
| **B3 Señales en runtime** | `signal_runtime` + `evaluate_signals` / `clear_after_s` → marcadores 3D | ✅ `live_drive.rs`, `signals.rs` |
| **B4 HUD de juego** | Próxima parada, penalización, overspeed, destino | ✅ `hud.rs` + `LiveGameplay` (sin depender de `openrailsrs-game`) |
| **B5 Input** | W/S/Space en orbit; `H` bocina | ✅ |
| **B6 Calibración** | Tests smoke (`live_*`, `sound_regions`); ejemplo `[[sound_regions]]` en smoke | ✅ |

**Código:** `live_drive.rs` (`LiveGameplay`, señales), `live.rs` (audio), `signals::update_live_signal_markers`, `hud::build_hud_content_live`.

**Uso:**

```bash
cargo run -p openrailsrs-viewer3d -- --live examples/smoke/scenario.toml
# Orbit: W/S throttle/freno, Space emergencia, H bocina, +/- velocidad sim
```

**Assets de sonido:** sintético (`openrailsrs-audio`); sin WAV de ruta.

**Limitaciones:** sin enforcement de parada en rojo (solo aspectos visibles + límite caution); sin gamepad; Chiltern externo sigue siendo cubos sin content OR.

**Depende de:** Fase A ✅.

---

### Fase C — Cabina 🔶

**Objetivo:** Vista de conductor (inmersión), no solo cámara exterior.

Open Rails usa `cabview3d/` (meshes + texturas `.ace`) y a veces `cabview` 2D. **Estado detallado y próximos pasos:** [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md).

| Enfoque | Descripción | Estado |
|---------|-------------|--------|
| **C1 Cabview MSTS** | Parser CVF + meshes/texturas cabina 3D | 🔶 ver [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md) |
| **C2 Cabina genérica** | Cab 3D procedural o mesh único reutilizable | 🔲 |
| **C3 Panel híbrido** | Panel Bevy UI: velocidad, límite, THR/BRK, kN freno, RPM diesel, P boiler; tecla **C** | ✅ `cab_panel.rs`, `CabTelemetry` |

**C3 hecho:** panel inferior derecho en `--live`; follow **chase** al arrancar; cubos del tren más visibles.

**Datos ya disponibles en sim para el panel (C3):**

- `velocity_mps`, `throttle`, `brake`
- `diesel_rpm_*`, `diesel_apparent_*` (si se exportan al HUD)
- `boiler_pressure_bar` (vapor)
- Freno aire: telemetría en CSV (`brake_*`); exponer en live

**Assets imagen cabina (C1):** del trainset en OR Content, p. ej.  
`Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/CABVIEW3D/`

**Depende de:** Fase A; recomendable B1 (audio) para bocina en cabina.

**Estimación:** C3 ~3–6 semanas; C1 ~2–4 meses adicionales.

---

### Fase D — Assets MSTS (geometría y sync) 🔶

**Objetivo:** Que rutas y trenes reales se vean bien sin cubos fallback.

| Entrega | Descripción | Estado |
|---------|-------------|--------|
| **D1 Shapes binarios** | SIMISA zlib + volcado binario→ASCII (`shape_binary.rs`) | 🔶 básico |
| **D2 LOD / distancia** | `lod_level_for_distance` + `load_shape_from_path(..., Some(d))` | ✅ API |
| **D3 Animación shape** | Jerarquía animada en `.s` (puertas, bogies) | 🔲 |
| **D4 Sync ampliado** | `sync_chiltern_assets.py --with-shapes` + `WagonShape` en eng/wag | ✅ |
| **D5 ACE completo** | Mips, BGRA flag MSTS, menos fallbacks magenta | 🔲 |
| **D6 GLTF pipeline** (opcional) | Export/import para content propio sin MSTS | 🔲 |

**D4 uso (Chiltern con content OR):**

```bash
./scripts/sync_chiltern_assets.sh --with-shapes \
  "$HOME/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman"
```

Copia meshes a `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/`; el viewer busca ahí vía `TrainConsistScene::shape_search_dirs`.

**De dónde salen las imágenes:** instalación MSTS/OR del usuario; el repo documenta paths y parsers.

**Depende de:** Fase 0 ✅; mejora independiente de B/C.

---

### Fase E — Vía visual avanzada 🔲

**Objetivo:** Alinear mesh de vía con gradiente/peralte del sim y decoración OR.

| Entrega | Descripción |
|---------|-------------|
| **E1 Splines 3D** | `track.toml` + elevación → mesh continuo (no solo cilindros de grafo) |
| **E2 Peralte / cant** | Usar `grade_percent` y datos TDB si se importan |
| **E3 TSection / `.tdb`** | Perfiles de riel como OR `DynamicTrackPrimitive` |
| **E4 Acoplamiento sim–visual** | Multi-body: posición por vehículo en 3D (no solo cabeza del tren) |

**Datos:** `track.toml` (en repo); TDB completo vía `import-msts` (parcial hoy).

**Prerequisito de diseño:** [`TRACKVIEWER_STUDY.md`](TRACKVIEWER_STUDY.md) — geometría OR (`FindLocationInSection`, arcos) vs `TdbChord` actual.

**Depende de:** D (opcional); A para validar velocidad en pendiente.

---

### Fase F — Modo juego completo 🔲

**Objetivo:** Sesión jugable con objetivos, varios trenes, menú.

| Entrega | Descripción |
|---------|-------------|
| **F1** | `openrailsrs play3d` o menú en viewer: elegir escenario, overlay |
| **F2** | `LiveMultiSim` → varios consists en 3D |
| **F3** | Campaña / timetable visual (`examples/mitre_campaign`) |
| **F4** | Pausa, reinicio, resumen `outcome.toml` en pantalla |
| **F5** | Asumir señales / dificultad desde escenario |

**Depende de:** A, B (mínimo B4), D parcial para presentación.

---

### Fase G — Editor de ruta 3D (ROADMAP Fase 15) 🔲

**Objetivo:** Editar `track.toml` en el viewer (nodos, aristas, señales).

Independiente del sim jugable; puede reutilizar `track.rs` y gizmos Bevy.

---

## 6. Matriz: fase × fuente de assets

| Fase | Geometría 3D | Texturas | Sonido | Cabina visual |
|------|--------------|----------|--------|----------------|
| 0 | MSTS `SHAPES`+`.w` o smoke git | `.ace` TERRTEX/TEXTURES | — | — |
| A | Igual que 0 | Igual | — | — |
| B | Igual | Igual | Sintético (+ regiones TOML) | — |
| C | Igual | Panel UI; C1: `CABVIEW3D` | Bocina/motor | C3: HUD; C1: cabview OR |
| D | Binario `.s`, sync trainset | ACE mips | — | — |
| E | + splines vía | — | — | — |
| F | — | — | — | — |

---

## 7. Guía rápida: qué instalar para cada escenario

### Smoke (todo en repo, sin OR)

```bash
cargo run -p openrailsrs-viewer3d -- --live examples/smoke/scenario.toml
```

Meshes: `examples/smoke/routes/test/SHAPES/test.s`.

### Chiltern (física en repo, visual desde OR Content)

```bash
# Física diesel (ya en repo)
./scripts/sync_chiltern_assets.sh

# Sim / validación
cargo run -p openrailsrs-cli -- sim examples/chiltern/scenario.toml

# 3D con mundo real: usar assets desde el directorio de RUTA MSTS/OR
CHILTERN_ROUTE="$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cargo run -p openrailsrs-viewer3d -- --run-corridor --live --route-root "$CHILTERN_ROUTE" examples/chiltern/scenario.toml
# El grafo/física sale de examples/chiltern; tren y vía .tdb salen de --route-root.
# No carga WORLD/terreno/grilla/objetos estáticos.
# Ajustes opcionales: OPENRAILSRS_RUN_CORRIDOR_WIDTH_M=240 y OPENRAILSRS_RUN_CORRIDOR_RADIUS_M=3000.
```

**Sonido:** `openrailsrs cab` o viewer `--live` (sintético; requiere dispositivo de audio).

### SCE / otras rutas importadas

Mismo patrón: `import-msts` → `track.toml` en `examples/sce/`; assets 3D desde `Content/Demo Model 1/ROUTES/SCE/`.

---

## 8. Deuda técnica y mejoras transversales

| Tema | Estado | Fase |
|------|--------|------|
| `--route-root` para mezclar `scenario.toml` del repo + `WORLD/` de OR externo | ✅ | B/D |
| README Fase 23 marcada 🔲 pero órdenes 1–11 ✅ | Doc | Actualizar `README.md` |
| Señales enforced en `LiveDriveSession` | 🔲 | B |
| Freno residual Chiltern al arrancar en live | 🔲 | Afinar init como `ScriptedDriver` |
| CI: tests viewer sin GPU | 🔶 | `app_smoke` unit-style |
| Windows/macOS Bevy | 🔲 | features winit |

---

## 8.1 Brechas de fidelidad visual (priorizadas)

Evaluación del viewer 3D actual respecto a un simulador ferroviario visualmente realista.

| # | Brecha | Impacto visual | Esfuerzo | Estado | Archivo clave |
|---|--------|---------------|----------|--------|-------|---------------|
| 0 | **Escenario `unlit`** (no recibe sol ni sombras; brillo a base de hacks) | Alto | Medio | ✅ Camino lit estilo OR **por defecto** (opt-out: `OPENRAILSRS_UNLIT_SCENERY=1`) | `shapes.rs:or_lighting_enabled` |
| 1 | **Sombras** | Alto | Bajo | ✅ `shadows_enabled: true` + 3 cascadas | `scene.rs` |
| 2 | **Shapes `.s` binarios** → fallback cubo magenta | Alto | Medio | Parser básico (`shape_binary.rs`), heurísticas frágiles | `shape_binary.rs`, `shapes.rs` |
| 3 | **Cabina 3D interior** | Alto | Alto | 🔶 Parcial (`cab_view.rs`); ver [`CABVIEW3D_ROADMAP.md`](CABVIEW3D_ROADMAP.md) | `cab_view.rs` |
| 4 | **Animación de shapes** (puertas, bogies, pantógrafos) | Medio | Alto | Todo estático | `shapes.rs` |
| 5 | **ACE mipmaps + alpha** (solo mip 0, BGRA parcial) | Medio | Bajo | Shimmer a distancia, alpha recortado | `shapes.rs:ace_to_image`, `openrailsrs-ace` |
| 6 | **Audio real** (.wav/.sms) | Medio | Medio | Tonos sintéticos | `openrailsrs-audio` |
| 7 | **Vía visual avanzada** (TSection, splines, peralte) | Medio | Alto | Durmientes procedurales sin perfil | `dyntrack.rs` |
| 8 | **Posición multi-body** por vehículo | Bajo | Medio | Offset estático desde cabeza | `live.rs`, `train.rs` |
| 9 | **Efectos atmosféricos** (niebla, día/noche, humo/vapor) | Medio | Medio | Solo cielo dome + lluvia toggle | `sky.rs`, `precipitation.rs` |
| 10 | **Pipeline de contenido** (GLTF/OBJ import) | Bajo | Bajo | Copia manual de assets OR | scripts/ |

### Por qué todavía no se ve como Open Rails (análisis 2026-06)

El motivo principal **no** es geométrico (la vía/escenario están bien posicionados) sino el
**modelo de iluminación**. El viewer arrastra una pila de compensaciones contradictorias:

1. La cámara usa **sol físico brillante** (`DirectionalLight` 75 000 lux) + **exposición oscura**
   (`Exposure::SUNLIGHT`) + **`Tonemapping::None`** (sin curva fílmica que reencuadre el HDR).
2. Pero **todo el escenario se dibuja `unlit`** (`scenery_shape_material` forzaba `unlit: true`),
   así que ignora el sol por completo → superficies **planas, sin volumen y sin recibir sombras**.
3. Para que el `unlit` no quede negro se recupera brillo "a mano":
   `brighten_dark_ace_rgba`, boost de albedo ×4 (`SCENERY_TEXTURE_ALBEDO_BOOST`) y `emissive`.

OR, en cambio, **ilumina el mundo** (sol + ambiente) con su `SceneryShader` y tone-mapea. Por eso
sus edificios/trenes tienen sombreado por orientación al sol y sombras proyectadas, y los nuestros
no, aunque las texturas sean las mismas.

**Arreglo (ahora el camino por defecto):** el render *lit* estilo OR es el predeterminado:

- Materiales de escenario `unlit: false` → el sol los sombrea y **reciben sombras** (brecha #1 ya
  proyecta; ahora también se reciben sobre objetos, no solo el suelo).
- Albedo **neutral** (sin boost ×4 ni `brighten_dark_ace`): la luz aporta el brillo, no un hack.
- Sin `emissive` de relleno (evita que texturas noche/señal se autoiluminen bajo luz real).
- Consistente con la cámara física (`Exposure::SUNLIGHT` + sol 75 klux + relleno ambiente 15 klux).

```bash
# Por defecto (lit, estilo OR):
cargo run -p openrailsrs-viewer3d -- --live examples/smoke/scenario.toml
# Volver al render fijo (legacy unlit), p. ej. para comparar:
OPENRAILSRS_UNLIT_SCENERY=1 cargo run -p openrailsrs-viewer3d -- --live examples/smoke/scenario.toml
```

Pendiente (mejoras incrementales, requieren validación visual): afinar exposición/tonemap
(p. ej. `Tonemapping::TonyMcMapface`) y propagar el camino lit al material rodante
(`rolling_stock.rs`) y al terreno (`terrain.wgsl`, shader propio que aún no recibe sombras).

### Plan de implementación: sombras + shapes binarios

#### Sombras (prioridad #1, esfuerzo bajo)

**Objetivo:** habilitar `DirectionalLight` shadows en `scene.rs` para objetos con `StandardMaterial`.

**Cambios:**
1. `scene.rs`: `shadows_enabled: true` + `CascadeShadowConfigBuilder` (3 cascadas: 10, 50, 200 m).
2. Ajustar `illuminance` si las sombras quedan demasiado duras.

**Fuera de alcance (por ahora):**
- `TerrainMaterial` (shader WGSL custom) no recibe sombras — requiere integrar shadow sampling en `terrain.wgsl`.
- Objetos con `StandardMaterial` (edificios, trenes, árboles, señales) sí proyectan y reciben sombras automáticamente.
- Ground plane (fallback sin terreno) recibe sombras automáticamente.

**Archivos:** `crates/openrailsrs-viewer3d/src/scene.rs`

**Verificación:** abrir `examples/smoke` con terreno; objetos proyectan sombra visible sobre el plano y terreno.

#### Shapes binarios (prioridad #2, esfuerzo medio)

**Estado:** el parser ya lee shapes `.s` binarios comprimidos (`SIMISA@F` + `JINX0s1b`) con la tabla de tokens alineada a Open Rails y fixtures reales Chiltern. Extrae buffers, texturas, shaders, matrices, `prim_states`, vertices reales, LODs, primitivas y triángulos.

**Brecha restante:** el camino interno todavía convierte binario a S-expression sintética antes del parser tipado. Para paridad más fuerte con Open Rails conviene reemplazarlo por un `BinaryBlockReader` estructural, completar flags/colores de `Vertex` y mapear flags reales de material para alpha/z-buffer.

**Plan detallado:** ver `docs/MSTS_SHAPE_BINARY_PARSER.md`.

**Archivos:** `crates/openrailsrs-formats/src/shape_binary.rs`, `crates/openrailsrs-formats/src/typed/shape.rs`, `crates/openrailsrs-viewer3d/src/shapes.rs`

---

## 9. Referencias de código Open Rails (lectura)

Clon shallow de OR (`RunActivity/Viewer3D/`):

| Tema | Archivo OR |
|------|------------|
| Terreno | `Terrain.cs` |
| Shapes / LOD | `Shapes.cs` |
| Escenary `.w` | `Scenery.cs` |
| Trenes | `Trains.cs`, `MSTSWagonViewer.cs` |
| Cabina | `CabView.cs`, grupo `RenderPrimitiveGroup::Cab` |
| Sonido | `SoundManagement.cs` (no portado) |

---

## 10. Historial de este documento

| Fecha | Cambio |
|-------|--------|
| 2026-05 | Creación: fases A–G, inventario código, fuentes imagen/sonido, post Fase A (`--live`) |

Cuando se complete una fase, actualizar la columna **Estado** en §5 y marcar criterios en la subsección correspondiente.

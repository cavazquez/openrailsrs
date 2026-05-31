# Chiltern — validación OR end-to-end

Escenario importado desde la ruta MSTS **Chiltern** (Open Rails 1.6.1) con topología real (~28k nodos).

**Guía completa** (Wine, instalación OR, captura baseline, sim): [`docs/CHILTERN_OR_SETUP.md`](../../docs/CHILTERN_OR_SETUP.md).

| Campo | Valor |
|-------|--------|
| Ruta MSTS | `~/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern` |
| Actividad | `RS_Let's go to Birmingham` |
| Baseline OR | `../baselines/chiltern_birmingham/or_evaluation_speed.csv` (**136 s** sim, 10:00→10:02:16) |
| Duración sim | 136 s |
| Señales | `scenario.overlay.toml` → `assume_signals_clear` (OR `AUTO_SIGNAL` / aspectos CLEAR en eval; OR-P7) |
| Física sim (default) | **Masa puntual** — ver [Modelo físico vs OR](../../docs/OR_PARITY_ROADMAP.md#modelo-físico-or-vs-openrailsrs-importante-para-baselines) |
| Consist | DMBSA + 6 Pullman + DMBSH (**8 vehículos**; no es un solo loco) |

## Modelo físico vs baseline OR

Open Rails simula los **8 coches acoplados** (multi-cuerpo nativo). Por defecto openrailsrs usa **masa puntual** (`multi_body = false`): una velocidad, Davis sumado, mismos cilindros de freno por vehículo pero sin holgura ni oleadas de arranque.

Los RMS de `compare-or` (p. ej. ~0.39 m/s en 136 s) comparan por tanto **masa puntual vs OR multi-cuerpo**. En crucero suele encajar; en arranque/frenada la diferencia de modelo puede compensarse con otros ajustes.

Para acercarse a OR:

```bash
# multi_body = true; time_step = 1.0 (sub-pasos acoplador ≤0.05 s internos)
openrailsrs sim scenario_multi_body.toml --driver driver_or.csv
cargo test -p openrailsrs-cli --test chiltern_multi_body
```

Detalle y plan de revisión de experimentos: [`docs/OR_PARITY_ROADMAP.md`](../../docs/OR_PARITY_ROADMAP.md).

---

```bash
CHILTERN="/path/to/Chiltern/ROUTES/Chiltern"
cargo run -p openrailsrs-cli -- import-msts "$CHILTERN" \
  --out-dir examples/chiltern \
  --activity "$CHILTERN/ACTIVITIES/RS_Let's go to Birmingham.act"
```

El import escribe `start=n3`, `start_offset_m` (~305.6 m desde PAT+`.srv`), destino lejano y `[[route.switches]]` desde el PAT.

### Coordenadas de nodo (`x_m` / `y_m`) sin tocar la topología

El `track.toml` del repo trae la topología y los experimentos de sim (`chiltern_brake_coast`, compare-or, etc.). Un **re-import completo** regenera nodos/aristas y puede romper esos tests.

Para añadir o refrescar **posiciones MSTS** en los nodos existentes (desde `TrVectorSection` + propagación por `TrPins`):

```bash
CHILTERN="$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
cargo run -p openrailsrs-cli -- import-msts "$CHILTERN" \
  --out-dir examples/chiltern \
  --patch-coords
```

Escribe `x_m`/`y_m` en nodos cuyo `id` coincide con un import fresco (`n*`, `anon*`). No modifica `scenario.toml` ni las aristas.

Comprobar que hay coords:

```bash
rg -c 'x_m' examples/chiltern/track.toml   # esperado: miles (p. ej. ~8370)
```

Usa **re-import completo** solo si quieres regenerar todo el grafo desde cero (y revisar tests/overlay después).

**Speed posts:** el import lee `Chiltern.tit` junto al `.tdb` y baja `speed_limit_kmh` en los edges que tienen un `SpeedPostItem` referenciado (~3600 posts en Chiltern). Eso no sustituye el override manual del tramo Birmingham (`e10771`–`e10777` → 50 mph en `scenario.overlay.toml`): OR aplica el post **en sentido de marcha** a lo largo del PAT, mientras nosotros (por ahora) capamos solo el vector que referencia el post. En la práctica el Pullman no supera 50 mph ahí y las métricas casi no cambian.

Tras el import se fusiona **`scenario.overlay.toml`** (duración eval, consist Pullman, `[validate]`, señales, `edge_speed_limits`). Edita ese overlay, no `scenario.toml` a mano.

## Sync consist Pullman

Los `.eng`/`.wag` del repo son **física simplificada** (sin cab/C#) generados desde MSTS:

```bash
# Solo física (eng/wag simplificados, sin meshes)
./scripts/sync_chiltern_assets.sh

# Física + WagonShape + SHAPES/TEXTURES para viewer3d --live
./scripts/sync_chiltern_assets.sh --with-shapes
```

Fuente: `Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/`. En muchas instalaciones MSTS los `.s` y `.ace` están en la **raíz del trainset** (no en subcarpetas); el script los copia a `trains/RF_Blue_Pullman/SHAPES` y `TEXTURES`.

Rutas distintas (sustituir por tus paths reales; no uses `...` como placeholder):

```bash
python3 scripts/sync_chiltern_assets.py \
  "$HOME/Documentos/Open Rails/Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman" \
  --with-shapes \
  --route-content "$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
```

- **DMBSA:** curvas ORTS + `DieselPowerTab` / `ThrottleRPMTab` + Davis (`ChangeUpRPMpS` 50, `RateOfChangeUpRPMpSS` 10).
- **DMBSH:** stub MSTS sin tablas ORTS; al cargar el consist hereda curvas/RPM del DMBSA lead (OR-P13). OR 1.6.x **no** aplica `RunUpTimeToMaxForce` en motores ORTS.

## Live 3D (viewer)

Conductor en primera persona, paradas y penalización vía `scenario.overlay.toml`:

```bash
cargo run --release -p openrailsrs-viewer3d -- --live examples/chiltern/scenario.toml
```

Controles: **W/S** acelerar/frenar, **V** driver/chase, **P** pausa, **R** reinicio, **C** panel CAB, **Shift+P** lluvia.

Cabina 3D MSTS (`CABVIEW3D`): estado y roadmap en [`docs/CABVIEW3D_ROADMAP.md`](../../docs/CABVIEW3D_ROADMAP.md).

Paradas Birmingham (overlay): `n10778` (~95 s), destino `n10770` (136 s). Penalización: 8 pts/s tarde.

En rutas grandes usa **`--release`** (debug ≈ 4 FPS; release ≈ 60+ FPS).

### Modo `--track-dev` (vía `.tdb` procedural)

Laboratorio para validar el encadenado TDB (TrPins + secciones) sin terreno ni escenario. Requiere el `.tdb` live vía `OPENRAILSRS_MSTS_CONTENT`:

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
export OPENRAILSRS_TRACK_AUDIT=1   # métricas en consola (graph match, inter-node gap)

cargo run --release -p openrailsrs-viewer3d -- \
  --track-dev --live examples/chiltern/scenario.toml
```

En `--track-dev` **no se cargan** tiles `.w` (evita ~40k objetos y OOM). Radio de vía `.tdb` por defecto **1500 m** alrededor del tren; override: `OPENRAILSRS_TRACK_DEV_RADIUS_M=800`.

Por defecto solo corre el **audit** (sin mallas de riel ni meshes Pullman). Para dibujar rieles y consist completo:

```bash
export OPENRAILSRS_TRACK_DEV_RENDER=1
```

El audit también corre **antes de abrir Bevy** (verás `track-audit` en consola aunque la ventana falle por RAM).

Con `track.toml` parcheado (`--patch-coords`), el audit puede comparar acordes `.tdb` con el grafo importado (`graph match %`, no `n/a`). JSON opcional: `OPENRAILSRS_TRACK_AUDIT=/tmp/track-audit.json`.

## Escenario MSTS (WORLD / terreno, opcional)

Chiltern **no tiene** carpeta `TERRAIN/` como el demo `smoke`. En MSTS el relieve está en **`TILES/`** (`.t` + `_y.raw`, ~1600 tiles). El viewer carga **`TILES/*.t`** y **`TERRAIN/*.y`** en un radio de ~8 km desde el centro de la vía.

Los directorios `WORLD/`, `TILES/`, `TERRTEX/`, `TEXTURES/`, `SHAPES/` (ruta) y `TERRAIN/` están en `.gitignore` (~5 GB con rsync). No los subas al repo; cópialos en local con los comandos de abajo.

### WORLD (objetos `.w`) — recomendado

```bash
ROUTE="$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern"
DEST=examples/chiltern
mkdir -p "$DEST/WORLD"
rsync -a --info=progress2 "$ROUTE/WORLD/" "$DEST/WORLD/"
```

Tras tu `rsync`, deberías tener ~900 archivos `.w` en `examples/chiltern/WORLD/` (~60 MB). El viewer los dibuja en un radio de ~8 km; los **meshes `.s` reales** solo dentro de **~2 km** si el shape existe en disco.

### SHAPES de ruta + GLOBAL (meshes reales)

Los `.w` referencian shapes en la carpeta de la ruta y en `GLOBAL/SHAPES` del MSTS:

```bash
rsync -a "$ROUTE/SHAPES/" "$DEST/SHAPES/"
rsync -a "$ROUTE/TEXTURES/" "$DEST/TEXTURES/"   # si faltan .ace junto a shapes de ruta
```

Indica la raíz de contenido MSTS (carpeta `Content/`) para resolver `GLOBAL/SHAPES`:

```bash
export OPENRAILSRS_MSTS_CONTENT="$HOME/Documentos/Open Rails/Content"
```

### TILES + TERRTEX (terreno)

```bash
rsync -a "$ROUTE/TILES/" "$DEST/TILES/"      # ~1675 tiles, varios GB
rsync -a "$ROUTE/TERRTEX/" "$DEST/TERRTEX/"  # texturas de suelo (parches texturizados)
```

**No copies `TERRAIN/`** — esa carpeta no existe en esta ruta.

Sin `TILES/` el viewer usa **suelo plano** + vía compacta; con **TILES** ganas heightfield (y texturas si hay `TERRTEX/`).

## Flujo compare-or (evaluación 136 s)

Usa el binario **del repo** (`cargo install --path crates/openrailsrs-cli` desde la raíz, o `cargo run -p openrailsrs-cli -- …`).

**Importante:** las rutas `../baselines/…` y `scenario.toml` son relativas a `examples/chiltern`.

### Opción A — desde `examples/chiltern`

```bash
cd examples/chiltern

openrailsrs or-eval-driver ../baselines/chiltern_birmingham/or_evaluation_speed.csv \
  --out driver_or.csv \
  --brake-full-scale 27

openrailsrs sim scenario.toml --driver driver_or.csv
```

### Opción B — desde la raíz del repo

```bash
openrailsrs or-eval-driver examples/baselines/chiltern_birmingham/or_evaluation_speed.csv \
  --out examples/chiltern/driver_or.csv \
  --brake-full-scale 27

openrailsrs sim examples/chiltern/scenario.toml \
  --driver examples/chiltern/driver_or.csv
```

La sim valida automáticamente contra `[validate]` si el baseline existe (`overall: PASS`).

### Diagnóstico por fases

```bash
openrailsrs compare-or \
  examples/baselines/chiltern_birmingham/or_evaluation_speed.csv \
  examples/chiltern/run.csv \
  --phase-bounds 0,30,61,136 \
  --max-velocity-rms 0.5 --max-position-max 45
```

Métricas típicas (post `assume_signals_clear`, línea base OR-correct diesel OR-P6):

| Fase | Vel RMS | Pos max |
|------|---------|---------|
| 0–30 s | ~0.9 m/s | ~23 m |
| 30–61 s | ~0.2 m/s | ~19 m |
| 61–136 s | ~0.35 m/s | ~23 m |
| **Global 136 s** | **~0.48 m/s** | **~24 m** |

## Capturar baseline OR más largo

```bash
./scripts/capture_chiltern_birmingham_or.sh
./scripts/install_chiltern_birmingham_baseline.sh
# MIN_DURATION_S=180 ./scripts/install_chiltern_birmingham_baseline.sh
```

## CI

```bash
cargo test -p openrailsrs-cli --test chiltern_validate
cargo test -p openrailsrs-cli --test chiltern_startup_brake_phase
cargo test -p openrailsrs-cli --test chiltern_startup_diesel_audit -- --nocapture
cargo test -p openrailsrs-sim chiltern_path_reaches
```

`chiltern_validate` comprueba el reporte global **y** cada ventana en `phase_bounds` (`0, 30, 61, 136` s). Umbrales en `scenario.toml`: global vel RMS ≤ 0.48 m/s; fase arranque ≤ 0.95 m/s.

(Omitido si `examples/chiltern/track.toml` no está presente.)

## Dump de rendimiento OR (`openrails_dump.csv`)

Para fuerzas/adhesión internas (no compatible con `compare-or` v1 sin mapa de columnas):

- Archivo: `examples/baselines/chiltern_birmingham/openrails_dump.csv` (~9961 filas).
- Uso: calibración manual de motor/resistencia; ver [`docs/OR_TRACE_COMPARISON.md`](../../docs/OR_TRACE_COMPARISON.md).

## Experimento A — freno + costa (180 s)

Frenada fuerte y costa libre (OR-P6). Baseline: `examples/baselines/chiltern_brake_coast/README.md`.

```bash
cd examples/chiltern
openrailsrs sim scenario_brake_coast.toml --driver driver_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast

# Multi-cuerpo:
openrailsrs sim scenario_brake_coast_multi_body.toml --driver driver_brake_coast.csv
cargo test -p openrailsrs-cli --test chiltern_brake_coast_multi_body
```

Costa 115–180 s vs OR: masa puntual ~**0.07** m/s RMS; multi-cuerpo ~**0.16** m/s RMS (umbral 0.50).

## Experimento E — throttle 50 % (30 s)

Driver fijo (`driver_throttle50.csv`) para aislar calibración RPM vs curvas F(v):

```bash
cd examples/chiltern
openrailsrs sim scenario_throttle50.toml --driver driver_throttle50.csv
cargo test -p openrailsrs-cli --test chiltern_throttle50
```

Baseline OR: `examples/baselines/chiltern_throttle50/README.md`.

## Experimento B — throttle 100 % (120 s)

```bash
cd examples/chiltern
openrailsrs sim scenario_throttle100.toml --driver driver_throttle100.csv
cargo test -p openrailsrs-cli --test chiltern_fullthrottle

openrailsrs sim scenario_throttle100_multi_body.toml --driver driver_throttle100.csv
cargo test -p openrailsrs-cli --test chiltern_fullthrottle_multi_body
```

Baseline OR: `examples/baselines/chiltern_fullthrottle/README.md`. Arranque 0–30 s: masa puntual y multi-cuerpo ~**0.47** m/s RMS vs OR.

## Experimento C — throttle 75 % (60 s)

Crucero a notch fijo para calibrar equilibrio F(v) vs resistencia (OR-P1):

```bash
cd examples/chiltern
openrailsrs sim scenario_throttle75.toml --driver driver_throttle75.csv
cargo test -p openrailsrs-cli --test chiltern_throttle75
```

Baseline OR: `examples/baselines/chiltern_throttle75/README.md` (captura con `./scripts/capture_chiltern_throttle75_or.sh`).

## Arranque diesel (OR-P6)

Open Rails 1.6.x limita la tracción diesel-eléctrica con **RPM → `ReverseThrottleRPMTab` → apparent throttle → curvas**, no con `RunUpTimeToMaxForce` (parámetro MSTS ignorado en ORTS).

| Componente | Comportamiento en sim |
|------------|------------------------|
| `advance_rpm_orts` | Igual que `DieselEngine.Update` (sqrt + clamp 1–100 % de `ChangeUpRPMpS`, snap a `DemandedRPM`) |
| `throttleAcclerationFactor` | 1.0 (diesel-eléctrico sin caja; DMBSA) |
| Trail DMBSH | Hereda tablas del lead; sin τ MSTS |
| CSV extra | `diesel_rpm_{i}`, `diesel_apparent_{i}`, `diesel_f_n_{i}`, `diesel_run_up_{i}` (siempre 1.0 en ORTS) |

**Audit 0–40 s** (`driver_or.csv`, 80 % notch):

```bash
cargo test -p openrailsrs-cli --test chiltern_startup_diesel_audit -- --nocapture
# Escribe examples/chiltern/run_startup_diesel_audit.csv (gitignored)
```

Hallazgos (nominal 50/10): RPM y apparent coherentes con OR (p. ej. t=7 → rpm≈1000, app≈0.67); overspeed 0–40 s apunta a **fuerza/resistencia/freno**, no a integración RPM (test `advance_rpm_orts_matches_diesel_engine_cs_dmb_sa`).

**Investigación F₀+F₁ / creep / trail (post-audit):**

| t (s) | v\_or | v\_sim | F₀ (N) | F₁ (N) | F\_sum | Pcap₀/v (N) |
|------:|------:|-------:|-------:|-------:|-------:|--------------:|
| 5 | 0 | 0 | 0 | 0 | 0 | — (freno residual, RPM sube) |
| 7 | 0.27 | 0.40 | ~64k | ~111k | ~175k | ≫ F (cap no limita) |
| 13 | ~1.2 | ~1.6 | ~60k | ~105k | ~165k | idem |

- **`LocomotiveMaxRailOutputPowerW`:** DMBSA parsea **745 513 W** (`MaxPower` en `.eng`); a baja v el cap rail×apparent no acota (columna audit `Pcap0` ≫ F₀).
- **Creep con freno:** OR revoluciona con freno sin tracción; sim cortaba tracción solo con `throttle=0`. Fix: `BRAKE_TRACTION_CUTOFF` — RPM/apparent siguen con notch del driver, **F=0** si `brake>0` (v=0 en t=5).
- **Trail DMBSH:** el `.eng` es stub MSTS P/v, pero la evaluación OR encaja con **OR-P13** (heredar ORTS del lead), no con P/v legacy a notch completo (doble ~170 kN → overspeed; solo lead → ~5 m/s de déficit a t=40). Sin tocar `ChangeUpRPMpS` (50/10 del content).

Roadmap completo 3D (fases, assets MSTS, sonidos): [`docs/SIMULACION_3D_ROADMAP.md`](../../docs/SIMULACION_3D_ROADMAP.md).

**Modo live 3D** (Fase A — sim en ventana, sin CSV):

```bash
cargo run -p openrailsrs-viewer3d -- --live examples/chiltern/scenario.toml
# Orbit (F1): W/S throttle/freno, Space emergencia, T follow, +/- velocidad sim
```

**Barrido local** de `ChangeUpRPMpS` × `RateOfChangeUpRPMpSS` (solo diagnóstico; no commitear resultados):

```bash
./examples/chiltern/rpm_sweep.sh
# → rpm_sweep_results.csv (gitignored); nominal OR 50/10 es el correcto en .eng
```

Flag `orts_inherit_partial_run_up` en `[simulation]`: **deprecado**, sin efecto (experimento MSTS retirado).

Detalle: [`docs/OR_PARITY_ROADMAP.md`](../../docs/OR_PARITY_ROADMAP.md) · [`docs/fisica.html`](../../docs/fisica.html).

## Gaps cerrados vs OR

- Topología: alias TDB, switches salientes, placement PAT (Paddington Pfm 6); path ≥ 6 edges hasta destino.
- Coordenadas: `import-msts --patch-coords` → `x_m`/`y_m` en nodos para audit 3D y alineación grafo↔`.tdb`.
- Consist: RF_Blue_Pullman multi-vagón, Davis sumado por vehículo, dual motor (ORTS lead + trail heredado).
- Señales eval: `assume_signals_clear` alinea AUTO_SIGNAL con baseline OR.
- Diesel OR-P6: RPM/apparent alineados con `DieselEngine.cs`; freno residual ≤15 PSI sin boost cilindro 9/35.
- Posición/velocidad 136 s (masa puntual, OR-correct): RMS ~0.48 m/s, Δpos ~24 m.

## Límites conocidos

- Comparación **masa puntual vs OR multi-cuerpo** — ver sección arriba.
- Arranque 0–40 s: sim adelanta a OR (~0.77 m/s RMS) con parámetros RPM nominales; siguiente palanca: tracción dual-motor / cap P/v / creep con freno residual (no bajar `ChangeUpRPMpS` sin empeorar 40–65 s).
- Objetivo estricto **0.3 m/s / 25 m** aún pendiente (límites TDB 80 km/h vs OR `MAXSPEED` 90→50 mph).
- `RestrictedSpeedZones` de la actividad no se aplican al `track.toml` importado.
- Import TDB: aspecto inicial de señales = `Stop` salvo `FailedSignals`; usar overlay o re-import mejorado.

Baselines: [`../baselines/chiltern_birmingham/`](../baselines/chiltern_birmingham/).

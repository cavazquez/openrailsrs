# Chiltern — validación OR end-to-end

Escenario importado desde la ruta MSTS **Chiltern** (Open Rails 1.6.1) con topología real (~28k nodos).

**Guía completa** (Wine, instalación OR, captura baseline, sim): [`docs/CHILTERN_OR_SETUP.md`](../../docs/CHILTERN_OR_SETUP.md).

| Campo | Valor |
|-------|--------|
| Ruta MSTS | `~/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern` |
| Actividad | `RS_Let's go to Birmingham` |
| Baseline OR | `../baselines/chiltern_birmingham/or_evaluation_speed.csv` (**136 s** sim, 10:00→10:02:16) |
| Duración sim | 136 s |
| Señales | `assume_signals_clear = true` (OR `AUTO_SIGNAL` / aspectos CLEAR en eval) |
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

**Speed posts:** el import lee `Chiltern.tit` junto al `.tdb` y baja `speed_limit_kmh` en los edges que tienen un `SpeedPostItem` referenciado (~3600 posts en Chiltern). Eso no sustituye el override manual del tramo Birmingham (`e10771`–`e10777` → 50 mph en `scenario.overlay.toml`): OR aplica el post **en sentido de marcha** a lo largo del PAT, mientras nosotros (por ahora) capamos solo el vector que referencia el post. En la práctica el Pullman no supera 50 mph ahí y las métricas casi no cambian.

Tras el import se fusiona **`scenario.overlay.toml`** (duración eval, consist Pullman, `[validate]`, señales, `edge_speed_limits`). Edita ese overlay, no `scenario.toml` a mano.

## Sync consist Pullman

Los `.eng`/`.wag` del repo son **física simplificada** (sin cab/C#) generados desde MSTS:

```bash
./scripts/sync_chiltern_assets.sh
```

Fuente: `Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/`.

- **DMBSA:** curvas ORTS + `DieselPowerTab` / `ThrottleRPMTab` + Davis.
- **DMBSH:** diesel legacy OR (`MaxPower`/`MaxForce` kN, `RunUpTimeToMaxForce` 30 s) — el `.eng` OR no incluye tablas ORTS.

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

Métricas típicas (post `assume_signals_clear`):

| Fase | Vel RMS | Pos max |
|------|---------|---------|
| 0–30 s | ~0.2 m/s | ~15 m |
| 30–61 s | ~0.4 m/s | ~22 m |
| 61–136 s | ~0.5 m/s | ~23 m |

## Capturar baseline OR más largo

```bash
./scripts/capture_chiltern_birmingham_or.sh
./scripts/install_chiltern_birmingham_baseline.sh
# MIN_DURATION_S=180 ./scripts/install_chiltern_birmingham_baseline.sh
```

## CI

```bash
cargo test -p openrailsrs-cli --test chiltern_validate
cargo test -p openrailsrs-sim chiltern_path_reaches
```

`chiltern_validate` comprueba el reporte global **y** cada ventana en `phase_bounds` (`0, 61, 136` s por defecto). Para arranque fino: `--phase-bounds 0,30,61,136` (0–30 s suele ser ~0.63 m/s RMS).

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

## Gaps cerrados vs OR

- Topología: alias TDB, switches salientes, placement PAT (Paddington Pfm 6); path ≥ 6 edges hasta destino.
- Consist: RF_Blue_Pullman multi-vagón, Davis sumado por vehículo, dual motor (ORTS + legacy).
- Señales eval: `assume_signals_clear` alinea AUTO_SIGNAL con baseline OR.
- Posición/velocidad 0–136 s (masa puntual): RMS ~0.39 m/s, Δpos ~23 m.

## Límites conocidos

- Comparación **masa puntual vs OR multi-cuerpo** — ver sección arriba; Exp A/B pendientes de re-validar con `multi_body`.
- Objetivo estricto **0.3 m/s / 25 m** aún pendiente (RPM fino, DMBSH legacy, límites TDB 80 km/h vs OR `MAXSPEED` 90→50 mph).
- `RestrictedSpeedZones` de la actividad no se aplican al `track.toml` importado.
- Import TDB: aspecto inicial de señales = `Stop` salvo `FailedSignals`; usar overlay o re-import mejorado.

Baselines: [`../baselines/chiltern_birmingham/`](../baselines/chiltern_birmingham/).

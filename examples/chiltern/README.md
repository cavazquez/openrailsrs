# Chiltern — validación OR end-to-end

Escenario importado desde la ruta MSTS **Chiltern** (Open Rails 1.6.1) con topología real (~28k nodos).

| Campo | Valor |
|-------|--------|
| Ruta MSTS | `~/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern` |
| Actividad | `RS_Let's go to Birmingham` |
| Baseline OR | `../baselines/chiltern_birmingham/or_evaluation_speed.csv` (~61 s eval) |
| Duración sim | 65 s (ventana evaluación + margen) |

## Importar de nuevo

```bash
CHILTERN="/path/to/Chiltern/ROUTES/Chiltern"
openrailsrs import-msts "$CHILTERN" \
  --out-dir examples/chiltern \
  --activity "$CHILTERN/ACTIVITIES/RS_Let's go to Birmingham.act"
```

El import escribe `start=n3`, `start_offset_m` (~305.6 m desde PAT+`.srv`), destino lejano y `[[route.switches]]` desde el PAT.

## Sync consist Pullman

Los `.eng`/`.wag` del repo son **física simplificada** (sin cab/C#) generados desde MSTS:

```bash
./scripts/sync_chiltern_assets.sh
```

Fuente: `Content/Chiltern/TRAINS/TRAINSET/RF_Blue_Pullman/`.

## Flujo compare-or (evaluación 61 s)

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

La sim valida automáticamente contra `[validate]` si el baseline existe (`overall: PASS` con umbrales documentados).

## CI

```bash
cargo test -p openrailsrs-cli --test chiltern_validate
```

(Omitido si `examples/chiltern/track.toml` no está presente.)

## Gaps cerrados vs OR

- Topología: alias TDB, switches salientes, placement PAT (Paddington Pfm 6).
- Consist: RF_Blue_Pullman multi-vagón, longitudes de freno reales.
- Parser MSTS: unidades, UTF-16, `EngineData` en `.con`, vapor opcional en `.eng`.
- Posición a t=61: ~210 m vs OR ~205 m (Δ < 10 m).

## Límites conocidos

- Pullman en OR es **diesel** (`DieselPowerTab`), no vapor — `steam_step` no aplica.
- Velocidad RMS ~4 m/s vs OR: falta curva tractiva por notch y scripts (`Default.cs`).
- Umbrales estrictos 0.3 / 25 m de velocidad pendientes hasta modelar diesel OR completo.

Baselines: [`../baselines/chiltern_birmingham/`](../baselines/chiltern_birmingham/).

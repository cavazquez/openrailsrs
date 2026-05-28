# SCE — Scottish Capital Express (Demo Model 1)

Escenario importado desde la actividad MSTS **0930 Edinburgh-Glasgow Queen Street**
(Open Rails Demo Model 1, ruta SCE).

| Campo | Valor |
|-------|--------|
| Ruta MSTS | `Content/Demo Model 1/ROUTES/SCE` |
| Actividad | `MT_MT_0930 Edinburgh-Glasgow Queen Street` |
| Consist | Class 47 + 6 vagones Mk2 (push-pull) |
| Distancia OR | ~111 km (Edinburgh Waverley → Glasgow Q.S.) |
| Baseline OR | `../baselines/sce_glasgow/or_evaluation_speed.csv` (~100 s eval) |
| Duración sim | 100 s (ventana evaluación) |
| Física sim (default) | **Masa puntual** (Class 47 + 6 MK2 en `.con`; OR usa multi-cuerpo) |

Multi-cuerpo (`multi_body = true`, sub-pasos acoplador ≤0.05 s):

```bash
openrailsrs sim scenario_multi_body.toml --driver driver_or.csv
cargo test -p openrailsrs-cli --test sce_multi_body
```

| Modo | RMS velocidad 100 s vs OR |
|------|---------------------------|
| Masa puntual | ~0.30 m/s |
| Multi-cuerpo | ~**0.29** m/s (arranque 30–60 s ~0.44 m/s) |

Ver [`docs/OR_PARITY_ROADMAP.md`](../../docs/OR_PARITY_ROADMAP.md#modelo-físico-or-vs-openrailsrs-importante-para-baselines).

## Importar de nuevo

```bash
SCE="$HOME/Documentos/Open Rails/Content/Demo Model 1/ROUTES/SCE"

openrailsrs import-msts "$SCE" \
  --out-dir examples/sce \
  --activity "$SCE/ACTIVITIES/MT_MT_0930 Edinburgh-Glasgow Queen Street.act"
```

Tras el import se fusiona **`scenario.overlay.toml`** (start=n474, consist Class 47, etc.).
Editar ese overlay, no `scenario.toml` a mano.

**Nota**: el import asigna `start=n3` (sidings de Edimburgo). El overlay lo corrige a
`start="n474"` (inicio de la vía principal de 111 km hacia Glasgow).

## Vehículos

Los stubs de física en `trains/` se generaron desde los archivos MSTS originales:

| Archivo | Tipo | Masa | Parámetros clave |
|---------|------|------|-----------------|
| `MT_DD_CLASS_47_47706.eng` | Diesel | 118 674 kg | `ORTSMaxTractiveForceCurves` reales (7 notches) |
| `MT_DB_MK2_TSO_*.wag` (×4) | Vagón TSO | 34 000 kg | longitud 20.34 m |
| `MT_DB_MK2_FK_SC13415.wag` | Vagón FK | 33 000 kg | longitud 20.34 m |
| `MT_DB_MK2_BSO_SC9411.wag` | Vagón BSO | 33 000 kg | longitud 20.34 m |

## Flujo compare-or (evaluación 100 s)

```bash
cd examples/sce

openrailsrs or-eval-driver ../baselines/sce_glasgow/or_evaluation_speed.csv \
  --out driver_or.csv --scenario scenario.toml

openrailsrs sim scenario.toml --driver driver_or.csv
openrailsrs compare-or ../baselines/sce_glasgow/or_evaluation_speed.csv run.csv
```

### Resultados baseline (OR 1.6.1, Wine, Linux)

| Métrica | Valor | Umbral |
|---------|-------|--------|
| Velocity RMS | ~0.30 m/s | 0.35 m/s |
| Position max | ~53 m | 100 m |
| Estado | **PASS** | — |

## Gaps conocidos vs OR

- La vía principal `e1017` (111 km) tiene un solo segmento en el track.toml; los sidings
  de Edimburgo (n3→n4→n1, 9 km) son una rama separada no usada en la ruta principal.
- OR crashea a los ~100 s de juego bajo Wine (NullReferenceException en Dispose);
  el baseline cubre la fase de aceleración desde Edimburgo.

### Crucero diesel (Class 47 @ 27 % mando)

Corregido en sim: `DieselPowerTab` escala P/v como `P_idle + (P(RPM)-P_idle)×throttle`
por debajo del 50 % de mando, más Davis estimado por vagón cuando falta ORTSDavis.
En t≈80 s del baseline OR la velocidad coincide (~13.6 mph).

## Capturar nuevo baseline OR

Ver [`../baselines/sce_glasgow/README.md`](../baselines/sce_glasgow/README.md).

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
| Velocity RMS | ~3.7 m/s | 5.0 m/s |
| Position max | ~53 m | 100 m |
| Estado | **PASS** | — |

## Gaps conocidos vs OR

- La vía principal `e1017` (111 km) tiene un solo segmento en el track.toml; los sidings
  de Edimburgo (n3→n4→n1, 9 km) son una rama separada no usada en la ruta principal.
- OR corre el Activity en AUTO\_SIGNAL (autopiloto) con 27% throttle; nuestro sim usa
  el driver capturado de OR.
- La velocidad de crucero OR (~14 mph con 27% throttle) es inferior a la de nuestro
  sim (~16 mph con mismos inputs) — diferencia debida al modelo diesel OR (DieselPowerTab
  + adhesión adaptativa) vs nuestras curvas ORTSMaxTractiveForceCurves.
- OR crashea a los ~100 s de juego bajo Wine (NullReferenceException en Dispose);
  el baseline cubre la fase de aceleración desde Edimburgo.

## Capturar nuevo baseline OR

Ver [`../baselines/sce_glasgow/README.md`](../baselines/sce_glasgow/README.md).

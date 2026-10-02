# Paridad física con Open Rails

Complementa [`ROADMAP.md`](../ROADMAP.md). Baselines: `examples/baselines/` · escenarios: `examples/chiltern/`.

## Referencia fija y aceptación reproducible

Open Rails **1.6.1**, tag `1.6.1`, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`. La versión se confirma en el log
de captura Birmingham. `oracles/openrails-reference.toml` congela el commit,
los binarios de la instalación, los CSV originales y las órdenes de conducción.
Una actualización de dependencias no modifica esta referencia.

```bash
python3 scripts/run_oracles.py
python3 scripts/run_oracles.py --verify-only --source-root ../openrails
python3 scripts/run_oracles.py --verify-only --installation-root "/ruta/Open Rails"
```

`oracles/chiltern.toml` declara cuatro casos: Birmingham multi-cuerpo,
aceleración al 100 %, frenado/costa multi-cuerpo y regulador al 50 %.
El presupuesto global de velocidad RMS es 0,55 m/s para Birmingham y
0,75 m/s para los demás; el pico máximo es 2 m/s. Birmingham exige además
error máximo de odómetro ≤45 m. Los casos Explorer no verifican distancia:
su CSV original registra ese campo en cero. Los umbrales de regulador y
freno son específicos de cada captura; las fases verifican velocidad,
pues la presión de cilindro registrada por OR no equivale al mando de freno.

La suite rechaza umbrales inválidos, columnas requeridas ausentes, NaN,
tiempos fuera de orden y cobertura temporal inferior al 98 %. Escribe una
muestra inicial en t=0. Los segundos repetidos del log OR se normalizan
conservando la última observación y declarando cuántos se eliminaron;
los CSV originales permanecen intactos. Las fases deben cubrir toda la
referencia. Los informes y trazas nuevas se guardan en `tmp/oracles` y
nunca reemplazan los datos de referencia ni las salidas del usuario.

La partida [`chiltern_local`](../examples/chiltern_local/README.md) tiene
oráculos de servicio independientes: parada real ≤0,1 m/s dentro de ±10 m,
embarque con puertas abiertas, salida autorizada y puertas cerradas,
pasajeros y llegada terminal. Se prueba a 30 y 144 FPS. Esta aceptación
funcional no sustituye una captura OR completa del nuevo recorrido, que
todavía falta; tampoco certifica SIGSCR completo, retroceso ni paridad visual.
El caso regulador 75 % no se declara aprobado: aún falta su CSV OR.

Con el Content original disponible, este comando agrega la comparación
espacial de las tres estaciones contra los puntos PAT originales:

```bash
OPENRAILSRS_NATIVE_ROUTE="$CHILTERN_ROUTE" ./check.sh
```

La tolerancia es 3 m por la cuantización a coordenadas
de renderizado; esta prueba detecta errores de longitudes, sentido y curvas,
sin afirmar equivalencia visual ni física de la partida completa.

## Modelo (importante)

| | Open Rails | openrailsrs default | `multi_body = true` |
|---|---|---|---|
| Dinámica | Multi-coche + acopladores | **Masa puntual** | Masas + acopladores |
| Davis | Por coche | Agregado | Por vehículo |
| Baselines CSV OR | Multi-cuerpo | Comparación mixta ⚠️ | Más comparable |

RMS publicados (Chiltern ~0.39 m/s, etc.) calibran a menudo **puntual vs OR multi-cuerpo**.

## Estado olas (resumen)

| Ola | Fases | Objetivo | Estado típico |
|-----|-------|----------|---------------|
| 1 | OR-P1…P3 | Diesel thr aparente, CN, run-up | ✅ / 🔶 |
| 2 | OR-P4…P6 | Multi-cuerpo, Davis/veh, frenos | 🔶 (multi_body estable Chiltern) |
| 3 | OR-P7…P8 | Señales + driver sin assume-clear | 🔶 |
| 4 | OR-P9+ | Gearbox, dinámico, vapor | 🔲 / parcial |

Detalle de cada OR-P*: historial en commits anteriores del repo y código en `openrailsrs-sim`.

## Calibración rápida

```bash
cargo run -p openrailsrs-cli -- sim examples/chiltern/scenario.toml
# Comparar: openrailsrs compare-or …  → OR_TRACE_COMPARISON.md
```

| Escenario | Notas |
|-----------|--------|
| Birmingham ~136 s | `assume_signals_clear`; RMS v ~0.39 |
| `scenario_multi_body.toml` | Acopladores; `time_step=1.0` |
| SCE Glasgow | Umbral ≤1 m/s |

Referencias OR: `MSTSDieselLocomotive.cs`, `DieselEngine.cs`, `TrainCar.cs`. Audit ENG/WAG: [`FORMATS.md`](FORMATS.md).

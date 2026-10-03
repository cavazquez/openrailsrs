# Paridad con Open Rails

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
funcional se complementa ahora con la [captura OR completa](../examples/baselines/chiltern_local/README.md)
del mismo recorrido: 15 003 muestras, ocho coches, 7054,788 m, tres tareas
nativas de estación aprobadas y reproducción exacta con las órdenes exportadas.
Todavía no certifica SIGSCR completo en Bevy, retroceso ni paridad visual.
El caso regulador 75 % no se declara aprobado: aún falta su CSV OR.

```bash
python3 scripts/verify_chiltern_service_capture.py
python3 scripts/run_oracles.py --suite service --out-dir tmp/service-parity
```

El nuevo objetivo de `oracles/chiltern-service.toml` es diagnóstico y devuelve
**FAIL** con RMS 7,5153 m/s, pico 16,6450 m/s y odómetro máximo 3029,15 m,
frente a los presupuestos existentes de 0,75 m/s, 2 m/s y 45 m. Utiliza
columnas y unidades explícitas en m/s, controles aplicados en su tick y
cobertura completa. Se aíslan las órdenes del freno/regulador mediante un
escenario de replay sin intervenciones automáticas del runner antiguo.
`check.sh` verifica la integridad y contratos de la referencia completa,
además de los cuatro casos físicos ya aceptados y el servicio Bevy.

La siguiente corrección se concentra en el poste de 35 mph aplicado a todo
el vector 96, pendientes nativas ausentes en la física del mapa y siete
frenos EP originales frente a la clasificación simplificada del perfil
convertido. El servicio usa ahora una demanda de freno EP en [0,1], con
escalas de conductor/cilindro ambas de 45 PSI. Las capturas anteriores
conservan su propio convenio de presión. Los objetivos de aceptación de
velocidad/distancia y las referencias anteriores permanecen iguales.

Con el Content original disponible, este comando agrega la comparación
espacial de las tres estaciones contra los extremos de andén del TDB original:

```bash
OPENRAILSRS_NATIVE_ROUTE="$CHILTERN_ROUTE" ./check.sh
```

La tolerancia es 3 m por la cuantización a coordenadas
de renderizado; se comprueba además el vector anfitrión y la distancia exacta
registrada en `TrItemSData`. Esta prueba detecta errores de longitudes,
sentido, curvas y confusiones entre un punto PAT y un andén,
sin afirmar equivalencia visual ni física de la partida completa.

## Escenario y oráculos gráficos

El streaming del visor conserva todos los objetos CPU de cada tile leído y
activa su geometría al entrar en la ventana móvil, con 64 m de preparación.
La identidad usa tile y ordinal del archivo, incluso si falta UID. La geometría
GPU distante se libera antes que el tile CPU; al volver se activa nuevamente.
La cola guarda poses relativas al foco y aplica el origen vigente al enviar
las entidades, incluyendo señales animadas, bosques, decals, agua y tráfico.
La simulación espera la carga inicial y continúa durante las cargas posteriores.
El primer LOD comparte los buffers de la malla inicial cuando corresponde;
las mallas temporales y los ACE decodificados se liberan al terminar la conversión.

La regresión integrada mueve la ventana por tiles ya leídos, cambia el origen
entre construcción y envío de mallas, comprueba ausencia de duplicados y vuelve
al inicio después de liberar el GPU. Con los assets originales se verifican
además dos capturas tras **viajar desde Northolt Park**, a 3,7 km y al completar
el servicio en West Ruislip; se exige cero shapes cercanos sin activar y RSS
máximo de 6 GiB. Se conservan PNG, logs y metadatos en `tmp/viewer-streaming`.
Este control comprueba continuidad y memoria, sin certificar paridad de píxeles
ni ampliar las tolerancias físicas.

```bash
python3 scripts/check_viewer_streaming.py --route-root "$CHILTERN_ROUTE" --software
```

La composición de tres modelos residenciales de Chiltern se compara con
`ShapeFile` de la DLL original 1.6.1: `housesemi1.s`, `housesemirow1.s` y
`Doc_30sDetachedHouse1.s`. `oracles/chiltern-scenery.json` conserva hashes de
modelos y lector, primitivas, materiales y bandas LOD. Se exige igualdad de
conteos e identidades y una tolerancia de **1 mm** en distancias LOD.

```bash
python3 scripts/run_scenery_oracle.py --route-root "$CHILTERN_ROUTE"
xvfb-run -a bash scripts/visual_regression_instancing.sh
```

El segundo oráculo dibuja realmente las mismas geometrías mediante instancias
GPU y mallas individuales. Incluye una caja indexada, una tarjeta abierta
vista por detrás y una barra no indexada con mallas distintas. Una entidad
ajena y un grupo trasladado/rotado fuera del frustum detectan dependencia
del orden de uniforms, caras descartadas y pérdida del AABB agregado.
El solapamiento de siluetas debe ser ≥**98 %** en cada región; la corrección
da 100 % en las tres geometrías.
No compara iluminación ni sustituye una referencia de píxeles nativa.

La [vista nativa de Northolt](fixtures/visual/or_reference/chiltern_local/README.md)
congela cámara, hora, distancia de dibujo, matrices y captura de OR 1.6.1.
El lector y el consumidor gráfico son clientes de las DLL originales; no
modifican shaders ni contenido. La vista permite revisar casas, caminos y
terreno, pero todavía quedan diferencias de iluminación, sombras y detalle a
distancia. La paridad visual completa no se declara aprobada.

Las correcciones del visor conservan las texturas diurnas antes que los
fallbacks de carpetas, los mipmaps ACE originales y el filtrado anisotrópico.
La niebla de día despejado usa 20 km, independientemente del radio de carga.
El grafo lógico no se superpone a las vías originales durante una partida.
La carga presta el caché ACE por lote, sin copiar todas las texturas por frame.
El domo del cielo queda excluido de las sombras: no debe oscurecer las cascadas
que siguen a la cámara. Se mantienen los objetos físicos como emisores de sombra.
El AABB agregado se conserva mediante `NoAutoAabb`; la visibilidad usa la
identidad principal y el LOD considera la colocación más cercana del grupo.
La cabina conserva la exposición exterior del paisaje. Los Transfer se
recortan a sus límites UV, se construyen sobre la grilla local de terreno y
mezclan con profundidad de solo lectura. La captura nativa de West Ruislip
sirvió para identificar el patrón de triángulos causado por esas superficies.

Los brazos de señales usan `SemaphorePos`, `SemaphoreInfo` y la compatibilidad
de dos claves de OR: se colocan al iniciar, se desplazan una vez al cambiar
el aspecto y permanecen inmóviles con aspecto estable. Se comparte el aspecto
con las luces; esto no amplía el alcance actual del intérprete SIGSCR.

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

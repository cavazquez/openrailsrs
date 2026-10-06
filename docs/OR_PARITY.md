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

El servicio completo de `oracles/chiltern-service.toml` pasa desde la revisión
del 5 de octubre de 2026: RMS **0,2081 m/s**, pico **1,0921 m/s** y diferencia
máxima de odómetro **39,61 m**, con cobertura del 100 %. Los presupuestos siguen
siendo 0,75 m/s, 2 m/s y 45 m; las cinco fases también cumplen su RMS ≤1,10 m/s.
La referencia de Open Rails 1.6.1, sus 15003 muestras, los controles y sus hashes
permanecen intactos. `check.sh` ejecuta ahora los cuatro ensayos cortos y este
servicio completo, además de la partida jugable.

El replay usa las pendientes/límites nativos del corredor y una extracción
numérica independiente de la formación original: 440906,4 kg, 166,164 m,
gobernadores distintos para ambos motores, escalones reales del regulador,
corte de tracción por presión del cilindro, EP con carrera del pistón y zapatas,
patinaje Pacha, rodamientos con temperatura y conexiones rígidas declaradas.
No incluye los recursos gráficos descargados en Git. Procedencia, extracción y
condiciones del entorno: [fixtures físicos](../examples/chiltern_local/physics/README.md).

El cliente original avanzaba el simulador con el clima pausado. Se reproducen
esas entradas explícitamente, con factor base de adherencia 0,5, primer sector
resbaladizo de la semilla original y temperatura inicial medida; el ruido de
adherencia se representa por su media. El resultado certifica este ensayo y sus
presupuestos, no equivalencia universal de motores, frenado, clima o scripts.
La escala de demanda EP de este caso es 45 PSI. Los casos cortos mantienen su
convenio histórico y su material rodante, sin volver a capturar sus referencias.

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

### Perfiles nativos y servicio extendido (octubre de 2026)

La importación conserva carteles por posición y sentido y pendientes AX por
sección, incluidos los sentidos inversos. `chiltern_extended` aplica estos
perfiles durante un recorrido de seis estaciones/15,32 km. También se importan
el sistema EP/aire, la presión de plena fuerza y las tasas de aplicación y
liberación de ENG/WAG. El Pullman usa siete vehículos EP y un motor de cola de
aire; no se infiere el sistema solo de la categoría locomotora/remolque. Estos
datos mejoran el modelo simplificado de cilindros; todavía no reproducen todas
las válvulas, depósitos y demanda de presión del original. El regulador nativo
permite sobrepasar un límite de vía;
solo los escenarios con `legacy_power_cap` conservan el resguardo automático.
Los perfiles no se insertan retroactivamente en los escenarios históricos de los
oráculos congelados.

### Señales y clima del servicio extendido

`chiltern_extended/track.toml` contiene los programas originales de `SIGSCR`,
la función declarada por `SIGCFG` y la dirección del `SignalItem` TDB. El
intérprete admite las condiciones y funciones normal/distante utilizadas por
Chiltern y conserva ocho aspectos en las lámparas. La evaluación utiliza bloques
dirigidos y la ocupación longitudinal de jugador, tráfico y sección estacionada.
Errores o construcciones no soportadas rechazan el contenido o dan alto; una
orden manual de vía libre no anula la restricción del script. La regresión del
recorrido verifica seis paradas, ambos servicios AI y estados de alto/advertencia.
Se añaden reservas exclusivas del próximo bloque en recorridos fijos, incluyendo circulación opuesta y guardado de concesiones, y propagación de restricciones del despachador a los scripts anteriores. INFO conserva su representación sin crear autoridad. Esto no certifica el despachador general, todos los enlaces, bloqueos de desvíos, memorias ni scripts de Open Rails. [Alcance ampliado](SIGNALS_AND_CONTENT_SCOPE.md).

La nieve usa la visibilidad inicial nativa de 500 m y las variantes Snow del
Content disponibles según la estación del año. La precipitación, el vidrio barrido por limpiaparabrisas,
la cobertura superior y el mojado de instancias GPU se adaptan a Bevy. Son
mejoras visuales; no modifican la referencia física ni simulan hielo/adhesión.
El presupuesto nativo de subida de recursos a GPU es de 8 MiB por cuadro
(límite flexible de Bevy). El informe conserva el histograma de toda la sesión
y separa los máximos/tirones de carga inicial y partida para comparar mediciones.

Los cuatro ensayos cortos y el servicio completo pasan con sus tolerancias
originales. El viaje gráfico de seis estaciones verifica además continuidad de
escenario, señales y detenciones. Son comprobaciones distintas: la tolerancia
física del servicio nativo de tres estaciones no certifica todos los horarios.
La sección 33 de [las pruebas manuales](PLAYER_MANUAL_TESTS.md) y la última
ampliación de [QA](PLAYER_POLISH_QA.md) documentan las mejoras y sus evidencias.

## Escenario y oráculos gráficos

Se conserva la fidelidad del contenido y de la conducción junto con las mejoras
de Bevy que benefician la partida. Iluminación física, tonemapping, instancias
GPU, origen flotante y límite exterior sobre el terreno son decisiones
intencionales. Los errores de posición, carga o movimiento se corrigen; no se
copian limitaciones del motor original como requisito de paridad.

El streaming del visor conserva todos los objetos CPU de cada tile leído y
activa su geometría al entrar en la ventana móvil, con 64 m de preparación.
El encuadre inicial y la alineación del grafo usan los objetos de la ventana
de inicio; los objetos CPU lejanos no desplazan la cámara ni el terreno.
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

La revisión de verano registra 3708,84 m y 7 tiles GPU a mitad de recorrido,
y 7054,34 m, 6 tiles GPU y las tres paradas servidas al terminar. Ambos puntos
tienen cero shapes cercanos pendientes. El pico RSS medido es 5606,4 MiB y
5689,2 MiB, respectivamente, con Vulkan lavapipe, vista desde la cabina a
1280×720, ventana de escenario de 2000 m y un único visor por prueba.
Estas cifras verifican el límite de memoria en esa configuración; no son
una medición de FPS de hardware.

```bash
python3 scripts/check_viewer_streaming.py --route-root "$CHILTERN_ROUTE" --software
```

Las vistas detenidas de Northolt Park, South Ruislip, West Ruislip y cabina
se capturan secuencialmente con `scripts/capture_route_views.py --with-cab`.
Incluyen pose de cámara, hora, dirección solar y cantidad de modelos compartidos
del tren. Complementan el viaje real; no lo reemplazan como prueba de streaming.

```bash
python3 scripts/capture_route_views.py --route-root "$CHILTERN_ROUTE" --software --with-cab
```

`oracles/solar-or161.json` contrasta ubicación MSTS y dirección del sol con
las DLL originales: 75 casos válidos y 15 discontinuidades de la proyección,
tres estaciones del año y cinco horarios. Exige error geográfico ≤1e−11 rad
y error L2 del vector solar <1e−5. Incluye los horarios de `SummerClear.env`
para la partida de verano a las 09:55 y los hashes de entradas y consumidor.
`check.sh` ejecuta esta comparación; las instrucciones de recaptura están en
[`tools/openrails-reference`](../tools/openrails-reference/README.md).
El visor evalúa la misma ecuación directamente cada segundo simulado;
OR interpola una tabla de muestras tomadas cada 20 minutos. Se mantiene
esa diferencia como mejora de Bevy,
sin declarar aprobada la iluminación ni la paridad completa de píxeles.

Las regresiones ECS de ruedas y bogies verifican pausa, reinicio, multiplicador
de tiempo, muestras por tren e inversión del coche sobre una curva. La fase
de rueda usa la distancia de presentación de la carrocería, no un reloj paralelo;
las partes comparten la shape original inmutable. Esto comprueba coherencia
de movimiento y memoria, sin ampliar la tolerancia física del servicio completo.

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

La referencia baja `northolt_low_exterior` (pitch −0,15, 160 m) expone huecos
bajo calles y jardines también en el render original. El visor mantiene la
cámara exterior de una partida 1,5 m sobre el RAW visible, respetando huecos y
ojos de cabina/pasajeros. Es una adaptación de cámara; no certifica el cierre
geométrico de las superficies originales ni cambia sus alturas.

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

## Oráculo de confort en curvas

`oracles/superelevation-or-1.6.1.json` congela diez resultados de la expresión original de `TrainCar.UpdateCurveSpeedLimit`, extraída del commit fijado y compilada en C# con aritmética `float`. La comparación Rust admite 0,00002 m/s; la captura inicial mide un error máximo de 0,000011826 m/s. Es validación aislada de la fórmula, no paridad de peralte generado, suspensión, avisos originales ni descarrilamiento. El diagnóstico de F8 utiliza geometría nativa, trocha y déficit de los coches. Los umbrales y capturas anteriores permanecen intactos.

## Segunda formación: Class 47 y seis coches Mk2

El ensayo nuevo conserva las DLL de OR 1.6.1 y su semilla. La actividad original
proviene de Demo Model 1; `FormationCapture.cs` estabiliza los frenos durante 20 s
y captura 250 s, con liberación, aceleración, deriva, frenado parcial, nuevo
arranque y frenado completo de servicio. No sustituye el baseline Pullman.

```bash
python3 scripts/run_oracles.py --suite class47 --out-dir tmp/class47-parity
```

Pasa con RMS **0,2382 m/s**, pico **1,9415 m/s** y diferencia máxima de odómetro
**25,79 m**, usando los mismos límites de **0,75 m/s / 2 m/s / 45 m** fijados antes
de comparar. Cubre las 5001 muestras y las seis fases. Manifiesto, controles y
traza están en `examples/baselines/class47`; el verificador rechaza cambios de
hash, cliente, versión o cobertura.

La extracción numérica conserva los valores finales de los Includes y los
bloques parciales de alimentación. El motor mantiene 450 RPM para alimentar los
coches sin producir tracción. El distribuidor UIC, la válvula relé, la carrera de
cilindros y las zapatas alimentan las presiones reales de cabina; el deslizamiento
Pacha aplica la fricción de rueda bloqueada. La tubería mide 5 bar liberada,
4,1 bar con demanda del 60 % y 3,5 bar con servicio completo. En ese último estado,
el cilindro de la locomotora alcanza 4,826 bar, distinto del de los coches.

Este pase certifica el ensayo. Los depósitos y su agotamiento, emergencia,
equipamiento de servicio rápido y protección de deslizamiento necesitan pruebas
adicionales. El mínimo de alimentación se usa con arranque caliente; falta probar
el ciclo completo del suministro auxiliar. La cabina original y Bevy coinciden
con error de cámara de **1,00 m / 0,090°**; se cargan 106 partes. Se leen ocho SMS
y 38 WAV, con 21 archivos referenciados ausentes del paquete original. Son
comprobaciones de carga y reproducción, no paridad del mezclador de OR.

## Las seis estaciones de Chiltern

`check_station_cameras.py` exige ahora las doce vistas. Error máximo medido:
**1,720 m / 0,654°**, manteniendo 3 m y 1°. Denham, Denham Golf Course y Gerrards
Cross se inspeccionaron desde cabina y exterior con los mismos modelos, árboles,
andén y cercos originales. Las referencias originales conservan sus hashes;
las imágenes Bevy y los resultados están en
[las pruebas nuevas](fixtures/compatibility/journey-release-2026-10-05/README.md).
Persisten diferencias de iluminación, sol y detalle a distancia entre motores.

## Sonido: semántica y atenuación

[openrails-audio.json](../oracles/openrails-audio.json) congela resultados de las
DLL originales fijadas: lectura SMS, siete cruces de distancia, 36 ganancias
con `SoundSource.SetRolloffFactor` y nueve identificadores de eventos de freno.
La captura se repitió con salida idéntica. Rust exige coincidencia de comandos
y umbrales y error absoluto de ganancia menor que `1e-6`.

La [prueba de siete formaciones](fixtures/audio/native-consistency-2026-10-05.json)
comprueba señal, ausencia de saturación y recursos SMS/WAV. Las doce vistas de
Chiltern no tienen advertencias; las dos de Class 47 conservan 21 WAV ausentes
del paquete. Esto no certifica identidad acústica de la mezcla completa con
OpenAL. Alcance y comandos en [NATIVE_AUDIO.md](NATIVE_AUDIO.md).

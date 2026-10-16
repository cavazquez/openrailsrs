# Actividades, mercancías y puntos operativos PAT

La referencia de estas operaciones es el código de Open Rails 1.6.1. Los ACT,
CON, WAG y WORLD del autor se leen desde su instalación. Las formaciones
derivadas de un `WagonsList` se escriben en la caché de importación, conservando
los archivos y los identificadores originales.

## Prueba rápida desde el menú

En **Nueva partida**, elegí la ruta de ejemplo **route** y buscá el servicio
**Actividad: carga, composición y guardado**, incluido en
`examples/activity_operations/scenario.toml`. Conservá
la formación del ejercicio: una locomotora diésel y tres vagones de mercancía.

1. Abrí **F7 · Servicio**. El evento de composición debe reconocer los cuatro
   UID, publicar su mensaje y activar el evento final del ejercicio. La actividad
   permanece pendiente hasta su evento de tiempo a los 300 segundos.
2. Con el tren detenido y el regulador cerrado, abrí **B · Tracción** o
   **F9 · Formación**. El primer vagón está alineado con el cargador. Pulsá
   **Cargar el vagón alineado**. Tras un segundo de apertura, su cantidad debe
   subir a unos 100 kg/s hasta **2,4 t**. Los otros dos vagones siguen vacíos.
   La masa física del vagón aumenta con su carga.
3. Guardá durante la transferencia y reanudá esa partida. Deben conservarse la
   cantidad, la apertura pendiente y el suministro consumido. **Pausa** detiene
   el tiempo de transferencia; **Desconectar cargador** la cancela. Mover el
   tren o desacoplar el vehículo interrumpe una toma conectada.
4. Con el vagón lleno y detenido, pulsá **Descargar el vagón alineado**. En este
   ejercicio transcurren un segundo de apertura y cinco segundos de retardo
   antes de bajar la cantidad. Desconectar conserva la cantidad restante.
5. Revisá el vagón en F9: muestra su UID, masa y cantidad. Alinear un vehículo
   sin toma compatible, intentar transferir en movimiento o usar otro material
   debe producir un diagnóstico y conservar la carga.

Este escenario y sus recursos son propios del proyecto. Sus eventos sirven
para practicar el flujo del menú; no representan una captura del simulador
original.

## Tutorial original de Zig Zag

Con **Great Zig Zag v1.6** y su material rodante original instalados, elegí
**Tutorial #6 - Loading and Servicing** en los servicios de esa ruta. Si la ruta
no aparece, agregá su instalación desde la biblioteca y revisá la auditoría de
recursos antes de iniciar. El archivo es `ACTIVITIES/tutorial#6_loading.act`.

La distribución oficial v1.6 examinada tiene un recurso de frenos ausente:
`AU_NSW_1905_JWW_Htype/JWW_Htype_E_animated.wag` incluye
`../Common_Inc_Brake/TripleValve_West_Plain.inc`, que no está en ese instalador.
La variante `JWW_Htype_L_animated.wag` contiene la misma referencia. El inicio y la aceptación original
se detienen indicando el archivo faltante; no se elimina el Include ni se le
asigna un reemplazo de otro vagón. Hasta que exista una corrección del autor,
la prueba de los tres vagones de este tutorial sigue pendiente. Los ejercicios
propios y el ACT BNSF permiten comprobar el backend por separado.
El [recibo de contenido original](fixtures/compatibility/zigzag-tutorial6-original-blocked.json)
registra hashes, catálogos y páginas del autor examinados. El issue #212 permanece
abierto hasta que la aceptación original pueda completar esa prueba.

F7 conserva los mensajes y las condiciones del autor. El primer mensaje se
activa a los cinco segundos. En F9/M deben existir los tres vagones vacíos de
la vía de báscula como una formación estática que ocupa su vía. Sus UID son
**2147483648, 2147483649 y 2147483650**, en ese orden. Al recogerlos conservan
su identidad; un guardado intermedio debe conservar su composición y posición.

El tutorial indica llevar esos tres vagones a la mina, cargarlos y devolverlos.
Tras recogerlos, alineá cada toma de carbón con el cargador del WORLD original,
detené el tren, cerrá regulador y usá B/F9. Cada vagón declara **10.160,5 kg** en
vacío y **16.256,8 kg** de carbón de capacidad. Las tres cargas completas suman
**48.770,4 kg**. Las animaciones alternativas `FreightCoal` y `FuelCoal`
describen una misma carga: no duplican la capacidad.

Hay una diferencia importante en el archivo del autor: sus once eventos son de
tiempo, posición, velocidad y `AllStops`. No contiene un evento de cantidad de
carbón. Además, el `AllStops` de éxito no tiene tareas de pasajeros. El visor
mantiene ese objetivo pendiente, como la condición nativa, y no inventa un
evento de carga ni aprueba la actividad al terminar el PAT. La secuencia completa
del tutorial, su mesa giratoria y las maniobras de todos sus servicios AI no
quedan certificadas por la prueba acotada de carga.

## Condiciones ACT y persistencia

Se ejecutan tiempo, posición ligada a la vía nativa, velocidad, `AllStops`,
`PickUpWagons`, `AssembleTrain`, `AssembleTrainAtLocation` y
`DropOffWagonsAtLocation`. Los eventos conservan orden de archivo, UID,
activación, reversibilidad y resultados de mensaje, éxito, fallo o activación de
otros eventos. La posición no se satisface por estar en una vía paralela cercana.

La recogida exige los UID pedidos en el tren del jugador y velocidad de hasta
1 m/s. La composición exige la lista completa en orden directo o invertido;
un subconjunto o una permutación distinta no basta. Las condiciones con apartadero
usan sus extremos TDB. Se conserva una peculiaridad de `DropOffWagonsAtLocation`
en OR 1.6.1: busca algún UID listado en un tren no estático y comprueba el
apartadero del jugador. No equivale a exigir que todos esos vagones estén
desacoplados.

Los eventos o resultados que requieren capacidades todavía no admitidas,
incluido `ORTSTriggeringTrain`, permanecen pendientes con diagnóstico en F7.
No se reinterpretan como carga ni como éxito. Guardar conserva los eventos
activados, mensajes y cantidades. Un guardado con UID, perfiles o cantidades
incompatibles se rechaza.

Con el contenido original **BNSF Scenic** completo, elegí **4. Everett
Switching** desde Nueva partida. F7 muestra las órdenes originales de recogida
y entrega. M y la lista de servicios de F7 deben conservar las seis formaciones `act-static-32769`,
`32772`, `32773`, `32774`, `32776` y `32777`, ocupando sus vías. Guardá y
reanudá antes de recoger una sección: su ubicación e identidad deben permanecer.
Tras recoger sus coches, F9 permite revisar los UID que pasaron a tu formación.
Una recogida con un UID ausente o a más de 1 m/s no cumple su evento. Completar
el PAT no satisface las órdenes pendientes. Este flujo permite revisar la
interacción; no certifica por sí solo completar toda la actividad de maniobras.

Los marcadores `WagonData ( "" "" )` originales de Chiltern generan un aviso;
el actor y sus coches reales se conservan, como en OR. Una formación que cruza
un cambio se extiende por los `TrPins` originales: al retroceder toma el pin
común y al entrar de frente respeta `TrackShape.MainRoute` de TSECTION. La
regresión real de Birmingham conserva todos los actores y el orden de sus UID.
Un apartadero sin extremos válidos mantiene su ID y distancia para otros
consumidores, pero no certifica una condición de entrega. Las referencias reales
ausentes, inseguras o dirigidas fuera de Trainset conservan su diagnóstico de
error.

## Puntos de espera PAT

En Nueva partida elegí **PAT: corte 40102 y código no admitido**. Con el tren
detenido, seleccioná el inversor hacia adelante. El primer marcador interpreta
`40102`: conserva la locomotora, asegura y separa los tres vagones, y espera dos
segundos antes de habilitar el siguiente tramo. F9 debe mostrar una sola sección
estacionada; guardar y reanudar durante la espera no repite el corte. Conducí
despacio los 50 metros hasta el segundo marcador. El código `60003` debe quedar
pendiente con su diagnóstico; no debe iniciar una espera de 60.003 segundos ni
completar el servicio.

Los códigos inferiores a 30000 son esperas en segundos. `3HHMM` es una hora
absoluta válida del reloj de la actividad, con cruce de medianoche. `4NNSS`
conserva los primeros NN vehículos y espera SS segundos; por ahora se ejecuta
en tramos no invertidos que conservan la cabina activa. Un corte imposible
permanece pendiente. `5NNSS`, `60001`, `60002`, horas inválidas y códigos
desconocidos se distinguen y diagnostican, sin aprobar una maniobra inexistente.

## Repetir la aceptación CPU

Las pruebas propias cubren límites de suministro, apertura/descarga, cambio de
masa, cancelación, guardado, alteraciones inválidas, composición y códigos PAT:

```bash
cargo test -p openrailsrs-formats --lib native_freight_alternatives
cargo test -p openrailsrs-formats --lib typed::activity_events
cargo test -p openrailsrs-formats --lib pat_codes_distinguish
cargo test -p openrailsrs-msts --lib service_formation_keeps_author_hash_and_spaces
cargo test -p openrailsrs-sim --lib original_composition_conditions
cargo test -p openrailsrs-sim --test activity_operations --test pat_operations
```

La aceptación del contenido original necesita una instalación local completa y
un directorio de salida nuevo:

```bash
cargo run -p openrailsrs-sim --example accept_zigzag_activity -- \
  "$ZIGZAG_ROUTE" \
  "$ZIGZAG_ROUTE/ACTIVITIES/tutorial#6_loading.act" \
  tmp/zigzag-activity-fresh
```

El programa escribe primero su protocolo y tolerancia de **0,02 kg**; conserva
la importación completa, las identidades y la ocupación inicial; comprueba el
mensaje original, las cantidades de los tres WAG y una reanudación a media carga.
Registra los hashes de los archivos originales leídos y vuelve a comprobarlos
al terminar. Usa el cargador WORLD original con alineación prescrita: comprueba
la transferencia, no el viaje hasta el cargador. No ejecuta los 27 minutos de
tráfico previos al inicio. Los errores producen `failed.json` y código distinto
de cero; sólo una ejecución completa produce `result.json` aprobado.

La lectura y las condiciones originales de **BNSF Scenic · 4_lumberrun** tienen
otra aceptación, con sus listas UID y apartaderos 553, 493 y 491. Comprueba que
el `SidingItem` del evento no se confunda con el valor 4294967295 que acompaña
a cada vagón de `Wagon_List`. Ejecuta estados controlados de recogida, vagón
ausente, velocidad excesiva, descarga, apartadero incorrecto y reanudación:

```bash
cargo run -p openrailsrs-sim --example accept_native_composition -- \
  "$BNSF_ROUTE/ACTIVITIES/4_lumberrun.act" \
  tmp/bnsf-composition-fresh --actors
```

Los eventos y sus resultados proceden del ACT intacto. La formación y la
geometría del apartadero son las del ejercicio propio: esta prueba cubre las
condiciones y la persistencia, no certifica el recorrido completo de BNSF ni
constituye una captura de las DLL originales.

La opción `--actors` añade una comprobación separada: importa el ACT y sus seis
formaciones originales, exige la ocupación de toda su longitud con tolerancia de
0,01 m, comprueba UID/orden, conserva su posición durante un segundo simulado y
reanuda el agregado jugador/tráfico. Las huellas de los recursos originales
leídos se contrastan al final. Sin esa opción sólo se ejecutan los seis estados
controlados de condiciones.

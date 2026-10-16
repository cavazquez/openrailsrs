# Secciones desacopladas y porciones de servicios

Una sección conserva sus coches, su identidad, las presiones de freno y la vía en la que quedó. Cortar otra sección no reemplaza la anterior. Al recoger una sección cambia el orden de la formación y de sus perfiles físicos, no la posición de los coches estacionados.

## Dos secciones propias

Desde **Nueva partida**, elegí el ejemplo **Porciones: dos cortes con identidad**. Si hay un filtro de país, usá **Todos**. Los ejemplos propios usan vehículos sencillos del patio y no requieren descargas.

1. Con el tren detenido, abrí **F9 · Formación**. Seleccioná el coche 3, pulsá **Asegurar sección posterior** y **Desacoplar detrás del seleccionado**. Quedan dos vagones estacionados.
2. Seleccioná el coche 1, asegurá la nueva cola y separala. F9 debe mostrar dos identidades de sección y su vía. **M** debe mostrar ambos grupos por separado.
3. Guardá y cargá la partida. Deben conservarse las dos secciones, los mismos coches y sus frenos de mano.
4. En este montaje inicial, recogé primero la sección más cercana **detrás** y después la otra. Una sección distante o en otra vía debe mostrar el motivo del rechazo. No alcanza con pulsar su nombre.
5. Tras cada recogida, conectá la manguera del nuevo empalme, abrí sus llaves y soltá los frenos de mano antes de acelerar. El tren debe moverse como una sola formación.

También existe **Recoger delante**. Sirve cuando el extremo delantero del tren está en contacto con la cola de la sección después de un rodeo físico. La operación comprueba la vía y posición de todos sus coches. No cambia por sí sola el recorrido ni mueve la locomotora alrededor de los vagones. El ensayo unitario de orden inverso prescribe únicamente la posición de contacto de la locomotora: no debe confundirse con una prueba de conducción del rodeo completo.

## Porciones de otro servicio

Elegí **Porciones de dos servicios: unir y devolver**.

1. En **F7 · Servicio**, recogé **Porción-Sur detrás**. Está asegurado y en contacto desde el inicio. Desaparece como tren independiente y sus tres vehículos pasan a F9, conservando sus identidades originales.
2. Guardá y cargá. Deben seguir existiendo seis vehículos en total, sin una segunda copia del servicio recogido.
3. Sin soltar los frenos de mano de la porción, abrí **F9** y pulsá **Devolver porción al servicio Porción-Sur**. El servicio recupera su horario original; sus coches conservan presión, combustible y estado de equipos. Las reservas de ambos servicios se reúnen al acoplar y se separan por la huella real de cada porción al devolverla; el despachador vuelve a comprobar los cambios y los bloques.
4. Volvé a guardar/cargar y recogelo de nuevo. Las dos formaciones deben mantener su posición. Conectar el empalme y soltar sus frenos permite conducir la formación reunida.

Sólo se devuelve una porción ajena completa que esté en un extremo. Separar una porción intermedia dejaría un hueco: la operación se rechaza en vez de juntar los dos lados desplazando coches. Si el conductor está en la porción a separar, primero hay que cambiar a una cabina de la parte que se conserva.

Las operaciones de horario `join_service` y `split_service` usan esas mismas transacciones. No se dan por cumplidas por llegar al final de un recorrido. La recogida espera que el otro servicio exista, esté detenido y tenga contacto físico; la división conserva su identidad y programación. El guardado conserva el progreso y reproduce las transacciones antes de restaurar los estados finales.

## Compatibilidad y validación

La referencia sigue siendo Open Rails 1.6.1. Los oráculos existentes de acople conservan los ensayos nativos de transferencia de coches, frenos y equilibrios prescritos. Las múltiples secciones, la elección manual de extremo y el remapeo de entidades se prueban como extensiones propias de Rust; no se declara equivalencia de toda la maniobra automática de Open Rails.

El importador de horarios admite `$pickup=SERVICIO` dentro de la misma tabla y la devolución de esa porción completa mediante `$detach/units=N/forms=SERVICIO`. Conserva los signos originales de `units`. Los actores se cargan de sus CON/PAT originales y sus nombres se resuelven sin distinguir mayúsculas ASCII. Un servicio anterior al inicio se prepara con su tiempo previo, hasta seis horas. En una recogida automática, el personal conecta el nuevo empalme y suelta los frenos de mano de la porción; en una división automática la asegura antes de separarla. Las mismas operaciones hechas a mano en F9 dejan esas tareas a cargo del jugador.

`$attach` completo, crear un servicio nuevo desde una porción arbitraria, pools y referencias a otras tablas siguen diagnosticándose expresamente. Las transacciones de porciones usan el TCS Rust; un host C# activo no se clona. Vapor, diésel y eléctrico conservan sus estados por vehículo cuando los modos de dinámica de ambos trenes son compatibles. También se guardan las temperaturas de rodamientos y velocidades de ejes de los vehículos estacionados. El aislamiento de la tubería se calcula desde la cabina activa, incluso cuando la locomotora queda detrás de los coches recogidos por delante.

Pruebas dirigidas:

```sh
cargo test -p openrailsrs-sim --test sections_and_services
cargo test -p openrailsrs-msts --lib portion_tests
cargo test -p openrailsrs-viewer3d --lib train_presentation::portion_entity_tests
```

Los registros de ejecución del lote están en `tmp/next-ten-20261015/`. Los fallos anteriores se conservan y no cuentan como aceptación.

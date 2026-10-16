# MidEast: Firsby–Spilsby, rodeo y regreso

La actividad original **Firsby to Spilsby Return** usa la locomotora de vapor **84030** y dos **Mk1**. Sus cuatro tramos PAT incluyen la vía de rodeo. El objetivo ACT es detenerse en las seis paradas; llegar a un punto final del PAT por sí solo no completa la actividad.

La ruta y los vehículos del autor se leen desde la biblioteca instalada. No están incorporados al repositorio. Esta es una aceptación operativa de importación, maniobra, persistencia y objetivos originales; no certifica paridad completa de la física del vapor con Open Rails 1.6.1.

## Prueba desde el menú

1. Abrí **Nueva partida**, elegí el país de la ruta o **Todos**, **MidEastCoast** y la actividad **Firsby to Spilsby Return**. Conservá su formación original: una 84030 y dos Mk1. La hora del autor es 08:00. **F6** muestra las teclas vigentes; **F9** muestra la formación y **M** el itinerario.
2. Serví Firsby y Halton Holegate antes de llegar a Spilsby. Detenete en las marcas de parada y abrí las puertas. El modo habitual conserva las esperas del autor; la práctica rápida opcional sirve para aprender pero no es la configuración del ensayo de aceptación.
3. Después de atender la primera parada de Spilsby, con el tren detenido, abrí **F9**, seleccioná la locomotora y usá **Asegurar sección posterior** y **Desacoplar detrás del seleccionado**. Debe quedar una locomotora activa y una sección de dos Mk1 con freno de mano. Guardá y cargá: la vía, las presiones y la posición de los coches deben conservarse.
4. Avanzá con la locomotora hacia el marcador de inversión. La locomotora completa debe librar el cambio. Estar dentro de los diez metros de tolerancia del marcador no basta si su cola sigue sobre otra vía. En ese caso el HUD pide avanzar despacio y mantiene el itinerario pendiente: no declara fallida la actividad ni traslada vehículos.
5. Detenete, pasá el inversor por neutro y seleccioná atrás. El segundo PAT dispone los cambios para la vía de rodeo. Recorré esa vía y detenete en la siguiente inversión; los dos Mk1 deben seguir inmóviles en el andén.
6. Seleccioná adelante para el tercer PAT. Acercate muy despacio al extremo posterior de los Mk1, quitá el regulador y detenete con los enganches en contacto. En **F9**, elegí esa sección y **Recoger delante**. Los dos coches pasan delante de la locomotora; la cabina activa sigue perteneciendo a la 84030.
7. En el nuevo empalme, conectá la manguera delantera de la locomotora, abrí su llave delantera y la llave trasera del coche contiguo. Soltá los dos frenos de mano. Un empalme cerrado debe aislar los coches; no debe aislar la locomotora que conserva la cabina.
8. Guardá y cargá de nuevo. Debe conservarse el orden Mk1–Mk1–84030, la cabina, la presión y el agua/carbón restantes. No debe aparecer una segunda copia de los coches ni recargarse el ténder.
9. Atendé la segunda parada de Spilsby, invertí para el regreso y llamá nuevamente en Halton Holegate y Firsby. Al completar las seis paradas, el resumen debe indicar que se cumplió el ACT original, sin paradas omitidas.

Una sección en otra vía o con el enganche distante debe rechazarse. No se puede recoger un servicio programado para el futuro. Si intentás invertir antes de librar los cambios, la operación debe quedar pendiente y poder continuar después de corregir la posición. Para desacoplar, la parte estacionada necesita freno de mano; cambiar de cabina o de servicio exige detener los trenes.

## Reproducción sin renderer

El arnés usa los mismos `LiveDriveSession`, `LiveTraffic`, frenos y funciones F9 de la partida. Modula regulador/freno, respeta las paradas y conduce los cuatro PAT. Sólo la ruta importada y los resultados se escriben en una carpeta nueva de QA; los recursos originales no se modifican.

```sh
cargo build -p openrailsrs-sim --example mideast_portions_qa
# Las dos carpetas de salida deben ser nuevas.
target/debug/examples/mideast_portions_qa /ruta/instalada/Routes/MidEastCoast tmp/mideast-import
target/debug/examples/mideast_portions_qa --drive tmp/mideast-import/scenario.toml tmp/mideast-drive
```

`drive.csv` conserva el movimiento integrado; `report.json` enumera corte, tramos originales, acople y restauraciones. Los dos guardados intermedios se conservan por separado. El arnés exige posición exacta de los coches durante el rodeo y declara antes de conducir una tolerancia de **1 cm** para el ajuste geométrico del contacto. No usa desplazamientos prescritos de la locomotora para completar esta maniobra.

La regresión pequeña `native_reversal_clearance` prescribe geometría para comprobar el error de cola sobre el cambio y su guardado. Esa prueba no se presenta como conducción del rodeo; el arnés con el contenido instalado es el caso de aceptación completo.

# Suspensión: dos casos aprobados y un fallo del guion conservado

El binario normal build05 reparó el retorno temprano que omitía la suspensión
física cuando la vibración cosmética estaba apagada. Desde reposo se ejecutaron
[irregularidades activadas](vertical-on.png) y
[desactivadas](vertical-off.png), con pausa y restauración JSON antes de abrir
**B → Tracción**. Ambos casos pasaron los criterios originales.

Con la opción activa, el render registró dos carrocerías, desplazamiento físico
máximo de 4,480 mm y veinte piezas de rodaje compensadas, aun con movimiento
cosmético `off`. Las cargas de rueda variaron. Al desactivarla, el desplazamiento
renderizado fue cero y los apoyos conservaron su carga estática. Las PNG permiten
leer la opción, carga normal y contactos perdidos; el panel tapa el tren, por lo
que estas imágenes no acreditan una oscilación visual completa ni la pose de
todas sus ruedas. Esas propiedades tienen pruebas CPU/ECS separadas.

El caso [descarrilamiento](derailment-from-rest.png) llegó a un incidente real y
conservó la causa tras restaurar y desactivar consecuencias. Sin embargo, el
guion **falló**: exigía una velocidad positiva en alguna acción y sólo tomó
acciones al arrancar y al final, cuando el tren ya se había detenido. Su
[fallo](failure.json) y [resultado parcial](result.json) siguen intactos. No se
reinterpreta ese intento como aprobado. El
[reintento estricto posterior](../physical-driving-gpu03/README.md) agregó una
observación intermedia sin cambiar física, mandos ni límites.

[qa-summary.json](qa-summary.json) conserva estados, cronología, hashes y revisión
de los tres PNG. Los tres visores y sus compositores terminaron con código 0 y
sus seis PIDs dejaron de existir. Se usó Vulkan en RX 7600 a 1280×720, con datos
privados y compositor propio; hubo trabajo CPU concurrente. No es un benchmark
aislado ni una aceptación de recursos originales.

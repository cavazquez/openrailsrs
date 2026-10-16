# Conducción física: tres casos aprobados y un fallo conservado

Con el binario normal identificado en [plan.json](plan.json) se ejecutaron, desde
reposo y con mandos normales, [hidráulica](hydraulic-running.png), [frenado por
cremallera](rack-positive-braking.png) y [salida de cremallera](rack-entry-exit.png).
Los tres pasaron sus comprobaciones físicas; sus PNG fueron inspeccionados.
Tracción permanece visible tras pausa y restauración JSON, con los medidores
hidráulicos y de cremallera legibles. Parte de los auxiliares de los casos de
cremallera queda bajo el área desplazable; no se ensayó la rueda del mouse.

El cuarto caso, [irregularidades activadas](vertical-on-failed.png), falló: las
cargas y desplazamientos del solver evolucionaban, pero el tracker de movimiento
renderizado estaba vacío y el desplazamiento físico visible registrado era cero.
[El fallo original](failure.json) detuvo la ejecución. Los casos de irregularidad
desactivada y descarrilamiento **no se ejecutaron**. No son seis pases.

La lectura del código confirmó que el retorno temprano de TrainMotion cuando la
vibración cosmética está apagada omitía la suspensión vertical física habilitada.
La hipótesis inicial de que faltaba la raíz del proxy no se confirmó: la raíz sí
existe. La reparación y su nueva prueba quedan separadas de este intento.

[qa-summary.json](qa-summary.json) conserva imágenes, hashes, estados, mandos y
límites; [result.json](result.json) conserva el resultado parcial original sin
reescribirlo. Los cuatro visores y sus compositores terminaron normalmente y no
quedaron sus PIDs. Se usó hardware Vulkan RX 7600, a 1280×720, con datos privados y
compositores propios. Había trabajo CPU concurrente; no es un benchmark aislado.
No se distribuyen recursos originales ni datos privados del jugador.

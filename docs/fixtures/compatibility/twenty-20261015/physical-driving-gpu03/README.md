# Descarrilamiento desde reposo: reintento estricto aprobado

El [plan](plan.json) y la [propuesta previa](proposal04.json) se guardaron antes
de ejecutar. Se mantuvieron el binario build05, formación propia, mandos, física,
umbrales y duración de 55 segundos reales a 2×. El único cambio frente al
[intento anterior](../physical-driving-gpu02/README.md) fue cerrar el panel a los
30 segundos para registrar una velocidad durante la marcha. No se impusieron
posición, velocidad, fuerzas ni causa, y las aserciones permanecieron idénticas.

La observación intermedia registró 29,340 m/s y 1.279,386 m de odómetro a los
59,30 segundos simulados. El incidente `WheelUnloading` ocurrió a los 83,60 s,
29,097 m/s, en el punto 2.095,057 m. Se registraron 163,520 kN de fuerza lateral
y carga mínima de rueda cero. Después de detenerse, pausar, restaurar por JSON
y desactivar consecuencias, la causa siguió presente.

La [imagen original](derailment-from-rest.png) fue inspeccionada: se leen la
causa, tiempo, posición, velocidad del incidente, fuerza lateral, carga mínima,
emergencia aplicada, esfuerzo cero y consecuencias desactivadas. Las líneas
inferiores de auxiliares continúan fuera del área visible; no se ensayó la rueda
del mouse. No se afirma que la imagen muestre una caída o una colisión.

El [resultado original](result.json) pasó sin relajar las comprobaciones;
[qa-summary.json](qa-summary.json) agrega la revisión de píxeles, cronología y
hashes. Visor y compositor terminaron con código 0 y sus PIDs desaparecieron.
Se usó Vulkan en RX 7600, 1280×720, con trabajo CPU concurrente. Es una formación
y un ejercicio propios de laboratorio, no una validación visual de material
original ni un oráculo de un solver que Open Rails 1.6.1 no incorpora.

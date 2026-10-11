# Diez mejoras de conducción y operación

La referencia permanece fijada en Open Rails 1.6.1. Las rutas instaladas desde Biblioteca
quedan en los datos del usuario. El repositorio y el Snap incluyen los ejemplos y
recursos versionados del proyecto; no incorporan esas descargas ni partidas guardadas.

## 1. Carga y descarga durante el viaje

Las colas materializan un paquete WORLD y uno de terreno por cuadro. Al retirar un
sector, se destruyen como máximo 128 entidades y ocho recursos de formas por cuadro;
los recursos compartidos siguen vivos mientras otros objetos los usan. La limpieza
continúa después de retirar la última entidad. Esto limita el trabajo acumulado de
una estación densa, aunque una forma individual grande todavía puede producir un pico.

**Prueba manual:** completá el servicio local a tiempo ×1, alternando `1` cabina y
`2` exterior. Usá 2 km de escenario y compará lluvia/nevada intensas. Deben continuar
la vía y el paisaje al cambiar de sector, sin huecos en la formación. F11, en un build
con `dev-tools`, muestra cuadros y picos; calidad Adaptativa permite ajustar el
presupuesto al equipo. La carga inicial se mide por separado del viaje.

## 2. Alpha Linux y Snap

La receta utiliza Ubuntu core24, confinamiento estricto y canal edge. La aplicación
incluye CLI, menú, ejemplos, shaders, certificados HTTPS, teclado y salida de audio.
Los perfiles, cachés y descargas se guardan fuera del paquete.

**Prueba manual:** actualizá con `sudo snap refresh openrailsrs --edge`, iniciá
`openrailsrs`, abrí Biblioteca y comprobá la ubicación de tu contenido. Probá una
partida, guardala y cerrá el visor; Continuar debe conservar el estado. Activá sonido
en F10 y comprobá bocina y limpiaparabrisas. La publicación y evidencia del paquete
se registran en el issue #208.

## 3. Ruedas, bogies, suspensión y enganches

Cada coche utiliza su movimiento y su posición en la vía. Una sección estacionada
no hereda la suspensión de la locomotora que se aleja. Los bogies, ruedas y bielas
compensan la suspensión de la carrocería para conservar su apoyo en los rieles.
Los enganches identificados por matrices COUPLER comparten el ángulo entre coches;
normalizan vehículos invertidos y vuelven al reposo al desacoplar.

**Prueba manual:** en exterior, observá un cambio y una curva a velocidad baja.
Las ruedas giran, los bogies siguen la vía y las carrocerías pueden balancearse
suavemente. En F10 → Imagen y cabina compará movimiento Apagado/Suave/Normal.
Desacoplá una sección asegurada y avanzá: la cola permanece inmóvil. Pausa y
reanuda sin acumular desplazamientos. Un modelo sin matrices de enganche o bielas
conserva su geometría original; no se le inventan piezas móviles.

## 4. Audio al acercarse, pasar y alejarse

Los SMS/WAV exteriores mono reciben balance estéreo por posición y Doppler acotado
por velocidad radial. El balance cambia sin reiniciar un bucle. Los interiores,
Ignore3D y los archivos estéreo del autor conservan su mezcla y altura originales.
Un salto grande de cámara no se interpreta como velocidad de paso del tren.

**Prueba manual:** activá audio y elegí el servicio local con tráfico. Desde exterior,
observá el Pullman contrario: cambia de lado y pierde volumen al alejarse. Desde
cabina y vista de pasajeros, comprobá su mezcla propia. P pausa y continúa; no deben
rearrancar los bucles al mover ligeramente la cámara. Necesita los SMS/WAV originales
que informe la auditoría de formación.

## 5. Independiente, emergencia y depósito principal

`[` libera y `]` aplica el freno independiente neumático de las locomotoras que lo
tienen. En partida, esos botones ya no mueven el zoom de la cámara: se usan coma/punto o la rueda. B ofrece los mismos botones y muestra depósito, compresor y presión.
Independiente y servicio comparten el cilindro: se toma la mayor demanda, sin sumar
ambas fuerzas. Los coches remolcados mantienen su freno de servicio propio.

Backspace aplica emergencia y corta tracción. Para rearmar con N hay que detenerse,
llevar regulador a cero y corregir cualquier manguera abierta. El freno de servicio
no se libera automáticamente al rearmar. El manómetro de cabina lee el cilindro
principal y el depósito físico, mientras F9 permite ver la fuerza por coche.

El depósito tiene volumen, carga, máximo e histéresis de compresor leídos del ENG;
la carga requiere alimentación auxiliar. Su consumo utiliza un cilindro efectivo
de 10 L por locomotora cuando no existe geometría validada. Es una extensión física
acotada: no certifica todas las válvulas de emergencia o el circuito completo de OR.
El independiente de vacío no se ofrece como si fuera neumático.

**Prueba manual:** elegí «Consumo eléctrico: pasajeros y auxiliares» o una locomotora
neumática original. Acelerá sin servicio y aplicá independiente: debe reducir la
aceleración y subir la presión sólo en la locomotora. Soltalo, aplicá emergencia,
intentá rearmar en marcha y luego detenido. En B observá caída/recarga del depósito;
al cortar auxiliares el compresor deja de recargar. Guardar conserva emergencia,
presión y compresor. Podés reasignar las teclas en F10 si tu teclado lo requiere.

## 6. Electricidad de pasajeros y auxiliares

Se leen ORTSPowerSupplyContinuousPower, ORTSPowerSupplyHeatingPower y
ORTSPowerSupplyAirConditioningPower de cada vehículo. B permite Auto, Apagada,
Calor y Frío; Apagada mantiene el consumo continuo de iluminación. Auto elige calor
por debajo de 18 °C y frío por encima de 24 °C. También se admiten cargas declaradas
en el escenario. Sin parámetros se conserva consumo cero, sin inventar cifras.

Las cargas necesitan auxiliares/ETS reales y se descuentan de la potencia disponible
para tracción. B presenta kW y energía acumulada en kWh. Al desacoplar se retiran las
cargas de esa sección. Guardado y reanudación conservan modo y energía.

**Prueba manual:** en «Consumo eléctrico: pasajeros y auxiliares», abrí B y elegí
Frío: dos coches consumen 176 kW y los auxiliares 10 kW. Apagada mantiene 16 kW
continuos de pasajeros. A igual regulador, el frío reduce la aceleración y aumenta
el consumo. Cortá ETS en F9: desaparece su carga; reconectá y comprobá los retardos.

## 7. Vapor sincronizado

Las ruedas motrices, bielas con animación original, emboladas SMS y pulsos del escape
usan una misma fase de vueltas de rueda, incluyendo patinaje y marcha atrás. El
escape depende del esfuerzo efectivo y de la caldera; el corte sigue controlando
la tracción y el consumo. En rueda libre no se mantienen emboladas de trabajo.

**Prueba manual:** usá Hall o King originales disponibles, o «Vapor: fogonero y ténder
finito» para los controles físicos. Abrí B, variá regulador y corte y observá desde
exterior. Al aumentar velocidad aumentan emboladas y movimiento; al soltar regulador
baja el escape de trabajo. Frená, invertí y comprobá el sentido de las ruedas y
bielas. P congela fase, sonido y emisiones. El modelo de conservación de caldera
continúa siendo propio; esto no declara paridad completa de termodinámica de OR.

## 8. Temperatura y diagnóstico diésel

Se leen temperatura máxima, constante térmica y los cuatro modos de refrigeración
de DieselEngine. La demanda utiliza tracción transmitida y electricidad entregada.
B muestra temperatura, refrigeración y la causa actual de pérdida de tracción:
neutral, emergencia, frenos, puertas, combustible, conexión, temperatura, patinaje,
consumo auxiliar o caída normal del esfuerzo con velocidad.

La reducción por sobretemperatura requiere ORTSRSOverheatProtection (1). Es una
opción de juego: no introduce una avería automática en los trenes originales.
Los cuatro cálculos térmicos se contrastan con 200 puntos y 8.000 entradas RPM/carga
de las DLL originales 1.6.1, tolerancia 0,01 °C y estado de refrigeración exacto.

**Prueba manual:** elegí «Diésel: temperatura y protección térmica», cerrá puertas,
poné Adelante y acelerá. B debe explicar la reducción al superar 70 °C. Bajá
regulador y observá recuperación. En un diésel original sin esa opción, la temperatura
no agrega protección artificial. Guardá/reanudá y comprobá temperatura y diagnóstico.

## 9. Servicio con tráfico original

«Chiltern local con tráfico: Northolt Park → West Ruislip» conserva tres paradas del
jugador, dos del tren adelantado y una del contrario. Los servicios comparten reloj,
clima, audio, ocupación de toda la formación, señales y partidas guardadas. En el menú
se utiliza Birmingham Pullman.con del Content original instalado para los trenes AI.

**Prueba manual:** elegí Chiltern clásico y ese servicio. Observá el adelantado y la
señal ocupada, y seguí su cambio al liberar el bloque. El contrario sale a los 700 s
de simulación y para en la vía contigua; F10 → General permite acelerar el reloj.
F7 debe registrar las tres paradas propias; F8 permite inspeccionar servicios.
Guardar antes de su salida conserva los horarios al reanudar. El piloto curado usa
el grafo clásico; no se anuncia el mismo itinerario como validado para Chiltern v4.

## 10. Patio guiado

En Nueva partida buscá «Patio de prácticas: separar y volver a formar». Es un patio
de aproximadamente 1 km con material sintético incluido y cinco objetivos. F4 y F9
muestran la instrucción actual; F7 conserva los pasos cumplidos y el resultado.

1. Detenido en 160 m, abrí F9 y seleccioná el primer coche. **Asegurar sección
   posterior** y **Desacoplar** dejan los otros dos coches con freno de mano.
2. Avanzá hasta 240 m y detenete. La distancia de B/F4 ayuda a alinearte; el objetivo
   no se cumple si reacoplás inmediatamente sin haber despejado.
3. En M seleccioná el cambio «junction» y ordená Desviada. Se permite cambiar el
   tramo futuro mientras la cola asegurada permanece sobre el tramo anterior.
4. Invertí detenido y regresá al punto de recogida de 160 m. F9 → Acoplar. Reconectá
   la manguera, abrí la llave trasera del primer coche y la delantera del segundo,
   y soltá ambos frenos de mano. Acoplar sin preparar frenos no cumple el objetivo.
5. Volvé a Adelante y salí con toda la formación pasando los 400 m. Continuá hasta
   el destino, detenete y completá la parada. F7 y el resumen deben registrar cinco
   objetivos y una parada sin interrupción. Guardar/reanudar a mitad de la maniobra
   conserva los coches estacionados, el desvío y los objetivos.

## Verificación automática

`./check.sh` ejecuta formato, Clippy sin warnings, todos los tests, empaquetado,
web y oráculos fijados. `crates/openrailsrs-sim/tests/ten_goals.rs` prueba los nuevos
frenos, cargas, temperatura, fase de vapor y la maniobra hasta el final.
Los tests de Bevy verifican enganches invertidos, cola inmóvil y limpieza gradual;
los de audio verifican balance continuo, Doppler y excepciones SMS.

La captura de referencia térmica es reproducible con
`scripts/capture_traction_operation_reference.py`. Capturas GPU, histogramas del
viaje ×1, memoria y el paquete publicado se conservan en el registro del issue #208.

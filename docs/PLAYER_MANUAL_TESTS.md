# Prueba manual de la interfaz y de la partida

Las ocho funciones se usan en una partida real. La referencia de contenido sigue
siendo Open Rails 1.6.1; la iluminación, el streaming y la interfaz se ejecutan en
Bevy. El audio continúa desactivado. Esta guía comprueba funcionamiento y efectos
visibles; no certifica la paridad física completa con Open Rails.

## Preparación

Desde la raíz de `openrailsrs`:

```bash
./scripts/run_chiltern_service.sh
```

El script compila y abre el menú. Si el Content está en otro directorio, definí
`CHILTERN_ROUTE` con la carpeta que contiene `WORLD`, `TILES` y `Chiltern.tdb`.
Para saltar el menú: `./scripts/run_chiltern_service.sh --direct`.

Elegí **Chiltern**, **Chiltern local: Northolt Park → West Ruislip**,
**birmingham_pullman**, **Recorrido del servicio**, **09:55**, **Verano** y
**Despejado**. El tren debe tener ocho coches y comenzar detenido en Northolt Park.
No abras dos visores a la vez.

Las teclas siguientes son las predeterminadas. F10 permite cambiarlas y F6 muestra
las asignaciones actuales. Los paneles grandes pausan la simulación; F4 y F5 son
superposiciones que permiten seguir conduciendo. Esc cierra un panel o abre pausa.

## 1. Menú de inicio y selección de contenido

1. Cambiá ruta y servicio con las flechas de sus filas. La lista de formaciones y
   recorridos debe actualizarse para ese contenido.
2. Volvé al servicio Chiltern local y pulsá **Iniciar partida**. Debe aparecer la
   pantalla de carga y después el escenario original, el Pullman y la próxima parada.
3. Volvé al menú desde la pausa y probá otra hora, estación del año y clima. La
   libreta/HUD deben mostrar la hora elegida; el sol debe cambiar con fecha y hora.
   Lluvia muestra precipitación; Niebla reduce la visibilidad
   a unos 500 m cuando la niebla está habilitada. Los recursos estacionales que
   ofrece el Content se cargan mediante su selección original: en invierno el
   terreno usa TERRTEX/Snow y los árboles sus variantes Winter cuando existen.
   Los recursos ausentes usan la textura base. De noche se seleccionan las
   variantes Night que declara cada modelo, además de cambiar la luz del sol.
4. Elegí un PAT diferente en **Recorrido**. Debe indicarse que se iniciará una
   exploración por ese camino, hasta su destino; no deben conservarse las tres
   paradas del servicio local en un recorrido diferente.

Las actividades ACT usan el importador existente. La selección no incorpora un
intérprete completo de todos los eventos ni scripts C# originales.

## 2. Pausa, guardar, cargar y reanudar

1. Pulsá **P** o **Esc** mientras el tren se mueve. Velocidad, odómetro y reloj
   deben congelarse; **Continuar la partida** debe reanudar exactamente esa posición.
2. Guardá en una ranura. Debe aparecer un mensaje con ranura, distancia y velocidad.
3. Continuá, cambiá controles y cámara, avanzá unos metros y volvé a cargar esa
   ranura. Deben volver posición, velocidad, frenos, inversor, puertas, cámara,
   formación, horario y órdenes del despachador. La partida se restaura pausada.
4. Salí y abrí de nuevo el menú. **Reanudar 1/2/3** debe cargar la ranura elegida,
   incluso cuando el archivo de escenario original usaba rutas relativas.
5. Probá cargar una ranura de otro servicio dentro de la partida actual. Debe
   rechazarse con un mensaje, sin alterar el tren. Para cambiar de contenido,
   usá **Menú de inicio** y después **Reanudar**.

Los archivos se guardan en `player-data/save-1.json`, etc. Ajustes y partidas son
archivos locales y quedan excluidos de Git. `OPENRAILSRS_PLAYER_DIR` permite usar
otra carpeta. Volver al menú libera el escenario antes de abrir el selector.

## 3. Monitor gráfico de vía — F4

1. Abrí F4 y comenzá a conducir. Debe verse una línea con señales de colores,
   próximas estaciones, cambios y variaciones de límite dentro de los próximos 5 km.
2. Las distancias deben disminuir al acercarte. Una señal ya rebasada debe salir
   de la lista; la siguiente debe ocupar su lugar. F4 vuelve a ocultar el monitor.
3. En el mapa, ordená **Alto** a una señal libre que esté adelante. Cerrá el mapa:
   el monitor debe mostrarla en rojo y el monitor compacto debe decir **Alto**.
   Restaurá **Automática** antes de continuar.

El color y distancia se calculan a partir de la vía y los estados de la sesión.
No se replica todavía todo el SIGSCR de Chiltern.

## 4. Libreta del servicio — F7

1. Abrí **Briefing**. Debe describir el servicio elegido y cómo completar las paradas.
2. En **Horarios** deben aparecer Northolt Park, South Ruislip y West Ruislip,
   con llegadas/salidas previstas. Al atender una parada aparecen tiempos reales
   y retrasos; mientras embarcan los pasajeros ya debe verse la llegada real.
3. En **Evaluación** comprobá pasajeros, paradas servidas y penalizaciones.
   Al finalizar deben aparecer las tres paradas y el resultado del servicio.
4. En una partida de prueba, pasá una parada sin atenderla o rebasá una señal roja.
   La libreta debe registrar el fallo y el HUD indicarlo. Reiniciá con R para
   realizar una prueba limpia.

## 5. Operaciones de la formación — F9

Hacé estas pruebas con el tren detenido. Guardá antes para poder restaurarlo.

1. Seleccioná un coche y pulsá **Freno de mano**. Su estado debe cambiar a Sí y
   aparecer fuerza de frenado. F8 → Frenos permite revisar los cilindros por coche.
   Con varios frenos de mano aplicados la aceleración debe ser menor. Soltalos.
2. Seleccioná el coche 2 y desconectá su **Manguera delantera** con las llaves
   abiertas. La tubería debe indicar **Venteada (emergencia)** y aplicarse frenado.
   Reconectá la manguera. Cerrando la llave trasera del coche 1 y la delantera del
   coche 2 antes de desconectar, la sección posterior debe quedar **Aislada**.
   Restaurá manguera y llaves abiertas antes de seguir.
3. En los coches motores, probá **Conectar / cortar tracción**, **Batería** y
   **Mando múltiple**. F8 → Locomotora debe reflejar la potencia disponible.
   Con ambas unidades del Pullman sin tracción o batería, el regulador no debe
   producir esfuerzo motor. El mando múltiple controla la unidad posterior;
   la primera conserva su mando local. Volvé a conectar las unidades.
4. Seleccioná el coche 3, pulsá **Asegurar sección posterior** y después
   **Desacoplar detrás del seleccionado**. Deben quedar 3 coches acoplados y 5
   estacionados. Masa/longitud activas deben disminuir. Avanzá unos metros en
   vista exterior: los coches separados, bogies y ruedas deben permanecer quietos.
5. Detenete, elegí retroceso con S y regresá lentamente al punto del enganche.
   A menos de 1 m y detenido, **Acoplar sección estacionada** debe reunir los
   ocho coches y recuperar su masa y propiedades de enganches. Reconectá la
   manguera del coche 4, abrí la llave trasera del coche 3 y delantera del 4,
   y soltá los frenos de mano de los coches 4 a 8.
6. Intentá operar o cambiar directamente de adelante a atrás con el tren en
   movimiento. La operación de formación debe mostrar su rechazo y el inversor
   conservar su sentido anterior; el tren no debe invertir de golpe su desplazamiento.

Se admite una sección posterior estacionada por vez, sobre el mismo recorrido.
Los coches estacionados quedan asegurados; no constituyen un tren AI autónomo.
Batería y mando múltiple actúan como habilitaciones reales de tracción, sin
simular todos los subsistemas eléctricos ni scripts específicos de cada modelo.

## 6. Mapa ferroviario y despachador — M

1. Abrí M. Deben verse la red importada, el recorrido en cian, las secciones
   ocupadas en amarillo, las estaciones y el jugador. Una sección desacoplada
   debe aparecer estacionada. Probá zoom, desplazamiento y **Ajustar recorrido**.
2. Seleccioná un círculo de señal. Su identificador y estado aparecen debajo.
   Probá Alto, Precaución, Vía libre y Automática: el color y el estado de la
   señal deben cambiar y mantenerse al cerrar el panel.
3. Vía libre o Precaución sobre una vía ocupada deben rechazarse. La protección
   también se aplica a señales del sentido opuesto de esa misma vía.
4. Seleccioná un rombo de cambio libre y pulsá **Cambiar desvío**. Debe cambiar
   su posición en el grafo. Un cambio ocupado debe rechazar la orden.
5. Si el cambio pertenece al recorrido restante, se recalcula el itinerario.
   Si corta el acceso al destino o deja una parada sin atender, se rechaza y
   conserva el itinerario previo. No todos los cambios libres ofrecen un
   itinerario alternativo válido para un servicio con paradas.

El despachador opera el tren del jugador y su sección estacionada. No crea
circulaciones AI adicionales ni sesiones de multijugador.

## 7. HUD avanzado — F8; ajustes y teclas — F10

1. Abrí F8 y recorré sus diez páginas: General, Formación, Locomotora,
   Potencia distribuida, Alimentación, Frenos, Fuerzas, Despachador, Clima y
   Diagnóstico. Deben mostrar datos de la sesión, no valores de demostración.
   Diagnóstico incluye FPS, tiempo por cuadro y sectores/entidades del escenario.
2. En F10 cambiá distancia de escenario, campo visual y tamaño de interfaz.
   Al cerrar deben cambiar alcance del escenario, amplitud de la vista de
   cabina y tamaño del texto. Sombras y niebla permiten comparar sus efectos.
   Cambiar km/h a mph debe convertir velocidad y límite del HUD compacto.
3. Pulsá la asignación de **Puertas** e intentá D, que ya usa el regulador.
   Debe mostrar el conflicto y conservar Q. Probá una tecla libre, por ejemplo L.
   Después L debe operar puertas y Q debe dejar de hacerlo. La cámara exterior
   debe conservar su posición en ambos casos. F6 muestra la nueva asignación.
4. Pulsá **Guardar ajustes**, cerrá y reiniciá el visor. Deben conservarse.
   **Restablecer controles** recupera las teclas predeterminadas; volvé a guardar.

Los controles reservados de navegación no pueden asignarse a otra acción.
Una tecla de conducción tiene una sola función. Esc sigue siendo el acceso
fijo a cerrar/pausa y la tecla física Pause es un alias del mismo menú.

## 8. Ratón en cabina 3D

1. Pulsá 1. Si aparece cabina 2D, alterná con Alt+1. Mirá hacia las palancas
   del puesto; mantené el botón derecho y arrastrá para orientar la mirada.
2. Pasá el cursor por el regulador, freno e inversor originales. Abajo debe
   aparecer el nombre del control y su valor. Los instrumentos indicadores no
   deben ofrecer una acción de palanca.
3. Con el botón izquierdo arrastrá una palanca hacia arriba/abajo. Su valor en HUD
   y su animación deben cambiar; la mirada de cámara debe permanecer quieta durante
   ese arrastre.
   El regulador y el freno se controlan por separado. Para probar el inversor,
   mantené el tren detenido.
4. Pulsá interruptores que el modelo identifica como limpiaparabrisas, puertas,
   bocina o pantógrafo. Debe actuar el subsistema correspondiente. La bocina
   cambia su estado, pero permanece silenciosa por la configuración actual.
5. Un clic sobre el mapa, un menú o el DMI no debe accionar una palanca ni
   arrastrar la cámara. Al cerrar los paneles la cámara no debe saltar por
   movimientos del ratón acumulados mientras estaban abiertos.

Se usan las mallas y asociaciones CVF/ORTS originales. Los modelos que no
identifican una pieza accionable mantienen sus controles de teclado y cabina 2D;
no se inventan palancas interactivas sobre indicadores o piezas decorativas.

## Recorrido completo de aceptación

En Northolt Park esperá a que termine el embarque y aparezca autorización de
salida; cerrá puertas con Q. W selecciona adelante; pulsaciones de D aumentan el
regulador y A lo reducen. `;` suelta freno y `'` lo aplica. Acercate a las paradas
con regulador cerrado y frená hasta ≤0,1 m/s dentro de ±10 m del punto mostrado.
Abrí puertas, esperá el embarque (20 s en Northolt; 30 s en las otras estaciones)
y cerralas cuando el HUD autorice salida.

Debe completarse **Northolt Park → South Ruislip → West Ruislip**, unos 7,06 km.
El terreno, vía, edificios y señales deben seguir cargándose a lo largo del
recorrido; cámara exterior y cabina deben seguir al mismo tren. Al terminar:
**3/3 paradas**, servicio completado y evaluación con tiempos reales.

Para una comprobación visual sin conducir: `./scripts/run_chiltern_service.sh
--autodrive --cab`. El conductor automático también opera puertas y paradas;
no lo uses para probar manualmente regulador o inversor porque los sobrescribe.

La comparación física del recorrido completo con OR 1.6.1 continúa pendiente.
Los cuatro oráculos físicos breves y la prueba funcional del servicio son
comprobaciones diferentes; aprobar la interfaz no elimina esa diferencia.

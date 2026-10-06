# Prueba manual de la interfaz y de la partida

Las funciones se usan en una partida real. La referencia de contenido sigue
siendo Open Rails 1.6.1; la iluminación, el streaming y la interfaz se ejecutan en
Bevy. El sonido original SMS/WAV está habilitado y se ajusta en F10. Esta guía comprueba funcionamiento y efectos
visibles; no certifica la paridad física completa con Open Rails.

## Preparación

Desde la raíz de `openrailsrs`:

```bash
./scripts/run_chiltern_service.sh
```

El script compila y abre el menú. Si el Content está en otro directorio, definí
`CHILTERN_ROUTE` con la carpeta que contiene `WORLD`, `TILES` y `Chiltern.tdb`.
Para saltar el menú: `./scripts/run_chiltern_service.sh --direct`.

Elegí **Chiltern**, **Chiltern local con tráfico: Northolt Park → West Ruislip**,
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
   variantes Night que declara cada modelo; el sol, el cielo y la niebla deben
   oscurecerse juntos. Probá 21:55: el horizonte no debe conservar el azul diurno.
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

El mapa también muestra los servicios del escenario en violeta y las secciones
que ocupan. El despachador no crea servicios nuevos ni sesiones de multijugador.

## 7. HUD avanzado — F8; ajustes y teclas — F10

1. Abrí F8 y recorré sus diez páginas: General, Formación, Locomotora,
   Potencia distribuida, Alimentación, Frenos, Fuerzas, Despachador, Clima y
   Diagnóstico. Deben mostrar datos de la sesión, no valores de demostración.
   Diagnóstico incluye FPS, tiempo por cuadro y sectores/entidades del escenario.
   También muestra P50/P95/P99, cuadros de carga, cuadros superiores a 100 ms
   y RAM actual/máxima de toda la sesión; la memoria no disponible aparece como «—».
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
   cambia su estado y usa el sonido original si el SMS lo declara y el audio está activo.
5. Un clic sobre el mapa, un menú o el DMI no debe accionar una palanca ni
   arrastrar la cámara. Al cerrar los paneles la cámara no debe saltar por
   movimientos del ratón acumulados mientras estaban abiertos.

Se usan las mallas y asociaciones CVF/ORTS originales. Los modelos que no
identifican una pieza accionable mantienen sus controles de teclado y cabina 2D;
no se inventan palancas interactivas sobre indicadores o piezas decorativas.

## Recorrido completo de aceptación

En Northolt Park abrí puertas con Q, esperá a que termine el embarque y aparezca
autorización de salida, y cerralas con Q. W selecciona adelante; pulsaciones de D
aumentan el regulador y A lo reducen. `;` suelta freno y `'` lo aplica. Acercate a las paradas
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

## 9. Escenario, materiales y detalle a distancia

1. Elegí el servicio extendido y probá las seis estaciones con **1** en cabina y
   **2** en exterior. Aumentá y
   reducí la distancia en F10; acercate a casas, árboles, cercos y andenes.
2. Deben conservar posición y forma, con ventanas y follaje recortados por su
   transparencia. La cámara exterior no debe atravesar el terreno ni producir
   una sombra propia. Cristales y planos transparentes no deben generar manchas
   rectangulares sobre la vía.
3. Mové lentamente la cámara alrededor de una distancia de cambio de detalle.
   El modelo no debe alternar entre dos niveles por pequeños movimientos. Hay
   histéresis del 8 %. Los grupos de instancias cambian de malla con una transición
   de 0,35 s: puede verse un tramado breve, pero no deben desaparecer ni duplicar
   su sombra. Los objetos sin instancias conservan el cambio discreto con histéresis.
4. Conducí hasta Gerrards Cross. Edificios, vía y árboles deben seguir apareciendo;
   no debe quedar sólo terreno vacío al superar el sector inicial.
5. Las seis referencias originales están en
   `docs/fixtures/visual/or_reference/chiltern_station_views/`. Compará desde un
   encuadre equivalente; para cabina usá 45° en F10. Bevy conserva su iluminación
   y tonemapping. Algunas construcciones del Content tienen reversos abiertos
   también en OR; no se les agregaron edificios o pisos inventados.

## 10. Noche, faros, lluvia y limpiaparabrisas

1. Elegí **21:55**, Verano y Despejado. En exterior, mirá hacia arriba: deben
   verse estrellas sobre cielo oscuro. El terreno y las casas deben oscurecerse
   junto con el cielo. Las ventanas con textura Night pueden quedar iluminadas.
2. **H** alterna faros apagados, bajos y altos. En cabina, sobre una recta,
   compará la vía frente al tren: altos deben iluminar los rieles y apagados
   deben quitar el haz. Las lámparas siguen las condiciones del archivo original;
   el Pullman declara sus luces blancas delanteras para altos y marcadores rojos
   para bajos. Mové la cámara exterior: la luz debe seguir la
   locomotora, sin actuar como una linterna de cámara.
3. **I** enciende la luz de cabina. El interior debe quedar más legible de noche,
   mientras los instrumentos luminosos conservan su iluminación. H e I también
   actúan mediante controles originales que declaren HEADLIGHT/CABLIGHT en CVF.
4. Iniciá con Lluvia y volvé a cabina. Deben caer gotas afuera y acumularse gotas
   con refracción en el parabrisas, sin cubrir el tablero ni los marcos opacos.
   **V** debe mover las escobillas originales y despejar sus arcos. Al apagarlo,
   los sectores limpiados vuelven a mojarse gradualmente.
5. Pausá: precipitación, gotas y escobillas deben detenerse con el reloj. En
   exterior no debe aparecer el efecto de gotas sobre la pantalla. Con lluvia
   o niebla las estrellas no deben verse. Guardar/cargar conserva H, I y V.
6. Con Niebla, compará F10 → **Modelo de niebla**: Atmosférica, Volumétrica 32 y
   Volumétrica 64. Probá faros altos de noche y sol bajo de día. La volumétrica
   concentra densidad cerca de la vía y participa en la iluminación de faros;
   el costo aumenta con los pasos. Guardá ajustes para conservar la elección.
7. Elegí una hora próxima al amanecer o atardecer. El horizonte debe tomar un
   tono cálido y el cielo cambiar gradualmente, sin un salto al cruzar la puesta
   del sol. En Despejado diurno aparecen nubes altas; Lluvia y Niebla usan un
   cielo más cubierto. En Despejado nocturno deben volver a verse las estrellas.

El cielo estrellado es procedural y repetible, sin reproducir un catálogo
astronómico de OR. El limpiado del vidrio usa una proyección aproximada de dos
escobillas; modelos con otra disposición pueden requerir perfiles específicos.
El cielo y la niebla comparten el color del horizonte. El volumen local pierde
densidad con la altura y en sus bordes; no debe dibujar una pared negra rectangular.

## 11. Fluidez y memoria durante todo el viaje

1. Iniciá una sesión nueva del servicio extendido y completá las seis paradas.
   F8 → Diagnóstico conserva
   mediciones de toda esa sesión aunque cierres el panel.
2. Compará las mismas condiciones con 500, 2000 y 4000 m de distancia de carga.
   Anotá P95/P99, máximo por cuadro y pico de RAM. El parseo de modelos y la
   decodificación ACE se ejecutan en segundo plano; la construcción principal
   se reparte con un presupuesto de 4 ms por cuadro. Un recurso grande puede
   superar ese presupuesto porque no se interrumpe a mitad de construcción.
3. La RAM debe estabilizarse al descargar sectores anteriores; no debe crecer
   con cada captura o visita. El histograma tiene tamaño fijo. No compares los
   FPS de lavapipe con los de una GPU dedicada.

Para medir el viaje en tu GPU y rechazar automáticamente un renderer por CPU:

```bash
python3 scripts/check_viewer_streaming.py --route-root "$CHILTERN_ROUTE" \
  --scenario examples/chiltern_traffic/scenario.toml --checkpoint terminal \
  --require-hardware --timeout-s 480 --out-dir tmp/hardware-journey
```

El informe conserva el dispositivo, los percentiles, el pico de RAM, las paradas,
los sectores activos y el estado de los shaders. Usá el mismo alcance y reloj
al comparar versiones; la compilación inicial de shaders puede producir tirones.

## 12. Tráfico ferroviario en vivo

1. Seleccioná **Chiltern local con tráfico**. Abrí F4/M: el Pullman adelantado
   debe ocupar secciones y mantener la primera señal en Alto mientras está
   delante. Al atender South Ruislip y continuar, las secciones se liberan.
2. Atendé Northolt y esperá la salida de las **10:00**. `+` acelera el reloj y
   `−` lo reduce. F7 muestra los horarios. El servicio sin tráfico anterior
   conserva su horario más corto.
3. Entre South Ruislip y West Ruislip debe pasar otro Pullman en la vía contigua.
   Su salida es a las **10:06:40**; no aparece antes. Sus coches, bogies, ruedas,
   puertas y luces deben seguir ese tren. M identifica los dos servicios.
4. Guardá antes del cruce, avanzá y restaurá. El jugador y los dos servicios
   deben volver juntos al estado guardado. Pausa congela todos.
5. Completá el recorrido: el jugador debe terminar con **3/3**; el adelantado
   atiende dos estaciones y el contrario una. El adelantado deja libre la
   terminal, sin quedarse bloqueando al jugador. El horario AI es de demostración.

## 13. Validez de todas las formaciones instaladas

1. Recorré **Formación** en el menú. Debajo aparece el estado de cada una.
   Las que tienen archivos faltantes o incompatibles deben explicar el motivo
   y rechazar Iniciar; las no motorizadas se identifican como material estático.
2. Una formación utilizable debe cargar todos sus vehículos, respetar Flip y
   usar cabina original 2D/3D cuando existe. Sin cabina declarada, el visor puede
   ofrecer su cabina genérica y lo informa como advertencia.
3. Para obtener el diagnóstico de todo el Content:

```bash
target/debug/openrailsrs audit-consists \
  "/ruta/Content/Chiltern/TRAINS/CONSISTS" --json > tmp/formaciones.json
```

Este control verifica parámetros, rutas ENG/WAG, modelos, archivos de texturas
y gráficos de cabina. No garantiza que cualquier locomotora tenga física,
sonidos o scripts C# equivalentes a Open Rails.

Probá estas seis formaciones originales desde el menú: **Bristol Pullman** y
**121single** (diésel), **1960CentralWR8Car** y **R Stock 6 Car** (eléctricas),
**Downton Hall LE** y **KingLE** (vapor). Deben aparecer 8, 1, 8, 6, 2 y 2 vehículos,
respectivamente. En el Pullman, **1** abre cabina 3D y **Alt+1** alterna 2D/3D.
Las otras cinco declaran cabina 2D: usá **Alt+1** para comprobar su panel original.
Las flechas izquierda/derecha cambian entre los puntos de vista CVF disponibles.
El ojo debe seguir Position/Direction de cada vista, manteniendo el paisaje a
la altura del puesto; en Hall las palancas deben quedar sobre el panel.
Revisá el velocímetro al avanzar, las palancas al mover regulador/freno y ruedas,
bogies y luces desde **2**. Sin una cabina 3D declarada no se genera una réplica
3D de la cabina 2D. Los controles luminosos y faros dependen de cada ENG/CVF.

El oráculo de recursos y capturas puede repetirse con un solo renderer por vez:

```bash
cargo build --locked -p openrailsrs-audio --example native_oracle
python3 scripts/check_rolling_stock.py --route-root "$CHILTERN_ROUTE" \
  --audio --out-dir tmp/rolling-stock
```

## 14. Sonido original SMS/WAV

1. Iniciá el Pullman, abrí F10 y activá **Sonido original**, con volumen 40 %.
   En F8 → Diagnóstico deben aparecer programas SMS, muestras y salida activa.
   Si no hay dispositivo o falta un archivo, el diagnóstico informa el problema.
2. Con el tren detenido, escuchá el ralentí. Cerrá puertas con **Q**, poné el
   inversor adelante y aumentá el regulador: debe cambiar el sonido del motor.
   Al ganar velocidad debe sumarse el rodaje de los coches según sus curvas SMS.
3. Mantené **Space** dos segundos y soltá. La bocina debe iniciar una vez,
   mantenerse y terminar al soltar; no debe reiniciarse en cada cuadro.
4. Alterná **1**, **Alt+1** y **2**. Cabina y exterior deben usar sus programas
   originales, sin recargar todas las muestras al cambiar de cámara. Alejá la
   cámara: cada coche debe atenuarse con su distancia. El otro servicio debe
   escucharse al aproximarse y alejarse, según sus archivos de sonido.
5. Aplicá y soltá el freno, y alterná puertas y limpiaparabrisas. Deben sonar
   los eventos que declare el SMS; no todos los modelos tienen cada efecto.
6. Pausá: el sonido debe detenerse. Reanudá y restaurá una partida anterior:
   los bucles deben continuar o reiniciarse de acuerdo con el estado restaurado.
   F10 permite silenciar y ajustar volumen; guardá ajustes para conservarlos.
7. Repetí cabina/exterior con los dos eléctricos y las dos locomotoras de vapor.
   King debe mantener sonidos de marcha fuera de la cabina. Con varios coches
   sonando y la bocina activa, la mezcla debe limitar sus picos sin recortar
   bruscamente las muestras. El ensayo WAV comprueba señal y cero saturación.

Para escuchar una demostración del mismo motor de audio sin abrir un dispositivo:

```bash
target/debug/examples/native_oracle "/ruta/TRAINS/CONSISTS/Bristol Pullman.con" \
  "$CHILTERN_ROUTE" tmp/pullman.wav exterior
```

El runtime admite curvas de volumen/frecuencia, bucles WAV con introducción y
salida, y eventos de conducción. No equivale todavía al motor completo de OR:
algunas variables, filtros y scripts específicos de contenido siguen pendientes.

Las variables de diésel conservan la escala 0–1; las eléctricas y de vapor
usan los porcentajes esperados por sus SMS. Para las dos últimas, la demanda
del regulador aproxima carga/presión; faltan sus variables físicas completas.

## 15. Embarque, horario y práctica rápida

1. Abrí el servicio corto **Chiltern local con tráfico**, quedate detenido en
   Northolt Park y abrí las puertas con **Q**. El HUD debe distinguir **pasajeros**
   (20 s) y **horario** (hasta 300 s en esta salida). Cuando termina el embarque
   debe decir **Pasajeros listos**, aunque aún falte la hora de salida. F7 muestra
   ambos tiempos por separado.
2. Cerrá las puertas. Podés esperar con puertas cerradas; no debe reiniciarse el
   embarque. Si salís antes del horario, el tren puede moverse y la evaluación de
   F7 registra una salida anticipada. Las puertas abiertas siguen cortando la
   tracción en la implementación actual.
3. Para probar sin esperas largas, **F10 → Práctica rápida en estaciones → Sí** y Esc.
   Abrí las puertas: una parada requiere como máximo **5 segundos de pasajeros**
   y permite salir sin esperar al horario. Cerrá las puertas y conducí normalmente.
   Se mantienen las posiciones, señales y paradas. F7 identifica la práctica.
4. Desactivá la práctica y reiniciá con **R** para volver al servicio normal.
   **Guardar ajustes** conserva la preferencia al volver a abrir el visor.

Open Rails 1.6.1 calcula el embarque y además espera la salida programada. No
inmoviliza universalmente al jugador por ese contador: salir antes afecta la
actividad; los enclavamientos dependen del vehículo. Aquí se quitaron el bloqueo
artificial del regulador por el temporizador y el mensaje que confundía las dos
esperas. El modo de práctica es una comodidad adicional, activada explícitamente.

## 16. Puesto de conducción por cabina

1. Elegí Pullman, **1 → Alt+1** para la cabina 3D y abrí F10. Probá **Altura del
   asiento** y **Asiento 3D hacia atrás**: al cerrar el panel el puesto se mueve
   respecto del escritorio. El límite de cada ajuste es ±0,40 m.
2. Guardá los ajustes, cerrá y abrí el juego: deben conservarse para esa cabina.
   Otra formación con un CVF distinto tiene su propio ajuste. **Restaurar puesto y
   barrido originales** devuelve los valores de ese perfil.
3. Conduciendo, el amperímetro/medidor de carga debe responder a la tracción:
   al quitar potencia su lectura baja aunque el motor conserve RPM. La presión
   de una cabina de vapor debe usar la caldera y las unidades del instrumento.
4. En clima **Lluvia**, probá **V** en cabinas 2D y 3D. Las gotas quedan en el
   vidrio y el barrido despeja su área. En 2D se usa la ventana definida en el CVF,
   conservando las bandas del panel al cambiar la relación de aspecto. F10 permite
   ajustar el alcance del barrido por cabina.

El barrido usa huesos WIPER cuando existen; en cabinas clásicas su área se
aproxima. El medidor eléctrico aún usa una estimación de carga, no un modelo
completo de corriente/tensión. Las cabinas 2D conservan su punto de vista original.

## 17. Humo, vapor y superficies mojadas

1. Elegí **Pullman**, vista exterior **2**. Con el motor funcionando observá las
   salidas de escape: el humo sale del punto definido por el ENG y aumenta con la
   potencia. Al avanzar debe quedar atrás del tren.
2. Elegí **Downton Hall LE** o **KingLE**: observá la chimenea, las purgas cerca de
   los cilindros al arrancar y el vapor al usar la bocina. La pausa congela el
   efecto. Las partículas no deben proyectar sombras ni seguir a la cámara.
3. Compará el mismo lugar en **Despejado** y **Lluvia**. El tren, los materiales
   opacos individuales y el terreno deben oscurecerse ligeramente; los materiales
   PBR adquieren más brillo y menos rugosidad. Cambiar lluvia/despejado en una
   partida restaurada permite observar el mojado y secado progresivos.

Se usan emisores originales y partículas acotadas a 512, cercanas a la cámara.
La intensidad es visual; no certifica la termodinámica completa de Open Rails.
Los objetos renderizados con el material específico de instancias conservan su
acabado original; los cambios PBR se aplican al tren y a objetos individuales.

## 18. Seis estaciones hasta Gerrards Cross

1. En el menú elegí **Chiltern extendido: Northolt Park → Gerrards Cross**,
   **birmingham_pullman** y **Recorrido del servicio**. F7 debe enumerar
   Northolt Park, South Ruislip, West Ruislip, Denham, Denham Golf Course y
   Gerrards Cross. Comienza con freno aplicado para sostener la pendiente; soltalo
   después de embarcar, cerrar puertas y elegir el sentido. F10 activa la práctica de 5 s.
2. Conducí y completá las seis paradas. El trayecto mide unos **15,3 km** desde
   Northolt Park. El mapa y el paisaje deben acompañar el avance después de West
   Ruislip, incluyendo las tres estaciones adicionales.
3. Observá el servicio adelantado y el contrario en F4/mapa. El adelantado ocupa
   estaciones y afecta señales; el contrario circula por el corredor contiguo.
4. En Gerrards Cross, frená dentro de ±10 m a ≤0,1 m/s, abrí puertas, completá el
   embarque y cerralas. Debe aparecer **Servicio completado** y F7 mostrar 6/6.

Para abrirlo directamente: `./scripts/run_chiltern_service.sh --direct`.
La referencia geométrica son el PAT local original y las plataformas/conexiones
TDB hacia Gerrards Cross. El horario es propio del escenario; no reproduce una
actividad nativa completa. El servicio extendido utiliza los scripts SIGSCR originales de Chiltern; los escenarios históricos conservan las reglas de tres aspectos.

## 19. Cambios de detalle y memoria

1. En exterior acercá/alejá despacio la cámara alrededor de edificios rígidos.
   Las mallas de un LOD deben sustituirse con una transición breve (0,35 s). No
   debe aparecer un objeto duplicado permanente ni cambiar de lugar una señal.
2. Durante el recorrido largo probá cabina y exterior, con distancia de dibujo
   de 2000 m. Los sectores cercanos deben estar completos al detenerse; los
   distantes se liberan al avanzar. No abras dos visores para esta prueba.
3. Para dejar evidencia reproducible, usá el comando de streaming documentado en
   `examples/chiltern_extended/README.md`. Produce PNG, memoria máxima, tiempos
   de cuadro, adaptador utilizado y sectores pendientes.

Las transiciones comparten materiales y texturas y admiten como máximo 64 mallas
salientes. Los objetos animados conservan su animación y el cambio discreto; las
instancias usan su ruta de detalle existente. Los resultados con lavapipe/CPU
certifican carga y memoria en esa configuración; los FPS de una GPU deben medirse
con `--require-hardware` en una máquina con el controlador funcionando.

## 20. Límites, pendientes y frenos nativos

1. En el servicio extendido abrí **F4**: los límites por cartel aparecen en su
   distancia real. El HUD cambia al alcanzarlos; un aumento espera a que pase
   también la cola de la formación.
2. Abrí **F8 → Fuerzas**: la pendiente debe cambiar con los tramos nativos de la
   vía. A igual regulador el tren responde de forma distinta en subida/bajada.
3. Aplicá y soltá freno. El Pullman original combina **siete vehículos EP** con
   **un motor de cola de aire**. Los EP responden sin espera de propagación;
   el motor de cola conserva la propagación y retención del aire. Las tasas de
   aplicación/liberación son 30/10 PSI/s para EP y 40/40 PSI/s para la cola.
   La lectura de cilindro utiliza la presión de plena fuerza del vehículo
   (45, 90 o 70 PSI según su ENG/WAG), dentro del modelo de freno simplificado.
4. Con puertas cerradas, pasar el límite no debe quitar automáticamente la
   potencia de una locomotora nativa: la conducción y sus consecuencias siguen
   siendo responsabilidad del jugador. Frená y verificá el aviso de exceso.

Estos perfiles se incluyen en el escenario extendido. Los escenarios históricos
sin perfiles mantienen sus pendientes/límites declarados. La importación nativa
produce perfiles para ambos sentidos. Los oráculos congelados de Open Rails
siguen siendo la referencia física; no se amplían sus tolerancias.

## 21. Nieve y materiales del escenario

1. Iniciá el servicio extendido con **Clima → Nieve**. La cobertura visual funciona también en otras estaciones; invierno conserva sus variantes estacionales normales en los edificios.
2. En exterior (`2`), observá copos con deriva lateral y nieve en el terreno y en superficies del tren. Los edificios completos, incluidos sus techos, conservan las texturas originales; no reciben cobertura ni variantes Snow. Los árboles mantienen sus recortes.
3. En cabina (`1` o `Alt+1`), observá copos adheridos al vidrio. Pulsá `V`: el barrido limpia el sector correspondiente. El tablero, los instrumentos y el HUD no reciben el efecto del vidrio.
4. Repetí con lluvia: el terreno y los edificios agrupados en GPU se oscurecen y reciben reflejos discretos. Las ventanas/recortes mantienen su transparencia. Con tiempo despejado los acabados vuelven a sus valores originales al iniciar otra partida.
5. Guardá y cargá la partida con nieve; el clima debe conservarse. Acelerá/pausá: copos y escobillas usan el reloj de simulación.

La cobertura ya representa un escenario nevado al cargar. No se simula hielo/adhesión ni temperatura. En nieve se reduce la visibilidad a 500 m como el ajuste inicial de Open Rails 1.6.1.

## 22. Señales originales de Chiltern y calidad del recorrido

1. Seleccioná el servicio extendido. Abrí `F4` y `M`: el adelantado ocupa el corredor y los aspectos responden a su posición. Los semáforos de dos estados conservan sus posiciones originales, y los distantes advierten sobre señales normales.
2. Detenete ante alto. Cuando la cola del otro tren libere el bloque, verificá que cambia la lámpara/brazo y coincide con F4. Una orden de vía libre no permite ignorar un bloque ocupado.
3. Completá las seis estaciones; comprobá edificios, andenes, árboles y cercos desde cabina y exterior. Las posiciones de los objetos deben permanecer estables al cambiar de cámara o detalle.
4. Durante la pantalla de carga, el tren y los contadores de pasajeros/horario deben quedar detenidos. Al terminar, la partida comienza desde su hora inicial.
5. En `F8 → Diagnóstico`, observá tiempos de cuadro y RAM durante el viaje. El JSON de las capturas distingue el máximo de arranque y los tirones durante juego; no uses FPS de lavapipe como rendimiento de una GPU.

Para capturar las seis estaciones en ambas vistas con GPU real:

```bash
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/radeon_icd.json WGPU_BACKEND=vulkan \
python3 scripts/capture_route_views.py --route-root "$CHILTERN_ROUTE" \
  --with-cab --headless-wayland --require-hardware --out-dir tmp/six-station-views
```

El intérprete reconoce las funciones normal/distante utilizadas por esta ruta. No sustituye todavía las reservas, enlaces de todos los cruces ni los scripts C# del original. Las capturas se comparan por composición, geometría y transparencia; los motores conservan su iluminación/tonemapping propios.

## 23. Copos, modos de cálculo y renderizador

En el menú de inicio elegí Clima → Nieve y recorré cabina/exterior. Debés ver copos con tamaños y
formas variadas, movidos por el viento, que pasan junto al tren sin seguir a
la cámara. Bajo un techo cerrado debe disminuir la precipitación; es una máscara
conservadora de alturas. El terreno y las superficies del tren acumulan cobertura
irregular; los edificios conservan el material, el vidrio tiene manchas y V despeja el barrido.

Alterná Cálculo del clima entre GPU, CPU y Mixto. En F8 → Diagnóstico deben
cambiar los conteos de cada modo. GPU mantiene las semillas sin subir posiciones
cada cuadro; CPU tiene un límite menor. Automático puede reducir detalle tras
presión sostenida y recuperarlo con demora. RSS y VRAM son contadores distintos;
si el sistema no puede medir VRAM aparece ausencia, no cero.

Elegí Renderizado al iniciar CPU, cerrá y reiniciá. El adaptador debe figurar como
software; la partida conserva controles y escenario, con muchos menos cuadros
por segundo. Volvé a GPU/Auto y reiniciá para recuperar aceleración.
No se cambia de dispositivo en caliente. [Comandos y mediciones](WEATHER_EXECUTION.md).

## 24. Regresiones visuales de cabina y formación

Con los assets Chiltern instalados, ejecutá
`python3 scripts/check_visual_goldens.py --route-root "$CHILTERN_ROUTE" --prove-faults`.
Revisá las siete imágenes y `report.json`: frente, arriba, izquierda, derecha,
cabina 2D, chase y orbit. Texto e instrumentos deben tener orientación correcta,
las ventanas mostrar el recorrido, no debe haber un objeto tapando la cámara y
el exterior debe mostrar la formación. Los cuatro fallos intencionales deben
estar detectados. [Poses, máscaras y tolerancias](fixtures/visual/player_goldens/README.md).

## 25. Host opcional C# TCS

Ejecutá `bash scripts/check_tcs_host.sh` con SDK .NET 10. Debe informar PASS para
compilación, ACK, menú, límite de 18 km/h, frenado y rechazo de restauración.
En una locomotora con DMI ETCS, seleccioná explícitamente el fixture como indica
[TCS_CSHARP_HOST.md](TCS_CSHARP_HOST.md): mensaje de confirmación e intervención,
reconocimiento que libera el freno y nueva intervención al superar el límite.
El Pullman no tiene DMI ETCS; allí la aceptación se hace con el fixture headless.
La partida habitual usa el TCS Rust y no necesita .NET.

## 26. Web del proyecto

Abrí https://cavazquez.github.io/openrailsrs/. Debés ver la nueva portada con
captura real y seis estaciones. Seleccioná Gerrards Cross: cambia el texto de
llegada. En La experiencia, Cabina/Exterior/Noche/Nieve deben cambiar imagen y
leyenda. En Empezar, Copiar copia el comando o lo selecciona si el navegador
impide acceder al portapapeles. En móvil, Menú muestra navegación y Escape cierra
el menú. Las páginas Estado, Física y Referencia OR muestran el alcance real y
la comparación física completa que todavía falla.


## 27. Hora real, clima del lugar y tormentas

1. En el menú dejá **Hora visual** y **Origen del clima → Elegido por el jugador**.
   Elegí hora de salida 09:55 y **Clima manual / respaldo → Nieve**: se ve nieve
   y el sol corresponde a esa hora. No debe consultarse internet.
2. Activá solamente **Hora visual → Actual del lugar**. En Chiltern, el aviso
   inferior y **F8 → Clima** deben mostrar fecha/hora de **Europe/London**, aunque
   tu equipo esté en Argentina. La nieve elegida se conserva. Compará la hora
   londinense teniendo en cuenta horario de verano. El HUD de conducción sigue
   mostrando el reloj del servicio, separado del reloj del lugar.
3. Pausá con P, esperá unos segundos y volvé a **F8 → Clima**: avanza la hora
   real; no avanzan tren, pasajeros ni horario. Acelerá con +: la hora real
   mantiene su ritmo. Volvé a manual en F10: vuelve la iluminación de la partida.
4. Activá solamente **Origen del clima → Actual del lugar**. F8 debe mostrar
   Open-Meteo, dato UTC y antigüedad, temperatura, nubosidad, viento y coordenadas.
   La hora visual manual se conserva. El clima puede tardar unos segundos en
   llegar; el aviso muestra “consultando”. Lluvia/nieve/tormenta dependen del dato
   actual: para probar esos efectos siempre podés elegirlos manualmente.
5. En F10, pulsá **Elegir clima** hasta **Tormenta** y continuá la partida. Debe
   pasar a manual, llover y verse cielo cubierto. Tras unos cuatro segundos de
   simulación aparece un rayo con ramas y un destello; después se oye el trueno.
   Los eventos siguientes tardan 25–55 s. Alterná 1/2: audio más tenue en cabina,
   rayos fijos en el escenario, gotas y limpiaparabrisas V. Pausar debe congelar
   el efecto y pausar el sonido, sin repetir el trueno al continuar.
6. **F10 → Rayos y destellos → No** debe quitar los destellos y rayos. El trueno
   sigue controlado por sonido/volumen. Elegí **Despejado**: desaparecen lluvia y
   tormenta, y se descartan truenos pendientes. Mové la cámara: ningún rayo debe
   seguirla ni crear una sombra a sus pies.
7. Activá ambos modos actuales, guardá la partida, cerrá y reanudá. F8 conserva
   ambos selectores y resuelve la fecha actual; el horario, posición y tráfico
   guardados no saltan al horario real. Elegí manual y guardá ajustes para que
   el próximo menú recuerde esa preferencia.
8. Probá con la conexión deshabilitada antes de iniciar: debe usar el clima de
   respaldo y avisar el problema, permitiendo conducir. Si ya existe caché, la
   hora real mantiene su zona y el último clima válido informa su antigüedad.
   Datos de más de dos horas vuelven al respaldo. Una ruta de práctica sin
   coordenadas debe informar “ruta sin ubicación” y continuar manual.

Los datos actuales son estimaciones de un modelo meteorológico. Los rayos son
procedurales; no reproducen descargas reales observadas. Detalles, límites y
comandos: [LIVE_ENVIRONMENT.md](LIVE_ENVIRONMENT.md).

## 28. Luz interior, ejemplos y DDS

1. Elegí hora manual nocturna y tiempo despejado. Desde cabina 3D (`1`), pulsá
   `I`: tablero e interior deben aumentar su brillo. El HUD indica
   «Luz cabina Sí/No». Volvé a pulsar `I` para apagar. F6 muestra la asignación y
   F10 permite cambiarla sin conflictos. La 2D usa sus texturas nocturnas; la
   luz interior de la cabina 3D no es un foco proyectado hacia el exterior.
2. Iniciá desde el menú y recorré Ruta/Servicio. Deben aparecer los escenarios
   completos de `examples/`, incluidos SCE multi body y Retiro → Olivos de
   `mitre_campaign/scenarios/`. Los archivos de reportes/overlays/campañas/horarios
   no son partidas seleccionables. Los ejemplos sin paisaje nativo son escenas
   de práctica, no recreaciones visuales completas de Argentina. Si la formación
   figura «Incompleta», hay que instalar sus modelos/texturas/cabina antes de
   iniciar en 3D. Mitre CAF 6000, por ejemplo, aún carece de `caf6000_motor.s`.
3. Con la misma estación, hora y cámara, compará despejado y nieve. El suelo
   y los copos cambian; las fachadas y techos conservan sus colores/texturas.
   Probá también alejándote para activar LOD y cargando un sector nuevo.
4. Seguí [GPU_TEXTURES.md](GPU_TEXTURES.md): exportá una ACE o una carpeta a
   `tmp/`, revisá el informe y comprobá que las ACE originales siguen intactas.
   Compará carga automática y `OPENRAILSRS_TEXTURE_UPLOAD=rgba`: la formación,
   la vegetación y los edificios deben conservar su apariencia.

## 29. Señales, compatibilidad, C# y peralte

1. Elegí **Chiltern extendido** con tráfico. En F8 → **Despachador**, mirá los
   intervalos de reserva propios y de otros servicios. Al acercarte a una señal
   normal, el siguiente bloque puede quedar reservado. Al pasar cambia la
   reserva; la cola continúa ocupando la vía. Guardá, cargá y comprobá los
   aspectos y el tráfico. El test automatizado `track_reservations` verifica
   además dos trenes opuestos intentando entrar al mismo bloque vacío: sólo
   uno recibe concesión y el otro ve Alto. No es el planificador de cruces OR.
2. En el mapa/despachador ordená **Alto** a una señal normal que tenga otra
   anterior de varias indicaciones. Debe mostrar Alto y la anterior debe pasar
   a su aspecto nativo de advertencia. Restaurá la orden; no debe liberar una
   vía ocupada o reservada por otro tren. Algunas señales de dos aspectos no
   tienen una indicación amarilla; el resultado debe seguir su SIGSCR original.
3. En el menú seleccioná formaciones. Los modelos/cabinas faltantes impiden
   iniciar. Ahora aparecen avisos de sistemas parciales y C#. Un sonido
   opcional faltante permite iniciar con aviso. Para ver el detalle por coche:

   ```bash
   target/debug/openrailsrs audit-consists /ruta/TRAINS/CONSISTS --json
   ```

   Revisá `report.compatibility`: motor, frenos, scripts, sonidos, trocha y
   déficit de peralte. La auditoría no ejecuta C# automáticamente.
4. Durante una curva nativa, abrí F8 → **Locomotora**. Deben aparecer radio en
   metros, peralte en milímetros, trocha, velocidad de confort y aceleración
   lateral. El aviso cambia si superás la velocidad de confort. Puede haber
   peralte cero en el contenido original. En rectas o sin geometría nativa
   aparece «Sin curvatura nativa disponible en este tramo». El cálculo usa la
   curva de la cabeza y no aplica frenos ni penalizaciones. El peralte generado
   se controla ahora desde F10; ver la sección 30.
5. Con el SDK .NET, ejecutá la aceptación completa:

   ```bash
   OPENRAILSRS_DOTNET=/ruta/dotnet bash scripts/check_tcs_host.sh
   ```

   Deben pasar ACK/menú/freno, ocho aspectos, índices, postes y máxima del tren.
   La última línea debe confirmar `native SIGSCR -> indexed C# aspect ->
   physical train brake on Chiltern`. Para probar el fixture nuevo en ventana:

   ```bash
   dotnet build tools/or-tcs-host -o tmp/or-tcs-host
   OPENRAILSRS_TCS_HOST_DLL="$PWD/tmp/or-tcs-host/OrTcsHost.dll" \
   OPENRAILSRS_TCS_SCRIPT="$PWD/docs/fixtures/tcs/NativeLookaheadTcs.cs" \
   OPENRAILSRS_TCS_TYPE=NativeLookaheadTcs ./scripts/run_chiltern_service.sh
   ```

   Una señal normal a Alto debe aplicar freno. Este fixture frena de forma
   deliberadamente conservadora; no es un sistema ETCS operativo. El Pullman
   no tiene DMI ETCS: verificá el freno en el HUD. La carga de partidas con host
   C# sigue dando un error explícito porque el script no implementa persistencia.

El [alcance detallado](SIGNALS_AND_CONTENT_SCOPE.md) incluye límites del API,
reservas y el comando para reproducir el oráculo de peralte sin modificarlo.

## 30. Peralte, despachador, guardado C# y contenido oficial

### Curvas y peralte

1. Abrí **F10 → Peralte automático**, activalo y guardá los ajustes. Volvé al
   menú e iniciá una partida nueva de Chiltern. La selección se aplica al cargar
   la geometría: no cambia una vía ya construida durante la partida.
2. En una curva, abrí **F8 → Locomotora**: radio, roll y peralte deben cambiar de
   forma progresiva al entrar/salir. Con `2`, acercá la cámara al bogie: ruedas y
   rieles deben conservar el contacto mientras la vía se inclina. Con `1`, la
   cámara acompaña la inclinación del tren. Agujas y vías múltiples se excluyen;
   un peralte cero en un tramo excluido es esperable.
3. Guardá la posición y compará otra partida con generación desactivada. También
   podés iniciar con `OPENRAILSRS_SUPERELEVATION=0`. Se conserva cualquier roll
   escrito en el contenido. Las texturas de vía, edificios y árboles deben
   mantener sus colores; al cambiar LOD no debe volver la vía a una pose plana.
4. Para comprobar el algoritmo sin conducir hasta una curva:

   ```bash
   cargo test -p openrailsrs-sim --test cant_profiles_oracle -- --nocapture
   cargo test -p openrailsrs-bevy-scenery automatic_cant_contact -- --nocapture
   ```

   Deben pasar los diez perfiles C# originales con tolerancias fijadas y el
   contacto geométrico de vía/pose por debajo de 1 mm. El límite local por tipo
   de tren aún no reemplaza la velocidad de diseño usada para generar cant.

### Agujas y tráfico

1. Elegí **Chiltern extendido** con tráfico y abrí **F8 → Despachador**. Deben
   aparecer reservas, bloqueos propios/ajenos, dueños de espera y número de
   desvíos. En **M**, intentá cambiar una aguja reservada: muestra el motivo y
   conserva su posición. La posición es compartida con los servicios AI.
2. Al atravesar una aguja, observá la cola: el bloqueo permanece hasta que
   la formación completa sale de su zona. Una sección desacoplada estacionada
   sigue ocupando la vía. Guardá/cargá y comprobá que no se libera una autoridad
   sólo por haber reanudado la partida.
3. El desvío se busca después de cinco segundos de espera cuando existe una
   alternativa libre hacia las estaciones pendientes. Una vía única sin
   alternativa conserva la restricción; no se espera una reversa automática.
   La reproducción controlada del cruce y de los conflictos de guardado es:

   ```bash
   cargo test -p openrailsrs-sim track_reservations -- --nocapture
   ```

### Guardado de un TCS C#

1. Ejecutá `OPENRAILSRS_DOTNET=/ruta/dotnet bash scripts/check_tcs_host.sh`.
   Debe comprobar el host real, reinicio con ACK/límite conservados y un restore
   corrupto rechazado sin cambiar la sesión. Incluye la prueba de freno físico
   desde una señal Chiltern.
2. Para abrir una partida con persistencia, compilá el host y elegí el fixture
   **MinimalTcs.cs** de la sección 29 en lugar de NativeLookaheadTcs.cs, con
   `OPENRAILSRS_TCS_TYPE=MinimalTcs`. En una cabina con DMI, reconocé el mensaje,
   guardá, cerrá y reanudá con los mismos parámetros del host. Deben conservarse
   reconocimiento, mensajes y límite. El Pullman no tiene DMI ETCS: la prueba
   de aceptación permite observar esas salidas sin depender de su cabina.
3. Cambiar los bytes del script o elegir otro tipo debe rechazar cargar esa
   partida con un diagnóstico de identidad. Un script sin ambos hooks rechaza
   guardar su estado; no se simula que se preservó su memoria. El modo Rust
   habitual continúa guardando sin requerir .NET.

### Descargar contenido

1. En el menú pulsá **Descargar contenido oficial**, seleccioná **Demo Model 1**
   y **Buscar actualización e instalar**. Debe mostrar autor, origen y tamaños; progreso en
   MiB, extracción y preparación. El menú sigue respondiendo mientras trabaja.
2. Cancelá una descarga: debe informar cancelación y volver a ofrecer el botón
   de instalar. Las rutas previamente instaladas siguen disponibles. Reintentá;
   si el ZIP ya se descargó completo, se audita esa copia sin otra descarga.
3. Cuando termine, volvé al menú y elegí la ruta **SCE** y una **Actividad
   oficial**. La formación debe mostrar su auditoría antes de iniciar y el
   visor debe utilizar su propio escenario. Chiltern conserva la versión que
   ya tenías. Algunos formatos de actividad o sistemas todavía pueden mostrar
   limitaciones del importador; instalar no certifica su comportamiento entero.
   Seleccioná **MT_MT_Class 47 & 6 mk2 PP**: debe indicar siete vehículos,
   unos 140 m y cabina 3D, avisando que la alternativa 2D falta. Iniciá y
   pulsá `1`: debés ver la cabina original; `2` muestra el tren desde afuera.
   En una actividad sin paradas programadas, el monitor debe mostrar
   la distancia al destino y «Sin paradas programadas». «Destino alcanzado»
   sólo debe aparecer cuando termine realmente el recorrido.
4. Consultá el destino mostrado en el panel: manifiesto con URL/SHA-256, licencias
   del autor, informe por formación y red importada. Un paquete comercial o
   distribuido mediante web indica que se obtiene en su origen; el botón de
   catálogo abre la página oficial. [Guía y CLI](OFFICIAL_CONTENT.md).

## 31. Actualizaciones, biblioteca del usuario y binarios

1. Seleccioná Chiltern en **Descargar contenido oficial** y buscá una
   actualización. El selector de ruta debe conservar el Chiltern anterior
   y añadir la copia del autor con **origen fecha · identificador** o
   **descarga identificador** si no hay fecha. Repetir con el mismo commit
   debe reutilizarlo. Si el autor publica otro commit, ambas copias deben
   quedar disponibles; no se sobrescribe el recorrido anterior.
2. Pulsá **Reauditar esta copia** en una instalación existente. Debe funcionar
   sin conexión; se actualizan auditoría y actividades sin descargar otra vez.
3. En Mitre elegí el CAF sintético. Debe mostrar **Escenario**, **Actividad /
   servicio**, **Formación** y los archivos faltantes. Abrí **Ver todos los
   faltantes y sus ubicaciones**: debe abrir su propia ventana; cada archivo
   debe indicar quién lo referencia
   y la ruta absoluta donde colocarlo. No debe ofrecer descargas alternativas:
   ese ejemplo no tiene un repositorio original del modelo identificado.
   Para material obtenido manualmente de su origen, conservá la estructura
   `rolling-stock/TRAINS` y elegí su `.con` original. Un archivo opcional
   ausente debe aparecer como **Falta opcional** y permitir la cabina válida.
   Tras colocar un archivo original en su destino, pulsá **Reauditar esta
   formación**: debe actualizar los faltantes sin cambiar el servicio ni la hora.
4. Ejecutá `openrailsrs content --list` desde otra carpeta con el binario
   copiado allí. Debe listar el catálogo sin necesitar `scripts/` ni el repo.
   Python 3 debe estar instalado. Cambiar de binario o directorio no debe
   perder las descargas almacenadas en los datos del usuario.
5. Para probar la ubicación de Snap sin construirlo, ejecutá con
   `SNAP_USER_COMMON=/tmp/openrailsrs-snap-test` y sin
   `OPENRAILSRS_PLAYER_DIR`. El destino debe ser
   `/tmp/openrailsrs-snap-test/openrailsrs/official-content` aunque cambie
   `SNAP_USER_DATA` entre revisiones. Es una prueba de selección de carpeta;
   el confinamiento requiere instalar el Snap real de la [guía de distribución](DISTRIBUTION.md).
6. En una formación incompleta de un paquete GitHub original del catálogo,
   los botones **Buscar en el repositorio original** y **Actualizar desde el
   repositorio original** deben apuntar a ese paquete. La búsqueda debe enviar
   solamente el nombre del faltante. Cambiar a una formación externa no debe
   asignarle el repositorio del escenario. Una formación sin origen identificado
   debe conservar el detalle de rutas y no ofrecer esos botones.

## 32. Catálogo web, biblioteca, KTX2 y paquete trasladable

1. Abrí [Contenido en la web](https://cavazquez.github.io/openrailsrs/contenido.html).
   Buscá «Chiltern» y luego un autor: deben filtrarse las tarjetas y actualizarse
   el contador. «Gratuito» e «Instalable desde el juego» deben distinguir las
   descargas manuales/comerciales. Cada enlace conserva su origen original.
2. En el menú del juego, pulsá **Abrir carpeta del escenario**. Debe abrir la
   misma carpeta absoluta que muestra el diagnóstico. En la biblioteca de
   contenido, cada copia tiene **Abrir carpeta** y conserva fecha/identificador.
   Pulsá **Copiar diagnóstico** y pegalo en un editor: debe incluir servicio,
   formación, archivos faltantes y destinos; no debe enviar datos a nadie.
3. Iniciá Chiltern corto y visitá Northolt Park, South Ruislip y West Ruislip,
   desde cabina (`1`) y exterior (`2`). Repetí a la misma hora y clima después
   de cerrar el visor. Edificios, árboles, recortes y detalle deben conservarse;
   la segunda carga puede reutilizar la caché. Para comparar sin caché, usá
   `OPENRAILSRS_TEXTURE_CACHE=off`; para RGBA, `OPENRAILSRS_TEXTURE_UPLOAD=rgba`.
   No esperes una aceleración fija del arranque completo.
4. Ejecutá dos veces `openrailsrs textures-ktx2 /ruta/a/una.ace`. La segunda
   salida debe informar `cache_hit: true`, con igual formato/mipmaps/payload.
   Borrar solo esa carpeta de caché debe regenerar derivados, conservando los
   originales. Con `--rgba`, BC1/2/3 deben conservar imagen y mipmaps en CPU.
   Probá KTX2 nativos BC/RGBA/UASTC válidos; ETC1S/BasisLZ muestra un error
   explícito. [Detalles y límites](GPU_TEXTURES.md).
5. Trasladá el paquete Linux completo a otra carpeta y ejecutá su visor desde
   una tercera carpeta vacía. Debe mostrar el menú, los ejemplos, las tildes y
   cargar shaders al entrar en una partida. No copies solo el ejecutable: debe
   conservar `share/openrailsrs/` junto a `bin/`. Python 3 sigue siendo necesario
   para descargar. Para Snap, seguí [DISTRIBUTION.md](DISTRIBUTION.md); las
   descargas deben quedar en `~/snap/openrailsrs/common/openrailsrs/` y sobrevivir
   una actualización del paquete.
6. En Demo Model 1 elegí la actividad 0930 Edinburgh–Glasgow y la formación
   original **MT_MT_Class 47 & 6 mk2 PP**. La cabina debe arrancar delante de la
   cubierta de Edinburgh, orientada hacia los túneles, como OR 1.6.1. Cambiar la
   longitud de la formación debe ajustar la cabeza, manteniendo el inicio de
   la cola del itinerario original. No implica importación completa de horarios
   o de las maniobras con inversión del PAT.
   En exterior, el suelo debe continuar debajo de la estación y las carreteras
   deben tener sus superficies y marcas originales. No deben aparecer huecos
   de cielo ni vías generadas donde el paquete ya trae el modelo original.
   Funciona con los nombres `global/` y `TILES/-11C3DCFC_y.raw` originales: no
   hace falta renombrar archivos ni mantener otra instalación de MSTS.

Belgrano CC sigue necesitando los recursos originales del autor. Tras instalarlos,
`prepare_native_pilot.py --inspect` debe informar las carpetas y faltantes reales;
el piloto exige tres estaciones nativas, formación auditada y un PAT continuo.
El ejemplo sintético Mitre no sustituye esta validación.

## 33. Escenario continuo, materiales, cámaras y actividad Demo

1. En la biblioteca elegí **Chiltern ampliado**, formación **Birmingham Pullman**,
   clima despejado y día. Recorré las seis estaciones, con cabina `1` y exterior
   `2`. Deben cargarse vía, suelo y objetos al avanzar; al volver a un sector
   deben reaparecer. La cabina, el tren y la vía deben seguir visibles después
   de varios kilómetros y al terminar, incluso al cambiar entre `1` y `2`.
   El HUD no debe mostrar recursos cercanos pendientes una
   vez estabilizada la vista. La primera carga todavía puede causar una pausa
   de unos cuatro segundos en el equipo de referencia.
2. En Northolt Park mirá los árboles desde la cabina y rodealos desde afuera.
   El roble debe conservar ramas y hojas sin el rectángulo pálido detrás.
   Ventanas y cristales deben seguir siendo transparentes. En South Ruislip y
   West Ruislip revisá también edificios, postes y vegetación. Compará las
   [capturas verificadas](PLAYER_POLISH_QA.md); no se afirma igualdad píxel a píxel.
3. En exterior rodeá la composición y observá los enganches. La cabeza debe
   estar delante del centro del primer coche y los coches deben mantener sus
   uniones sin sumarse media longitud de locomotora al cambiar de cámara.
   La órbita habitual y el campo de visión de Bevy se conservan. La prueba de
   cámaras de OR usa su encuadre de referencia de forma explícita.
4. Con **Demo Model 1** del autor instalado, elegí **0930 Edinburgh–Glasgow** y
   **MT_MT_Class 47 & 6 mk2 PP**. El monitor debe listar **Edinburgh Waverley,
   Haymarket y Linlithgow**, en ese orden. Cerrá las puertas cuando el HUD
   autorice la salida, respetá las señales y detenete en el marcador de cada
   andén. Esta edición del mapa termina en Linlithgow. La parada final nativa
   dura **600 s**; para probar rápido podés elegir la espera de práctica de
   cinco segundos. Al finalizar debe aparecer el resultado de tres paradas.
5. Para repetir las mediciones, seguí los comandos y condiciones de
   [PLAYER_POLISH_QA.md](PLAYER_POLISH_QA.md). Los informes incluyen cuadros
   durante la partida, memoria y recursos pendientes. La comparación física
   se ejecuta por separado con `openrailsrs oracle-suite --manifest
   oracles/chiltern-service.toml`: debe evaluar velocidad y distancia contra
   el registro congelado de Open Rails 1.6.1, usando sus límites publicados.

La prueba gráfica de Pullman admite `--pullman-cab-reference
docs/fixtures/compatibility/polish-2026-10-05/pullman-cab-foreground-reference.png`.
Compara seis zonas opacas de la cabina a 1280×720, FOV 60° y de día; al menos
cuatro deben conservar su imagen. También registra la posición real de la cámara
y la visibilidad de las mallas. Esta regresión comprueba que la cabina sigue
dibujándose; la comparación de cámaras y la física con OR se validan aparte.

### 33.6. Repetir la comparación física completa

Desde la raíz del repositorio, ejecutar:

```bash
python3 scripts/run_oracles.py --suite service --out-dir tmp/service-parity
```

Debe mostrar `PASS chiltern_local_native_controls`. En
`tmp/service-parity/report.json` deben aparecer cobertura `1.0`, RMS de velocidad
menor a 0,75 m/s, pico menor a 2 m/s y diferencia máxima de odómetro menor a 45 m.
También deben pasar las cinco fases con RMS menor a 1,10 m/s. No editar los
baselines ni los umbrales para hacer pasar un fallo.

Esta es una prueba automática de los mismos controles del original. Para una
partida manual, abrir el menú, seleccionar Chiltern y una formación original
auditable, y recorrer Northolt Park, South Ruislip y West Ruislip. Usar 1 para
cabina y 2 para exterior; comprobar tildes, árboles sin rectángulos claros,
edificios apoyados en el terreno, señales ancladas y continuidad de los sectores.
Al frenar, las RPM deben bajar; al liberar EP, puede quedar unos segundos de
presión antes de habilitar la tracción. Cada parada debe quedar registrada en F7.
Al terminar debe aparecer el resumen del servicio. Un manejo manual distinto
produce otra curva y no se compara con el oráculo del conductor original.

Demo Model 1: seleccionar la actividad nativa de Edimburgo–Glasgow y su Class 47
con seis coches Mk2. La porción del PAT elegida termina en Linlithgow: deben
registrarse Edimburgo Waverley, Haymarket y Linlithgow, con señales originales y
cabina 3D. F10 permite práctica de cinco segundos para probar las puertas; el
modo normal mantiene la espera final de 600 segundos declarada por el autor.

## 34. Probar las cinco mejoras del recorrido y distribución

### 34.1. Carga y fluidez de cabina

Abrí Chiltern extendido con el Pullman original, de día y despejado. Durante
la carga debe aparecer «Preparando cabina…» y luego el puesto de conducción
completo. Alterná **1 / 2** varias veces: la cabina debe conservar texturas y
agujas al regresar. Avanzá hasta Gerrards Cross y revisá RAM, VRAM y cuadros en
**F8 → Diagnóstico**. Las mediciones publicadas usan 1280×720 y radio 450 m;
otra resolución o distancia cambia la carga. Todavía puede haber tirones breves.

### 34.2. Las tres estaciones finales

En Denham, Denham Golf Course y Gerrards Cross, usá **1** y **2** para mirar la
vía, andenes, edificios, cercos y árboles. Los sectores deben estar cargados,
los objetos deben permanecer anclados y los árboles no deben mostrar rectángulos
opacos alrededor del follaje. En Denham Golf Course se ve el cerco blanco junto
a la vía; en Gerrards Cross, el edificio de estación y el puente. No bajar la
cámara bajo el suelo. Las capturas para contrastar el encuadre están en
[las doce referencias originales](fixtures/visual/or_reference/chiltern_station_views/README.md)
y [las vistas Bevy finales](fixtures/compatibility/journey-release-2026-10-05/README.md).

### 34.3. Horario y conductor automático

Probá el servicio extendido normal y mirá **F7**. Las dos últimas llegadas están
programadas a **10:21:30** y **10:29:30**, con salidas a 10:22 y 10:30. Es el
horario propio de esta extensión; se conserva el límite original de 15 mph.
El conductor automático debe registrar **seis paradas**, detenerse dentro de
10 m y completar el servicio sin salidas anticipadas. Se puede repetir sin
ventana con `openrailsrs play-service examples/chiltern_extended/scenario.toml`.
En una conducción manual, el resultado depende de tu manejo. La práctica de
estaciones acorta las esperas y no sirve para evaluar puntualidad normal.

### 34.4. Class 47: cabina, motor, frenos y sonidos

En Biblioteca, elegí Demo Model 1, la actividad de las 09:30 de Edimburgo y la
formación **MT_MT_Class 47 & 6 mk2 PP**. Usá cabina 3D. Con regulador en cero,
en **F8 → Locomotora**, el motor debe permanecer alrededor de **450 RPM** para alimentar los coches,
y el tren debe seguir detenido. Liberá el freno: la tubería debe subir a **5 bar**
y el cilindro debe descargarse. Con freno de servicio completo, la tubería baja
a **3,5 bar** y el cilindro de la locomotora llega cerca de **4,83 bar**.

Acelerá, soltá el regulador y frená: las agujas deben seguir el HUD. Probá la
bocina y escuchá motor y rodadura desde **1** y **2**; cambia su mezcla por la
posición del oyente. El paquete original tiene algunos WAV faltantes, detallados
en los informes de audio. No se rellenan con descargas de terceros. Para la
comparación física fija, ejecutar `python3 scripts/run_oracles.py --suite class47`:
debe mostrar `PASS class47_original_controls`. No equivale al servicio completo.
El viaje normal debe registrar las tres paradas y llegar a Linlithgow con cabina
y escenario visibles; el conductor al 75 % de la prueba llega unos 105/108 s
tarde a las dos últimas paradas. La actividad original conserva sus 600 s de
espera final; F10 permite práctica de cinco segundos.

### 34.5. Paquete Linux sin compilar

Extraé el paquete completo en una carpeta, incluso con espacios. Abrí una
terminal allí y ejecutá `./Jugar.sh --check`; debe terminar con «Paquete listo».
Ejecutá `./Jugar.sh` y comprobá que abre el menú, muestra ejemplos y permite
seleccionar el contenido instalado. Iniciá Chiltern o Demo Model 1 y comprobá
cabina, HUD y sonidos. No hace falta Cargo ni Rust. `./Jugar.sh --cpu` permite
probar el renderizador por software, si Mesa/lavapipe está instalado.

Los recursos originales se descargan por separado desde Biblioteca. Guardados,
ajustes y descargas quedan en los datos del usuario, fuera de la carpeta del
paquete. `BUILD.json` identifica la compilación y glibc; este paquete de QA
requiere el sistema del host de prueba o uno compatible. Snap sigue pendiente
de instalación y prueba de confinamiento, según el issue #189.

## 35. Cambiar km/h y mph durante la partida

Con el tren en marcha, pulsá **U** o el botón **km/h ↔ mph** de la barra inferior.
La velocidad y el límite del HUD deben cambiar de unidad sin pausar el tren:
por ejemplo, **36 km/h** equivalen a **22,37 mph**. Repetí para regresar a km/h.
Mantener U apretada debe producir un solo cambio.

Abrí **F4**, **F7**, **F8** y el **mapa M**: sus velocidades y límites deben usar
la misma selección. Las distancias, presiones y la física no cambian. Las agujas
y números de los instrumentos originales mantienen la unidad de la cabina;
un velocímetro cuya esfera dice MPH continúa mostrando mph.

Para conservar la elección al reiniciar, abrí **F10 → Guardar ajustes**. Podés
reasignar el atajo en F10. Si una configuración anterior ya usaba U para otra
acción, esa asignación se conserva y **F6** muestra la tecla libre elegida para
el nuevo control.

## 36. Sonido por coche, distancia y límites de tracción

1. Iniciá Pullman o 121, activá **Sonido original** en **F10**, con volumen
   moderado. En **F8 → Diagnóstico** deben aparecer SMS, muestras y salida
   activa. Escuchá primero detenido y después al acelerar: motor y rodaje
   deben variar según los programas del autor.
2. En **2**, acercá y alejá la cámara sin cambiar el regulador. Los coches
   próximos deben escucharse más que los distantes; al cruzar los rangos SMS
   debe activarse o silenciarse la capa correspondiente. Quedarte cerca no
   debe reiniciar la misma muestra en cada cuadro. Algunos sonidos no tienen
   efecto de distancia si su autor declaró `Ignore3D` o `Stereo`.
3. Alterná **1**, **Alt+1**, **2** y **5**; repetí 5 para cambiar de coche
   con vista interior. El sonido debe seguir el interior del coche elegido,
   sin recargar los WAV. Las fuentes externas que su SMS permita oír desde
   dentro deben estar amortiguadas según el coche; no se espera que todo
   el volumen de la cabina sea exactamente la mitad del exterior.
4. Con **Space** sostenida dos segundos, la bocina debe iniciar una vez y
   terminar al soltar. Aplicá y soltá freno, accioná puertas y limpiaparabrisas.
   Deben sonar los efectos disponibles, sin copiar la presión de la locomotora
   a todos los coches. El siseo de presión debe terminar al estabilizarse:
   puede haber hasta medio segundo de demora de simulación, según el muestreo.
   Un SMS que no declare un efecto no tiene por qué sonarlo.
5. Pausá y reanudá; luego guardá, avanzá y restaurá. La pausa detiene el audio
   y restaurar reinicia el estado de disparadores del instante guardado.
   Si separás una sección usando F9, sus sonidos de rodaje deben responder
   a que está estacionada, aunque la sección acoplada vuelva a acelerar.
6. Repetí con **1960CentralWR8Car**, **R Stock 6 Car**, **Downton Hall LE**,
   **KingLE** y la **Class 47 de Demo Model 1**. En las de vapor deben existir
   sonidos de marcha; en Class 47 deben conservarse los avisos de WAV ausentes
   del paquete. Consultá las ubicaciones indicadas por la auditoría del menú.

Los eléctricos requieren tensión y un captador compatible. Para contenido
antiguo de tercer o cuarto riel puede hacer falta una declaración explícita;
las pruebas de alimentación están en la sección 37. Esto no certifica todos
los sistemas eléctricos originales. Vapor tiene una caldera simplificada y no
hay cremallera funcional; detalle en [TRACTION_SUPPORT.md](TRACTION_SUPPORT.md).

Para repetir el ensayo sin ventana, con WAV e informe de cada vista, usá
[check_native_audio.py](../scripts/check_native_audio.py) según
[NATIVE_AUDIO.md](NATIVE_AUDIO.md). Señal y ausencia de saturación no certifican
una mezcla acústicamente idéntica a Open Rails.


## 37. Alimentación eléctrica, pantógrafo y disyuntor

Estos ejemplos son un banco de prueba sintético, con un modelo sencillo; no
pretenden comparar el paisaje ni la cabina de una locomotora eléctrica original.
Se pueden elegir desde el menú de ejemplos. Para abrirlos directamente:

```bash
cargo run --locked -p openrailsrs-viewer3d -- --live examples/electric_supply/scenario.toml
cargo run --locked -p openrailsrs-viewer3d -- --live examples/electric_supply/scenario_third_rail.toml
```

Usá el binario recién compilado. En F10 podés activar práctica rápida para no
esperar el horario. Las teclas de abajo son las predeterminadas; F6 muestra las
asignaciones vigentes si tenés controles propios.

1. **Arranque con catenaria.** Elegí el primer ejemplo. Debés ver 25 000 V y
   «Tracción disponible». Cerrá puertas con Q, soltá el freno con `;`, poné
   adelante con W y subí regulador con D. La velocidad debe aumentar.
2. **Corte del pantógrafo.** En marcha y antes del sector neutro, pulsá O.
   Debés ver «Pantógrafo sin contacto», tensión de contacto cero y esfuerzo de
   motor cero en F8, página de tracción. El tren sigue por inercia y pierde
   velocidad gradualmente. No debe quedarse congelado ni aplicar emergencia.
3. **Reconexión.** Esperá que termine de bajar y pulsá O otra vez. Este ENG
   declara 2 s de elevación, 1 s de cierre y 0,5 s de alimentación. La tracción
   vuelve después de esa secuencia si conservaste regulador y vía con tensión.
   Un modelo original que tenga pantógrafo animado debe seguir ese progreso.
4. **Disyuntor.** Pulsá J en un tramo alimentado. Debés ver disyuntor abierto y
   esfuerzo cero, aunque siga levantado el pantógrafo. Pulsá J de nuevo: el
   cierre tarda 1 s y la alimentación otros 0,5 s. El HUD y los instrumentos
   nativos disponibles deben acompañar el estado.
5. **Sector neutro.** Llegá al tramo de 150–230 m del primer edge con suficiente
   velocidad para cruzarlo por inercia. La toma está en el centro del coche:
   el cambio de estado ocurre unos metros después del valor del frente del tren.
   Dentro del tramo debés ver «Sin tensión de vía»; al salir, la conexión
   solicitada se recupera con sus retardos. No depende de la carga del escenario.
6. **Tercer riel.** Reiniciá con el segundo ejemplo. Debés ver 750 V, captador
   «Tercer riel» en F8 y tracción con el pantógrafo abajo. O no acciona un
   pantógrafo de esta formación. J y el tramo sin corriente deben cortar y
   recuperar la fuerza igual que en el otro ejemplo.
7. **Guardar durante el cierre.** Abrí J, pedí cerrar y guardá antes de que
   termine el retardo. Al cargar deben conservarse los controles, el tiempo
   pendiente y el movimiento; no debe recuperarse corriente anticipadamente.
8. **Aislar un motor.** Detené el tren y usá F9 para apagar alimentación o
   batería. Debe aparecer «Motor aislado». En una formación mixta diésel y
   eléctrica, el motor diésel conserva tracción sin suministro exterior.
9. **Sin electrificación o captador incompatible.** En una copia del TOML,
   cambiá la alimentación a `kind = "none", voltage_v = 0.0`, o cambiá
   solamente el suministro a `third_rail` dejando el captador aéreo.
   En vía plana, con regulador y freno suelto, el motor no debe acelerar.
   El HUD debe explicar el corte; en pendiente el tren puede moverse por gravedad.

Declaraciones físicas del escenario, separadas de los edificios y de la
geometría de catenaria:

```toml
[route.electric_supply]
kind = "overhead"           # none, overhead, third_rail, fourth_rail
voltage_v = 25000.0

[[route.electric_supply.sections]]
edge = "e1"                 # la dirección corresponde a este edge
start_m = 150.0
end_m = 230.0                # intervalo [inicio, fin)
kind = "none"
voltage_v = 0.0

# Para una formación que toma corriente de tercer riel:
[[train.electric_pickups]]
vehicle = 0                 # índice desde cero, contando todos los coches
kind = "third_rail"
```

Un suministro de tercer riel requiere declarar también ese suministro en
`route.electric_supply`. Esta configuración no crea rieles visuales ni añade
un motor auxiliar. Los perfiles de ejemplo son deliberados y no describen
la electrificación histórica de Chiltern. Los límites del modelo se detallan
en [TRACTION_SUPPORT.md](TRACTION_SUPPORT.md).

## 38. Vapor y diésel: reservas, arranque y fogonero

En el menú elegí los ejemplos de **traction_operation**. Tienen 1 km, tres
paradas y esperas de cinco segundos. El modelo sencillo permite probar los
controles sin descargar contenido. Para las cabinas y sonidos originales,
repetí con Pullman/121, Downton Hall LE y KingLE del paquete Chiltern instalado.
Las teclas son las predeterminadas; F6 muestra tus asignaciones actuales.

Iniciá una partida nueva para estas pruebas: los guardados anteriores que no
incluyen las reservas de tracción no permiten recuperarlas con certeza y pueden
ser rechazados al cargar.

1. **Parar y arrancar el diésel.** Elegí «Diésel: arranque, parada y combustible».
   Abrí B: debe mostrar «En marcha», 300 RPM y 25 L de capacidad. Al pasar
   tiempo en ralentí debe consumir unos 18 L/h. Cerrá B y pulsá K; las RPM
   deben bajar hasta cero en unos segundos. Ya detenido, el consumo cesa.
   Con regulador cerrado, K inicia «Arrancando» antes de «En marcha»;
   durante el arranque todavía no debe producir fuerza.
2. **Corte en movimiento.** Cerrá puertas con Q, soltá freno con `;`, poné
   adelante con W y aumentá regulador con D. Pulsá K: el esfuerzo del motor
   debe caer a cero y el tren sigue por inercia. En un modelo con emisor ENG,
   el escape cesa cuando termina la combustión, no al instante de pulsar K.
   Para arrancar de nuevo, cerrá primero el regulador con A.
3. **Condiciones de arranque.** Con el motor detenido, abrir regulador y
   pulsar K debe informar que hay que cerrarlo. Con tren detenido, apagá
   batería en F9: tampoco debe arrancar. Restablecé batería para probarlo.
4. **Agotamiento rápido.** Elegí «Diésel: agotar el tanque en 20 segundos».
   El depósito artificial de 0,2 L se vacía en unos 20 segundos a ralentí;
   al acelerar tarda menos. Debe quedar en cero, cortar fuerza y rechazar
   otro arranque. Guardar y cargar o aislar y reconectar un motor no lo repone.
5. **Dos motores.** Elegí «Diésel: dos motores independientes». En B, parando
   solo el motor 2 debe conservarse el motor 1. En F9, aislá el motor 1 y
   comprobá en F8 que el 2 conserva sus RPM, reserva e identidad. Reconectar
   no debe devolver combustible. Las operaciones de F9 requieren tren parado.
6. **Vapor asistido.** Elegí «Vapor: fogonero y ténder finito». B debe mostrar
   presión de 16 bar, agua de caldera cerca del 90 %, 15 000 L en el ténder,
   8000 kg de carbón y fuego separado. Conducí: el consumo de vapor baja el
   nivel; al bajar del 75 %, el automático acciona los inyectores y toma agua
   del ténder hasta recuperar el 90 %. En este recorrido corto puede no llegar
   a ese umbral; el ejemplo de reservas mínimas permite probarlo en segundos.
   Abrir B no pausa una partida en marcha; si ya estaba pausada, sigue así.
7. **Corte y purgas.** En B reducí el corte manteniendo el mismo regulador:
   deben bajar fuerza y consumo. Abrí las purgas: deben aumentar las pérdidas
   y verse vapor en los emisores de cilindros disponibles. El corte y las
   purgas permiten conservar el fogonero automático.
8. **Fogonero manual.** Accioná tiro, pala, soplador o inyectores: debe indicar
   «manual». Con tiro y pala en cero, el fuego deja de producir calor nuevo.
   Abrí un inyector: con presión suficiente el agua del ténder baja y la
   caldera gana agua; inyectar también enfría. Activar el automático devuelve
   la asistencia con las reservas restantes. No debe rellenar el ténder.
9. **Vapor con reservas mínimas.** Elegí «Vapor: agotamiento rápido de reservas».
   Es un banco artificial de 20 L de caldera, 2 L de ténder y 0,2 kg de carbón.
   En B abrí un inyector para agotar la reserva de agua en segundos. No debe
   hacerse negativa ni crecer sola. Bajo demanda, una caldera con nivel del
   15 % o inferior debe indicar daño y perder la fuerza; cerrar controles no
   elimina ese daño. Reiniciar la partida sí inicia una locomotora nueva.
10. **Cabina y sonido originales.** Hall/King incluyen cabina clásica 2D.
    Entrá con 1 y usá Alt+1 para elegirla si estás en la vista 3D.
    Los manómetros, agua del
    ténder y controles CVF disponibles deben seguir el estado de B/F8. Con
    la cabina 2D, probá regulador, corte, tiro e inyectores con el mouse. Las
    purgas, inyectores y soplador deben disparar el efecto declarado en el SMS
    una vez por cambio, sin reiniciarlo en cada cuadro. Un control o un
    sonido que el autor no incluyó puede no estar disponible.
11. **Guardar y cargar.** Guardá desde Esc con el motor arrancando o el
    fogonero manual. Al cargar, deben conservarse transición, RPM, reservas,
    corte e interruptores, en lugar de empezar con depósitos llenos.

Los controles nuevos también funcionan en la simulación sin visor. Las
pruebas de arranque y unidades usan OR 1.6.1; la caldera tiene termodinámica
simplificada y no se afirma paridad completa de vapor, transmisiones ni freno
dinámico. Ver [TRACTION_SUPPORT.md](TRACTION_SUPPORT.md).

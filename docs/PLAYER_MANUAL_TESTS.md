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
   de práctica, no recreaciones visuales completas de Argentina.
3. Con la misma estación, hora y cámara, compará despejado y nieve. El suelo
   y los copos cambian; las fachadas y techos conservan sus colores/texturas.
   Probá también alejándote para activar LOD y cargando un sector nuevo.
4. Seguí [GPU_TEXTURES.md](GPU_TEXTURES.md): exportá una ACE o una carpeta a
   `tmp/`, revisá el informe y comprobá que las ACE originales siguen intactas.
   Compará carga automática y `OPENRAILSRS_TEXTURE_UPLOAD=rgba`: la formación,
   la vegetación y los edificios deben conservar su apariencia.

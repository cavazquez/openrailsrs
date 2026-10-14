# Prueba manual de los 40 issues del quinto lote

Esta guía recorre exactamente los 40 issues seleccionados en la matriz de progreso de la fase 37. Describe cómo probarlos; no certifica su cierre, la aceptación del candidato actual ni una ejecución de dos horas. Los resultados deben identificar la versión que realmente se ejecutó. Open Rails 1.6.1 es la referencia para los sistemas equivalentes; las extensiones propias de Bevy tienen pruebas separadas.

## Preparar la partida

En un checkout ya compilado, abrir `target/debug/openrailsrs-viewer3d --menu`. Para abrir un ejemplo directamente, usar `target/debug/openrailsrs-viewer3d examples/player_lab/scenario.toml --live`, cambiando el archivo por el indicado en cada caso. Esto permite distinguir ejemplos con el mismo nombre, como el laboratorio recto y el de curva. En el paquete portátil, abrir el lanzador de la distribución. No hace falta recompilar para cada prueba.

**Nueva partida empieza por País.** Para los laboratorios propios, elegir **Todos los países** y buscar el servicio en **Ruta y servicio**. Los paquetes sin procedencia declarada pueden aparecer en **Sin dato de país**. Elegir un país limita las rutas, pero la lista de **Tren** reúne las formaciones de todos los paquetes instalados. Por ejemplo, se puede mantener **Reino Unido → Chiltern** y buscar **SNCF_TGV-325-FR_Sharpe**. Su aviso de alimentación sigue siendo pertinente: disponible en el selector no significa capaz de traccionar sobre cualquier vía. Francia puede no tener ninguna ruta instalada aunque haya un TGV instalado.

En cada inicio, revisar **Ver detalles**. Los faltantes obligatorios impiden jugar con esa formación; una fila gris sigue siendo seleccionable para consultar el diagnóstico. Trocha y tensión desconocidas deben figurar como desconocidas. Una advertencia de compatibilidad y el número de sistemas contrastados con oráculos no son archivos faltantes. Para contenido original, instalar la edición completa del autor y conservar la versión anterior.

Las teclas citadas son las predeterminadas; **F6** muestra las asignaciones vigentes y **F10 → Controles** permite cambiarlas. **1** abre cabina, **2** exterior, **F4** monitor de vía, **F7** libreta del servicio, **F8** HUD avanzado, **F9** formación, **M** mapa, **B** tracción y **F10** ajustes. **Q** controla puertas. Para mover la cámara exterior, usar el mouse, la rueda y los controles de cámara; las flechas y Page Up/Page Down sirven para desplazamiento y altura en vuelo. Q no debe desplazarla.

**Guardar siempre significa P o Esc → Guardar 1/2/3. F8 no guarda.** Para restaurar en la misma partida, usar **Cargar 1/2/3**; desde el inicio, **Continuar → Reanudar**. Las partidas vuelven pausadas. Los paneles grandes pausan la simulación: después de ordenar puertas, mantenimiento o abastecimiento, cerrar el panel para observar su avance. Para invertir, cerrar regulador, detenerse y usar el control indicado en F6; el neutro tiene su propia acción, predeterminada en `\`.

Para una prueba visual repetible, anotar servicio, edición del contenido, formación, fecha/hora, semilla, clima, intensidad, cámara, distancia de visión y calidad. Comparar desde el mismo lugar. En efectos animados, repetir pausa, cabina/exterior, guardado/restauración y avance a otro sector. Una captura quieta no prueba movimiento ni estabilidad de memoria.

## Instalar y probar en Snap

Si la revisión que se quiere probar está publicada en edge, instalar con `sudo snap install openrailsrs --edge`; si ya está instalado, actualizar con `sudo snap refresh openrailsrs --edge`. Consultar `snap list openrailsrs` y anotar versión/revisión antes de ejecutar `snap run openrailsrs`. El nombre reservado o una compilación local no demuestran que esa revisión esté publicada. Para un archivo local suministrado para QA, instalar ese artefacto con `sudo snap install --dangerous /ruta/al/paquete.snap`; conservar su SHA-256 y comprobar que usa confinamiento estricto.

Las descargas habituales van a `$SNAP_USER_COMMON/openrailsrs/official-content`, normalmente `~/snap/openrailsrs/common/openrailsrs/official-content`. En un binario Linux sin otra preferencia, van bajo `$XDG_DATA_HOME/openrailsrs` o `~/.local/share/openrailsrs`. La biblioteca muestra el destino efectivo; una carpeta elegida por el usuario puede cambiarlo. Las rutas se instalan fuera de Git y del paquete. No copiar archivos en `/snap` ni intentar modificarlo con privilegios.

Para un disco externo, conectar el permiso cuando corresponda con `sudo snap connect openrailsrs:removable-media` y elegir una carpeta accesible desde **Biblioteca → Copias y respaldos**. La preferencia crea `openrailsrs-content` dentro de esa carpeta; no traslada instalaciones anteriores. Un rechazo de permisos debe explicar qué ubicación no es accesible. La [guía de contenido](OFFICIAL_CONTENT.md) explica destinos, URL originales y formatos admitidos.

## Qué cuenta como medición

Los apartados instrumentados necesitan un visor con `dev-tools` o `--all-features`, sus recursos/shaders correspondientes y las herramientas del checkout. Un Snap normal puede no incluir esos ganchos. Usar una sola carga gráfica por GPU y conservar hashes de ejecutables, configuración, logs, JSON y PNG nuevos. Una prueba por software sirve para funcionamiento, no para atribuir FPS de una GPU física.

Para rendimiento, guardar P99 y máximo de **carga inicial** y **conducción** por separado, con la misma ruta, formación, cámara, calidad, semilla y hardware. Una caché fría de la aplicación no vacía las cachés del sistema operativo o del controlador. Repetir referencia/candidato en orden ABBA, con lanzamiento frío y caliente en cada bloque. No atribuir una mejora al código cuando cambia la carga de fondo. Las herramientas y sus límites están en [PERFORMANCE_FIFTH40.md](PERFORMANCE_FIFTH40.md).

Para memoria, comparar visitas al mismo escenario, cámara, clima y edad real desde su carga; distinguir RAM, VRAM/GTT del proceso, recursos GPU vivos y reserva del asignador. Un dato no disponible no equivale a cero. Bajar el consumo inicial o terminar una captura no acredita ausencia de fugas.

## #240 — Plan 030: Reducir tirones restantes al preparar objetos grandes

- **Entrar:** Chiltern original instalado, servicio extendido con Blue Pullman; mantener 2.000 m de visión, cámara de cabina y la misma lluvia intensa en referencia y candidato.
- **Acción:** conducir el mismo tramo atravesando carga de sectores y pasar a exterior. Conservar el arranque completo y repetir los ocho lanzamientos ABBA frío/caliente. `scripts/check_fifth40_performance.py` sirve para capturar comparaciones; comprobar sus opciones y la distancia efectiva del recibo: su configuración habitual de 500 m no sustituye el protocolo de 2.000 m.
- **Medir:** P99 y máximo por fase, colas de formas/transferencias y `shape_stages`. Para atribuir picos, habilitar el diagnóstico `OPENRAILSRS_PROFILE_SYSTEMS=1` y `OPENRAILSRS_FRAME_SPANS=1` en una ejecución separada. Los bloques anidados pueden ser inclusivos; no sumarlos como trabajo independiente. La aceptación necesita mejora medida y contenido próximo completo, no sólo sensación de fluidez.
- **Límite:** la comparación ABBA de fase 37 fue rechazada; sus empates y regresiones siguen siendo antecedentes válidos. Esta guía no declara aprobado el candidato posterior.

## #243 — Plan 033: Animar fuelles acompañando curvas y uniones entre coches

- **Entrar:** `examples/player_lab/scenario_connections_curve.toml`, laboratorio propio de fuelles, mangueras y acopladores.
- **Acción:** con **2**, acercarse a la unión desde ambos lados; avanzar despacio por la curva. Comparar con `scenario_connections.toml` en recta, cambiar zoom/LOD, pausar y guardar/restaurar.
- **Ver:** los dos extremos del fuelle siguen unidos a sus coches, la textura y la topología se conservan y no aparece un estiramiento ilimitado. En el ensayo instrumentado, `flexible_connections` debe registrar piezas elegibles y deformadas; cero error con cero piezas no prueba nada.
- **Límite:** sólo se deforma la geometría y las matrices declaradas. No debe aparecer un fuelle inventado en un tren que no lo trae.

## #244 — Plan 034: Animar mangueras y cables flexibles entre vehículos

- **Entrar:** el mismo laboratorio de conexiones; usar la versión recta para el corte.
- **Acción:** mirar manguera y cable al avanzar por curva. En **F9**, asegurar el segundo coche con freno de mano y desacoplar detrás del primero. Guardar y restaurar. Probar también las conexiones neumáticas y eléctricas disponibles en F9.
- **Ver:** cada extremo acompaña a su conector; las piezas que cruzaban el corte dejan de unir los coches y siguen desconectadas después de reanudar. La proximidad visual no restablece por sí sola manguera o bus.
- **Límite:** el modelo necesita conectores declarados. No confundir una manguera dibujada con una conexión neumática abierta.

## #245 — Plan 035: Afinar coincidencia entre posición física y visual de acopladores

- **Entrar:** `examples/player_lab/scenario_connections.toml` y su variante `scenario_connections_curve.toml`.
- **Acción:** observar el enganche en recta/curva, aplicar y soltar freno para cambiar holgura, invertir detenido y repetir corte/restauración desde F9. Comparar cabeza, cola y contacto, sin mover artificialmente un coche a otra vía.
- **Ver o medir:** el enganche visual permanece en los puntos declarados y la operación física conserva la composición. El informe de conexiones debe tener piezas reales y error de extremos acotado; revisar fuerzas en **F8 → Fuerzas**.
- **Límite:** el oráculo de [acople original](fixtures/compatibility/fifth40-20261013/coupling-original.json) compara composición, estados y tres equilibrios con OR 1.6.1. No certifica curvas transitorias de impacto ni convierte el acople manual detenido en acople automático en movimiento.

## #253 — Plan 043: Mejorar trenes articulados con bogies compartidos

- **Entrar:** `examples/player_lab/scenario_jacobs_curve.toml`; comparar con `scenario_jacobs.toml` en recta.
- **Acción:** observar el apoyo compartido con **2**, avanzar y frenar en curva, guardar/restaurar e invertir detenido. Intentar cortar la unión articulada desde F9.
- **Ver:** los dos cuerpos conservan el apoyo común, el bogie declarado sigue esa pose y el corte de esa articulación se rechaza. `jacobs_joints` debe contener la unión real; una formación convencional no valida este caso.
- **Límite:** es un laboratorio propio de articulación longitudinal ideal. No demuestra fuerzas laterales, contacto rueda/pestaña ni que todos los TGV u otros modelos de comunidad declaren Jacobs correctamente.

## #254 — Plan 044: Representar pasajeros entrando y saliendo por puertas habilitadas

- **Entrar:** usar un servicio con parada, cantidades de pasajeros, lado de andén y accesos visuales declarados. El laboratorio base tiene puertas, pero **no trae una parada de embarque**: abrirlo y pulsar Q no reproduce este ensayo completo.
- **Acción:** para el caso propio reproducible, ejecutar `python3 scripts/check_player_fifth_batch.py --viewer target/debug/openrailsrs-viewer3d --out-dir tmp/manual-fifth40 --cases boarding boarding-close boarding-restore alighting`. Genera escenarios bajo `tmp/manual-fifth40/scenarios`. Después se puede abrir `boarding.toml` directamente con el visor normal, sin acciones automáticas, detenerse en su parada y abrir sólo el lado izquierdo mediante F9.
- **Ver:** las figuras entran por la puerta habilitada y los contadores progresan juntos. Pedir cierre durante un cruce deja terminar sólo esa transferencia, de hasta 1,5 s, y empieza a cerrar sin esperar a todos. Pausa y restauración conservan puerta y progreso; el lado contrario no embarca.
- **Límite:** `alighting` inicializa pasajeros mediante el arnés de QA; no se presenta como una opción manual ya disponible en el menú. La bajada requiere pasajeros previos reales o ese fixture instrumentado. La cantidad de figuras puede agrupar más pasajeros.

## #265 — Plan 055: Agregar espejos funcionales para formación y andén

- **Entrar:** **Laboratorio de cabina y carteles**, `examples/player_lab/scenario.toml`, con cabina **1**.
- **Acción:** mirar los dos espejos, mover el tren, pasar a **2**, volver a cabina y repetir con lluvia y pausa.
- **Ver o medir:** cada espejo muestra su costado y parte posterior con orientación de espejo, sin HUD reflejado. Exterior los oculta. El informe registra dos vistas de 256 × 160 y actualizaciones efectivas, limitadas a 10 Hz. Comprobar coste de cámaras en un perfil separado.
- **Límite:** se desactivan bajo presión gráfica; no exigir su presencia a una cabina que no declara espejos. Una captura inmóvil no demuestra actualización ni corrección de origen flotante.

## #273 — Plan 063: Mantener horizonte convincente con terreno cargado por sectores

- **Entrar:** Chiltern, servicio extendido **Northolt Park → Gerrards Cross**, con su escenario original instalado.
- **Acción:** revisar Northolt Park, South Ruislip y Gerrards Cross desde **1** y **2**; elevar/alejar cámara y cambiar **F10 → Imagen y cabina → Distancia del escenario**. Avanzar a otros sectores y repetir.
- **Ver:** el horizonte conecta con el relieve cercano sin agujeros ni una plataforma de terreno duplicada. Al llegar el detalle cercano, su versión distante se retira; el terreno sigue apareciendo al avanzar. Registrar pendientes de carga antes y después de detenerse.
- **Límite:** el horizonte usa alturas y muestras del contenido instalado con un número acotado de sectores. No es una reconstrucción completa del país ni una comparación píxel a píxel con OR.

## #276 — Plan 066: Afinar sombras de trenes, edificios y vegetación

- **Entrar:** las mismas tres estaciones de Chiltern, a igual fecha/hora y clima despejado; incluir tren, edificios y árboles en el encuadre.
- **Acción:** comparar **F10 → Imagen y cabina → Sombras de contacto** desactivadas y su mayor nivel disponible. Mover únicamente la cámara, pasar por una curva y revisar una vista baja junto a la vía o un puente realmente visible.
- **Ver:** tren y objetos proyectan sombras coherentes; no debe desplazarse una sombra con forma de cámara ni oscurecerse el suelo como si el cielo fuera un objeto. Revisar transiciones de cascadas y el coste con las mismas condiciones.
- **Límite:** la vista histórica llamada `bridge-low` no prueba estar debajo de un puente. No registrar esa prueba sin un puente y sus apoyos visibles en la nueva imagen.

## #277 — Plan 067: Mejorar cristales y transparencias superpuestas

- **Entrar:** Blue Pullman original completo, cabina y exterior; colocar varias ventanas y vegetación en línea de visión.
- **Acción:** alternar **F10 → Imagen y cabina → Mejorar cristales superpuestos**; comparar con el mismo encuadre, de día y de noche, con pausa y cambio de cámara.
- **Ver:** el interior sigue visible a través de capas de vidrio; no aparecen ventanas como paneles opacos ni halos rectangulares en hojas con recorte alpha. Los elementos opacos conservan su profundidad.
- **Límite:** la mejora OIT tiene almacenamiento acotado y puede desactivarse bajo presión. Sólo se comprueba el material y modelo presentes; un recurso de cabina ausente no se arregla con transparencia.

## #278 — Plan 068: Agregar reflejos de lluvia en rieles y superficies compatibles

- **Entrar:** Chiltern desde cabina y exterior, manteniendo hora, formación, cámara y calidad.
- **Acción:** en **F10 → Hora y clima**, pasar de seco a lluvia y volver a seco; dejar evolucionar el mojado, probar de noche con faros y repetir pausa/restauración.
- **Ver:** los materiales compatibles adquieren brillo de superficie mojada con intensidad limitada, manteniendo las texturas del autor. Al secarse recuperan su apariencia de origen, sin oscurecer permanentemente todos los objetos ni crear imágenes nuevas en cada cambio.
- **Límite:** es una respuesta de mojado de materiales compatibles; no supone reflejos especulares completos de cualquier objeto ni que todas las superficies originales declaren esa respuesta.

## #282 — Plan 072: Implementar pasos a nivel operativos

- **Entrar:** un itinerario original de Chiltern que atraviese el cruce de Park Royal. La referencia de QA es el sector WORLD `-6083:14926`, barreras UID 12473/12474; el recorrido extendido de las tres estaciones no se presenta como si pasara necesariamente por allí.
- **Acción:** acercarse con **2**, ocupar el cruce, guardar durante la apertura y avanzar hasta despejar con toda la cola. Para localizar el caso exacto, usar `python3 scripts/check_native_world_fifth_batch.py --viewer target/debug/openrailsrs-viewer3d --route-root /ruta/ROUTES/Chiltern --out-dir tmp/manual-crossing --cases crossing-closed crossing-released`.
- **Ver:** protección antes de entrar, barreras cerradas mientras queda cualquier parte del tren y apertura tras despejar. Otro tren o un corte sobre el cruce mantienen la protección. Pausa y restauración conservan fracción de apertura.
- **Límite:** necesita LevelCr y enlaces originales TDB/RDB válidos. El arnés crea un itinerario de prueba sobre el grafo original; no inventa una actividad manual del autor ni acredita todas las normativas regionales.

## #283 — Plan 073: Coordinar tránsito vial y colas en pasos a nivel

- **Entrar:** el mismo caso de Park Royal, con `CarSpawner` y RDB del autor cargados.
- **Acción:** observar las dos carreteras con el cruce protegido y después liberado; mantener el juego varios ciclos, pausar y volver desde otra cámara/sector.
- **Ver o medir:** los autos siguen las curvas originales, la primera fila para antes de las barreras y las siguientes guardan separación. Al abrir, las filas avanzan sin cruzarse con el tren ni reaparecer autos superpuestos en la entrada. Revisar número de autos, puntos de espera resueltos y colas del informe.
- **Límite:** una carretera sin spawner o con enlace ausente no demuestra un fallo de la cola, ni debe rellenarse con tránsito inventado. El número esperado depende de la edición y la escena, no es siempre 38.

## #284 — Plan 074: Agregar peatones en estaciones y espacios habilitados

- **Entrar:** el fixture propio de andén `boarding.toml` descrito en #254; para la geometría de acceso, partir de `examples/player_lab/scenario.toml`.
- **Acción:** observar espera, subida y bajada desde el lado habilitado; intentar ver el corredor completo entre espera y puerta, pedir cierre y repetir pausa, restauración y cambio de origen mediante el caso `boarding-rebase` del arnés.
- **Ver:** las figuras no caminan por la vía, no atraviesan paredes y no se bloquean por incluir sus propios cuerpos entre obstáculos. Los embarcados dejan de representarse fuera. Densidad y representación se mantienen acotadas —hasta 64 figuras— mientras el servicio conserva las cantidades reales.
- **Límite:** se requieren accesos y corredores seguros declarados. El juego no añade peatones navegando libremente por cualquier estación descargada. La comprobación de origen se acredita con posición absoluta y telemetría, no sólo alejando el zoom.

## #285 — Plan 075: Animar instalaciones industriales y equipos de carga

- **Entrar:** **Laboratorio de abastecimiento original**, `examples/player_lab/scenario_refill.toml`; el nombre del laboratorio no lo convierte en contenido comunitario.
- **Acción:** detenerse con la toma alineada y regulador cerrado. En **B**, pulsar **Abastecer en la toma más cercana** y cerrar el panel para avanzar. Observar el brazo con **2**; pausar, guardar/restaurar y probar **Cancelar abastecimiento**.
- **Ver:** equipo y transferencia siguen la misma operación; la animación abre y vuelve al terminar/cancelar. Pausa detiene ambas y reanudar conserva cantidad y fase. La reserva es finita y el aporte no borra consumo ni daños previos.
- **Límite:** una toma necesita Pickup e IntakePoint de tipo compatible. La captura de una apertura no prueba suministro; revisar las cantidades. No se promete funcionamiento genérico de grúas o terminales no modeladas.

## #291 — Plan 081: Evitar que cámara exterior atraviese objetos sólidos

- **Entrar:** laboratorio propio, con el muro `camera_wall.s`, o un edificio opaco cargado de Chiltern.
- **Acción:** en exterior, acercarse por mouse/zoom; para desplazamiento libre usar **F2**, flechas y Page Up/Page Down. Intentar cruzar pared y suelo desde varios ángulos, sin usar un teletransporte para colocar el ojo al otro lado. Repetir después de cargar un sector nuevo.
- **Ver:** la cámara se detiene o desliza antes del volumen sólido y conserva el límite del terreno. Si una carga encierra el ojo, debe salir a una posición válida sin dejarlo bajo el mapa. La acción Q no mueve la cámara.
- **Límite:** sólo cuentan sólidos cercanos cargados y clasificados como tales; la vegetación transparente no se trata automáticamente como una pared. El caso instrumentado `camera-collision` exige obstáculos reales y registra correcciones.

## #296 — Plan 086: Dar espesor visual a nieve acumulada

- **Entrar:** laboratorio propio y Chiltern en South Ruislip, con nieve elegida en **Hora y clima**. Usar el caso nativo `roofs-snow` para el mismo encuadre de tejados.
- **Acción:** mirar techo, suelo, fachada, vidrio y vía desde ambos lados, después variar intensidad/calidad y mover la cámara. Repetir pausa/restauración.
- **Ver:** espesor irregular en superficies superiores apropiadas; fachadas, cristales y cabina mantienen su material original. La vía sigue utilizable, sin una capa que oculte arbitrariamente los rieles. Los límites son 8 cm en terreno y 5 cm en superficies marcadas.
- **Límite:** depende de normales correctas y superficies compatibles. No cambiar edificios a una textura blanca ni utilizar una fachada con normal vertical como supuesto ensayo representativo. Es una extensión de Bevy, con aceptación visual propia.

## #297 — Plan 087: Mostrar huellas y despeje de nieve por vehículos

- **Entrar:** `examples/player_lab/scenario_jacobs_curve.toml`, con nieve; sus ruedas declaradas permiten relacionar las marcas con el tren.
- **Acción:** avanzar mirando detrás de las ruedas, frenar y pausar. Guardar/restaurar, moverse entre sectores y volver al tramo recorrido; comparar con nieve intacta a los lados.
- **Ver o medir:** las marcas nacen detrás de ruedas físicas, siguen la curva y no borran la cobertura exterior. Pausa no genera nuevas marcas; restaurar o cambiar origen no dibuja una raya entre posiciones lejanas. `snow_tracks` informa segmentos/sectores y actualizaciones de materiales con almacenamiento acotado.
- **Límite:** no fabricar huellas para un modelo sin ejes compatibles. Los 64 segmentos próximos enviados a GPU son un límite de representación; no implican guardar indefinidamente todo el trayecto.

## #314 — Plan 104: Reducir carga y memoria de bancos de audio grandes

- **Entrar:** Blue Pullman con recursos SMS/WAV originales completos. Activar **F10 → Sonido → Sonido original** con volumen moderado; abrir **F8 → Diagnóstico** para comprobar salida y programas.
- **Acción:** escuchar ralentí, acelerar, rodar, frenar y alternar cabina/exterior/pasajeros varias veces. Repetir el mismo viaje frío y caliente con el arnés de rendimiento y conservar sus métricas de audio.
- **Ver, oír y medir:** los bucles continúan al cambiar cámara; no reinician cada WAV. Comparar apertura/decodificación, reutilización, PCM único, expulsiones y colas. `decoded_mib` es asignación única; no sumar bancos que comparten muestras.
- **Límite:** el presupuesto de PCM no incluye toda la RAM del audio. ALSA nulo permite medir el trabajador, pero no acredita escucha, ausencia de chasquidos o una mezcla correcta. Un SMS ausente conserva su diagnóstico.

## #333 — Plan 123: Cargar más tráfico original durante actividades completas

- **Entrar:** edición original instalada de **Chiltern v4 → RS_Football Special**, con la formación original elegida por esa actividad y todos sus recursos. No sustituirla por el servicio preparado de tres estaciones.
- **Acción:** abrir **F7** y comprobar Banbury General, Bicester North, Princes Risborough y High Wycombe. Completar el recorrido, observando **F4**, **M** y **F9**. Guardar durante circulación con tráfico y reanudar desde el menú.
- **Ver o medir:** servicios AI originales con sus horas, recorridos, formaciones y ocupación real; señales no asumidas libres. Deben conservarse posiciones, combustible, frenos y decisiones después de reanudar. Registrar final del jugador, errores SIGSCR y estado de cada AI.
- **Límite:** el caso incluye veinte servicios originales, pero pueden salir más tarde y seguir circulando cuando termina el jugador. No adelantarlos para contarlos como terminados ni extender el resultado a todos los eventos/scripts C# de otras actividades.

## #335 — Plan 125: Programar adelantamientos entre locales y expresos

- **Entrar:** **Tráfico: adelantamiento de local y expreso**, `examples/traffic_passing/scenario_overtake.toml`.
- **Acción:** atender la parada del local —abrir puertas, completar intercambio y cerrar—, revisar F9 y conducir despacio al apartadero elegido. Detenerse antes de la salida y observar con **2** al expreso. Guardar mientras se espera y reanudar.
- **Ver:** el local conserva su parada y queda protegido; sólo sale cuando la cola del expreso despeja el tramo compartido y las agujas. F9 conserva plan y espera; ambos pueden completar su destino.
- **Límite:** alternativas y longitud útil se declaran en la red del ejemplo. Un apartadero corto u ocupado debe producir diagnóstico; no eliminar paradas ni inventar vía por un temporizador.

## #339 — Plan 129: Planificar encuentros en vía única y apartaderos

- **Entrar:** **Tráfico: encuentro en vía única**, `examples/traffic_passing/scenario_meet.toml`.
- **Acción:** revisar el encuentro previsto en F9/M, atender la parada y avanzar al apartadero según la instrucción. Esperar al tren contrario, guardar/restaurar y continuar tras su despeje completo.
- **Ver:** el encuentro se resuelve antes de bloquear ambos servicios frente a frente. Se conserva el destino y el corredor no se libera mientras queda cola o una aguja ocupada. F9 identifica el motivo de espera y la alternativa válida.
- **Límite:** este ejemplo propio prueba una planificación declarada. Redes sin apartadero válido no deben aprobar mediante una separación visual artificial ni una reserva que ignore la longitud del tren.

## #340 — Plan 130: Profundizar autorizaciones de maniobra con límites

- **Entrar:** una partida con una señal real del itinerario, por ejemplo Chiltern, y el tren detenido a menos de 100 m antes de ella. El patio base no trae por sí solo la señal sintética `yard_signal` de los tests.
- **Acción:** en **M**, elegir la señal y pulsar **Maniobra · 200 m / 15 km/h / 120 s** con el corredor libre. Consultar F9; cerrar paneles y avanzar a velocidad de maniobra. Guardar a mitad de la autorización. Probar también dejar transcurrir el tiempo o invertir el itinerario.
- **Ver:** límite y distancia restante, frenado antes del extremo autorizado, persistencia del tiempo restante y revocación por vencimiento/inversión/ocupación. Una señal ajena, distancia inválida o sección con otro tren deben rechazarse sin mutar la partida.
- **Límite:** no pulsar **Vía libre** como sustituto de esta autorización. El caso exacto de ruedas propias, intrusión y expiración es el fixture creado en `crates/openrailsrs-sim/tests/shunt_authority.rs`; no se ofrece como actividad comunitaria instalada ni como SIGSCR completo.

## #341 — Plan 131: Permitir tomar control de otra locomotora o servicio

- **Entrar:** **Cambio de conductor · Laboratorio de cabina y carteles**, `examples/traffic_passing/scenario_control.toml`.
- **Acción:** detener ambas formaciones, cerrar regulador y poner neutro. En F9 pulsar **Tomar control** junto al servicio `other`. Revisar frenos de mano y preparar el nuevo tren antes de salir. Guardar/restaurar y probar otra transferencia válida.
- **Ver:** cambian cabina/modelos y mando sin reiniciar posición, hora, combustible, frenos o equipos. El anterior servicio queda bajo su conductor automático y las reservas siguen compartidas. El nuevo tren puede seguir asegurado: liberar sus frenos es una acción del jugador.
- **Límite:** rechazar tren en marcha, terminado o sin locomotora. Si falla contenido o guardado, debe conservarse la partida anterior; no sirve una captura de la nueva cabina para demostrar continuidad de estados.

## #342 — Plan 132: Acoplar manualmente formaciones independientes

- **Entrar:** **Recoger formación · Laboratorio de fuelles, mangueras y acopladores**, `examples/traffic_passing/scenario_coupling.toml`.
- **Acción:** detener, dejar sin tracción y asegurar con frenos de mano todos los coches donantes. Con extremos trasero/delantero compatibles a menos de 1 m, en F9 pulsar **Recoger formación**. Conectar manguera, abrir ambas llaves del nuevo corte y liberar frenos de mano; comprobar aplicación/liberación antes de partir. Guardar/restaurar.
- **Ver:** cuatro vehículos en este ejemplo, desaparición del donante de tráfico y conservación de presiones, combustible y reservas, sin duplicados. Toda la formación queda físicamente sobre el itinerario correcto.
- **Límite:** este comando detenido cubre diésel/eléctrico con TCS Rust y dinámica compatible; rechaza vapor, scripts externos, frenos incompatibles y uniones que desplazarían coches a otro ramal. El oráculo original y sus límites transitorios se detallan en la guía de operaciones.

## #343 — Plan 133: Clasificar vagones por destino en patios

- **Entrar:** **Patio: clasificar y recoger vagones por destino**, `examples/yard_training/scenario_delivery.toml`; después **Patio: dos cuadrillas, dos destinos**, `scenario_classification.toml`.
- **Acción:** revisar F7/F9. Asegurar coches 2 y 3, seleccionar el primero y desacoplar detrás. Comprobar el lote Norte en su zona; avanzar sólo con la locomotora. Volver y recogerlo preparando mangueras/llaves/frenos. En el segundo ejemplo, transferir mando al servicio Este para su lote y guardar/restaurar.
- **Ver:** entrega aceptada sólo cuando todos los vagones identificados están completos dentro de la vía/zona y asegurados. Recogerlos retira la aceptación. Se conservan identidad de origen, composición, frenos y reservas aunque dos coches tengan igual aspecto.
- **Límite:** una sección trasera independiente por formación; no afirmar patio por gravedad ni un número ilimitado de cortes separados.

## #347 — Plan 137: Agregar preparación y mantenimiento en depósitos

- **Entrar:** **Depósito: preparación y mantenimiento**, `examples/traction_operation/scenario_depot.toml`.
- **Acción:** mantener el tren en la misma vía y dentro de la zona del depósito, detenido, sin tracción, neutro, motores apagados y frenos de mano aplicados. En F9 elegir **Mantenimiento en…**. Cerrar el panel para avanzar; guardar a mitad y reanudar. Comparar cantidades en el HUD de locomotora.
- **Ver o medir:** operación y reservas finitas de aceite/refrigerante, progreso conservado y combustible/baterías/frenos sin reinicios. Las temperaturas continúan con su evolución física. Salir de zona o agotar reservas impide continuar como si el suministro fuera infinito.
- **Límite:** no es un botón de reparación total ni de combustible gratis. El ejemplo con recursos suficientes difiere del ensayo de agotamiento; una reserva insuficiente puede completar una tarea sin eliminar todas las averías.

## #350 — Plan 140: Conducir jornadas de varios servicios con el mismo material

- **Entrar:** **Nueva partida → Ruta y servicio → Campañas y jornadas → Dos jornadas · material propio**, definida en `examples/player_campaign/campaign.toml`.
- **Acción:** conducir la primera jornada y revisar el resultado; iniciar la segunda cuando se habilite. Anotar combustible, energía, equipos, averías y reloj antes/después. Guardar durante cada una, volver al inicio y reanudar.
- **Ver:** se conserva el mismo material y su estado, con salida a la hora prevista o al reloj acumulado si ya se llegó tarde. La segunda no aparece completada por abrir su pantalla; hay que conducirla. El progreso persiste entre inicios.
- **Límite:** un cambio en los archivos del material debe rechazar continuidad incompatible. El menú de campaña por sí solo no prueba dos servicios completos ni continuidad de temporizadores de alimentación.

## #373 — Plan 163: Elegir carpeta de contenido desde la interfaz

- **Entrar:** **Biblioteca → Copias y respaldos → Carpeta para descargar contenido**, tanto en portátil como en Snap.
- **Acción:** usar **Explorar carpeta** y **Usar esta carpeta**, o introducir una ruta absoluta existente y escribible. Instalar una copia pequeña, reiniciar y revisar biblioteca. Probar una carpeta sin permiso, un disco desconectado y un destino dentro de `/snap`; no alterar permisos del paquete.
- **Ver:** se muestra el destino real `openrailsrs-content`, la preferencia persiste y las instalaciones anteriores siguen localizadas. Un error no sustituye la preferencia por una ubicación inválida ni pierde versiones existentes. Retirar una copia de disco externo usa su destino recuperable en ese mismo disco.
- **Límite:** elegir otra carpeta no mueve automáticamente paquetes, partidas o ajustes. La aceptación de confinamiento necesita el artefacto Snap construido, con revisión/hash, no sólo el checkout. Seguir la preparación de Snap al comienzo de esta guía.

## #380 — Plan 170: Precargar recorrido según velocidad e itinerario

- **Entrar:** servicio completo de Chiltern, con el contenido original y una distancia de visión fija.
- **Acción:** conducir suficientemente lejos para cambiar varios sectores; comparar velocidades, volver a cabina y pasar rápidamente a exterior. Conservar una ejecución instrumentada con `scripts/check_viewer_streaming.py` y el mismo punto físico de comparación.
- **Ver o medir:** los sectores próximos a cabeza, cola y trayectoria prevista se preparan antes de necesitarlos. Al detenerse, el entorno visible acaba completo y las colas se vacían; los sectores antiguos se retiran. Revisar `unactivated_near_shapes`, `pending_terrain_tiles`, pendientes de transferencias, `gpu_tiles` y residencia.
- **Límite:** el pronóstico es acotado; no equivale a cargar todo el mapa ni demuestra mejora de P99 por sí solo. Comparar carga inicial y conducción por separado, sin ocultar objetos pendientes alejando la cámara.

## #381 — Plan 171: Priorizar sectores visibles y próximos al tren

- **Entrar:** el mismo servicio, en una estación de muchos objetos y durante entrada a otro sector.
- **Acción:** conducir desde cabina, pasar a **2** y girar a ambos lados, incluido el lado que estaba fuera de visión. Volver sobre un tramo cargado y comparar con la misma secuencia de cámaras en la referencia.
- **Ver o medir:** aparecen primero recursos próximos/relevantes, pero los de detrás también reciben turnos; no quedan ausentes hasta volver a entrar en cabina. El informe registra colas y tiempos de publicación, y la captura final debe estar lista sin shaders pendientes/fallidos.
- **Límite:** una escena todavía cargando no se acepta como completa. Una cola breve no acredita menos tirones: necesita P99/máximos y el mismo trabajo visual del protocolo de rendimiento.

## #383 — Plan 173: Guardar geometría preparada en caché persistente

- **Entrar:** mismo servicio original dos veces con el mismo ejecutable, ruta, `.s`/`.sd`, parámetros y carpeta privada de datos de prueba.
- **Acción:** iniciar una vez con caché nueva y repetir reutilizándola. Conservar `geometry_cache` y `shape_stages`. Para invalidación, trabajar sólo sobre una copia propia del laboratorio: cambiar su `.s`/`.sd` o dañar un derivado de esa caché privada, nunca el contenido instalado del autor.
- **Ver o medir:** los aciertos calientes reutilizan la preparación sin cambiar mallas, texturas o LOD; una fuente/descriptor/código incompatible o entrada corrupta obliga a reconstruir. En el formato nuevo se verifican longitudes, checksum y límites antes de materializar, con cuota agregada de 256 MiB y entrada de hasta 32 MiB.
- **Límite:** el formato Zstd posterior a fase 37 necesita su propia ejecución aceptada; no se atribuyen a él los aciertos históricos ni una mejora de FPS todavía no medida. La caché contiene derivados CPU, no todos los recursos de la GPU.

## #384 — Plan 174: Reutilizar importaciones y señales mediante caché persistente

- **Entrar:** una edición original instalada de Chiltern con TDB y SIGSCR. Para medición usar `python3 scripts/check_native_cache_performance.py --cli target/debug/openrailsrs --route-root /ruta/ROUTES/Chiltern --out-dir tmp/manual-native-cache`.
- **Acción:** conservar importación fresca, fría, caliente y repetida; comparar tamaño y SHA-256 de las salidas, así como aciertos y tiempos. Si se prueba invalidación, usar copias privadas y cambiar una dependencia realmente leída o un script.
- **Ver:** mismo grafo, metadatos y programas compilados byte a byte, con reconstrucción al cambiar dependencia/código o corromper entrada. La lectura caliente no reescribe una entrada ya válida.
- **Límite:** mide importación, no FPS ni todas las señales durante un viaje. Un caché válido de SIGSCR no demuestra compatibilidad con cualquier programa externo; conservar errores nativos y límites del importador.

## #390 — Plan 180: Compartir presupuesto de sombras, vegetación, luces y partículas

- **Entrar:** Chiltern, misma hora/encuadre, con lluvia o nieve intensa.
- **Acción:** en **F10 → Imagen y cabina**, probar **Calidad adaptativa general**, objetivo y degradación permitida; comparar también calidad fija normal/reducida/normal. Ajustar vegetación, luces y partículas desde sus secciones. Mantener faros del tren encendidos y observar la vía.
- **Ver o medir:** cambian presupuestos y trabajo cosmético, mientras reloj, física, controles y faros prioritarios siguen funcionando. Conservar presupuestos efectivos, partículas/luces/vegetación, memoria y tiempos de cuadro; la segunda pasada normal ayuda a detectar el efecto de cachés.
- **Límite:** menos partículas no garantiza más FPS si el cuello está en otro sistema. No bajar calidad de forma oculta en una comparación que pretende medir el mismo contenido.

## #391 — Plan 181: Gestionar presión de VRAM y liberación de recursos

- **Entrar:** recorrido de al menos 8 km en Chiltern, alternando cámaras y repitiendo sectores; luego cambios de rutas comparables.
- **Acción:** usar `python3 scripts/check_viewer_residency.py --viewer /ruta/al/visor --route-root /ruta/ROUTES/Chiltern --budget-mib 1024 --out-dir tmp/manual-residency` y comparar con su política física. Registrar el PCI y la cuenta real del proceso. La observación desde F8 acompaña, pero no sustituye el informe.
- **Ver o medir:** presión y liberaciones efectivas de mallas/imágenes, retirada de sectores lejanos y entorno próximo completo. Distinguir capacidad física, umbral del proceso, VRAM/GTT propia y reserva del asignador; repetir fases equivalentes después de cambiar ruta.
- **Límite:** 1.024 MiB es un umbral para una política, no un máximo duro ni una prueba de agotar la GPU. Una GPU abierta sólo por enumeración no aporta memoria disponible a la elegida. No sumar dispositivos ni tratar lecturas ausentes como cero.

## #393 — Plan 183: Paralelizar tráfico conservando reproducibilidad

- **Entrar:** servicio con tráfico desde el menú para observar señales/posiciones; para equivalencia usar el ejemplo instrumentado `cargo run --locked -p openrailsrs-sim --example traffic_benchmark -- 32` en un turno separado de otras mediciones.
- **Acción:** el ejemplo ejecuta 1/2/4/8 trabajadores con 32 AI más jugador y repite el corredor del repositorio. Comparar `state_sha256`, `initial_load_ms` y `driving_ms` de sus JSON. En una partida, guardar/restaurar mientras ambos servicios ocupan tramos compartidos.
- **Ver o medir:** hashes finales iguales y decisiones/reservas reproducibles entre cantidades de trabajadores; carga separada de 60 segundos simulados de conducción. La UI no tiene que mostrar un selector de hilos para que el ensayo sea válido.
- **Límite:** el benchmark es sin render y usa ejemplos concretos; no acredita el rendimiento gráfico ni más velocidad por usar más hilos. En cargas pequeñas la versión paralela puede ser más lenta.

## #396 — Plan 186: Crear editor gráfico de itinerarios, inversiones y esperas

- **Entrar:** elegir primero una ruta/servicio; **Nueva partida → Ruta y servicio → Crear o editar un servicio propio**. Para un caso sencillo, usar la red del laboratorio propio.
- **Acción:** seleccionar puntos conectados del mapa, comprobar los tramos dirigidos, pulsar **Invertir y regresar**, **Espera +30 s**, **Espera −30 s** y **Deshacer**. Probar origen/destino y **Generar recorrido comprobado**. Guardar con **Guardar servicio propio**, volver al selector e iniciar esa copia. Repetir en ventana 800 × 600, desplazando el cuerpo para alcanzar controles posteriores.
- **Ver:** mapa sin colapsar ni superponerse a botones, cambios reflejados en los tramos y espera válida de 0–1800 s. La copia aparece en `custom-services` bajo datos del jugador; ningún PAT original cambia. Deshacer recupera el estado previo y la partida sigue el sentido guardado.
- **Límite:** la red visible se limita a 128 nodos cercanos. Un regreso desconectado, falta de espacio para cabeza/centro/cola o un tramo inválido se rechaza; el editor no crea vías nuevas.

## #403 — Plan 193: Ejecutar pruebas prolongadas de fugas y acumulación

- **Entrar:** ensayo instrumentado con al menos dos rutas propias y una instalación original completa; no usar una sola captura del menú como prueba prolongada.
- **Acción:** ejecutar `scripts/check_viewer_soak.py` con visor identificado, `--route-root`, `--scenario`, `--routes-json`, `--audio-null --framepace 30 --seconds 7200 --out-dir` en una carpeta nueva. `routes-json` contiene objetos con `path` absoluto al escenario y `route_root` absoluto. El calendario cambia cámara/clima y realiza guardados cada minuto, y cambia recorrido cada tres minutos.
- **Medir:** duración real completa, muestras comparables de RAM/VRAM/GTT, entidades activas, imágenes Main/GPU, fuentes/atlas, audio y colas. Schema 3 exige restauración JSON y SavedGame realmente escrito/leído/restaurado en cada ciclo: al menos 120 éxitos de ambos, cero fallos y cambios de ruta completados. Exigir proceso final con salida 0, captura PNG nueva confirmada por el observador y colas/shaders finalizados.
- **Límite:** `--smoke`, una prueba interrumpida, sólo serializar a memoria o 7.170 s no equivalen a 7.200 s. Comparar a la misma edad real de escena y no esconder crecimientos de recursos GPU con una RAM aparentemente estable. Esta guía no afirma que el candidato actual haya aprobado dos horas.

## #412 — Snap: Chiltern extendido no resuelve Blue Pullman y recomienda copiar recursos dentro de /snap

- **Entrar:** instalación Snap limpia de la revisión que se quiera aceptar; **Nueva partida → País → Reino Unido → Chiltern → Chiltern extendido: Northolt Park → Gerrards Cross**, con **birmingham_pullman**.
- **Acción:** abrir **Ver detalles** antes de instalar nada. Los modelos compartidos incluidos deben resolverse sin otra descarga. Si falta la cabina u otro recurso original, volver a **Biblioteca → Descargas del autor**, instalar Chiltern desde DocMartin o usar una instalación completa existente y **Reauditar esta copia**. Volver al mismo servicio preparado y comenzar cuando estén todos los obligatorios.
- **Ver:** desaparecen faltantes realmente resueltos; auditor, carga y renderer coinciden. Deben verse ocho vehículos con modelos/texturas correctos desde **1** y **2**. F7 conserva las seis paradas del extendido —Northolt Park, South Ruislip, West Ruislip, Denham, Denham Golf Course y Gerrards Cross— y su tráfico; no se sustituye silenciosamente por otra actividad.
- **Persistencia y errores:** anotar versión/revisión/hash, reiniciar y actualizar el Snap conservando biblioteca. Probar cancelación o fallo de descarga sin publicar una instalación incompleta como válida. Los destinos sugeridos son escribibles en Common o en la carpeta elegida; nunca `/snap`. Recursos completos, avisos de compatibilidad y cobertura de oráculos se muestran separados; datos ausentes no se convierten en «0 mm» certificado.
- **Límite:** un menú abierto o un laboratorio sintético no cubren este caso. Tampoco un Snap histórico certifica una fuente posterior: registrar cabina/exterior nuevas del artefacto concreto.

## #413 — Biblioteca: instalar rutas y trenes pegando una URL del origen original

- **Entrar:** **Biblioteca → Descargas del autor → Instalar desde URL**; elegir antes una carpeta escribible si se desea otro disco.
- **Acción:** pegar `https://github.com/DocMartin7644/Chiltern-Route-v4` y pulsar **Comprobar enlace**. Revisar origen, revisión, fecha, tamaño disponible y destino antes de instalar. Para un ZIP directo, usar **Demo Model 1** y su enlace del catálogo. Para contenido sólo de trenes, comprobar `https://github.com/Jeanprisca/SNCF-TGV-Atlantique`; confirmar personalmente su procedencia si el catálogo no la acredita. No probar instaladores ejecutables.
- **Ver:** descarga/extracción/auditoría con progreso y cancelación, publicación completa junto a versiones anteriores y aparición en los selectores. El TGV aporta formaciones globales aunque no añada una ruta francesa. Revisar **SNCF_TGV-325-FR** o **SNCF_TGV-325-FR_Sharpe** y sus doce vehículos; otras formaciones pueden seguir incompletas. Para conducirlo, elegir alimentación/tensión compatibles y revisar el diagnóstico; el laboratorio propio de 25 kV no es una ruta original francesa.
- **Errores y persistencia:** probar enlace inválido o página sin descarga, cancelar y cortar red durante una copia de prueba. Comprobar que no aparece como instalación válida ni reemplaza la anterior. Falta de espacio o archivo corrupto deben conservar diagnóstico y copia previa. En Snap y portátil, reiniciar y verificar el destino/edición; las pruebas destructivas de archivos se limitan a datos privados de QA.
- **Límite:** no se descargan dependencias desde espejos ni se ejecuta código del paquete. Los ZIP ajenos al catálogo y orígenes no verificados requieren revisión/instalación manual según el caso. Las protecciones de URL/redirección, red interna y extracción se validan con pruebas automatizadas locales; no es necesario enviar paquetes maliciosos a un servidor real. Un tren con todos sus archivos no queda certificado en física, sonido o scripts C#.

## Registrar el resultado

Por cada apartado probado, conservar número de issue, versión/hash del programa, ruta/edición, formación, pasos ejecutados, resultado esperado/observado, captura o salida y límites. Marcar **no ejercitado** si falta el recurso o la declaración necesaria, en lugar de aprobar un efecto inexistente. Separar inspección manual, ensayos automáticos, medición y comparación con OR. Las referencias ampliadas están en [operaciones](TRAIN_OPERATIONS_FIFTH40.md), [validación visual](PLAN_40_5_VISUAL_QA.md), [rendimiento](PERFORMANCE_FIFTH40.md) y [contenido](OFFICIAL_CONTENT.md).

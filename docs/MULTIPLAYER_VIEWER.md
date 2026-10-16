# Sesiones compartidas en el visor

El anfitrión ejecuta la física y la circulación. Cada visor se conecta por TCP,
recibe el mundo del anfitrión y envía mandos con identidad, rol, concesión y
secuencia. El anfitrión también usa esta autoridad para sus propios mandos.

Esta implementación admite hasta ocho identidades, incluido el anfitrión, y
dieciséis servicios instalados. Es una sala de red local: escucha inicialmente en
`127.0.0.1:30500`. Para otra computadora se elige explícitamente una dirección de
la red local. El transporte no cifra la conexión ni ofrece descubrimiento de
servidores públicos. Todos necesitan la misma versión de la ruta, el servicio y
las formaciones; la sala verifica su firma antes de conceder un puesto. Los
archivos de contenido no se transfieren por este protocolo.

## Probar desde el menú

El manifiesto del Snap declara `network-bind` para alojar una sala. Los ensayos
de esta entrega se ejecutaron con el visor local; no certifican una instalación
confinada ni publican una revisión nueva del Snap.

1. En cada visor, abrir el ejemplo **Multijugador: dos trenes de vapor**, de
   `examples/multiplayer/scenario.toml`. Utiliza contenido propio ya incluido.
   También se puede iniciar con
   `openrailsrs-viewer3d --live examples/multiplayer/scenario.toml`.
2. Abrir **F10 → Multijugador**. En el primer visor elegir **Despachador** y
   **Abrir sala (Host)**. Dejar la clave vacía genera una aleatoria. El panel
   muestra la dirección efectiva y permite **Mostrar/ocultar clave** y
   **Copiar clave**. Una dirección con puerto `0` sirve para pruebas locales: el
   panel muestra el puerto asignado realmente.
3. En un segundo visor, introducir dirección, un nombre diferente y la clave
   —también mediante **Pegar clave**—. Elegir **Conductor**, seleccionar
   **Jugador** y pulsar **Conectar**. Otro conductor elige **vapor_2**. El panel
   debe mostrar ambas identidades conectadas y sus servicios distintos.
4. Con cada conductor seleccionar **Adelante**, liberar el freno y subir el
   regulador. Deben moverse los dos trenes por sus vías paralelas. Los mandos de
   uno no cambian el regulador del otro. Al cerrar el panel se usan las teclas de
   regulador, frenos, inversor, bocina, luces y limpiaparabrisas configuradas en
   F10; todos esos mandos pasan por el anfitrión. El panel conserva las tres
   posiciones del inversor, incluido **Neutro**.
5. En el despachador, escribir **free_j** en **Cambio** y alternar
   **Recta/Desviada**: es un cambio libre en el pequeño patio. Escribir
   **signal_b** en **Señal** y ordenar **Roja/Automática**. Su aspecto debe llegar
   a los otros visores. El cambio **j** está conectado a la vía ocupada por el
   primer tren: la orden debe rechazarse mientras siga ocupado o reservado.
   El mapa M permite consultar esos identificadores. El rol de conductor o
   espectador no puede imponer cambios ni señales.
6. Un visor adicional elige **Fogonero → Jugador**. Sus botones de fuego, tiro,
   inyectores, soplador y automático/manual deben cambiar la misma caldera que ve
   el conductor de Jugador. El conductor conserva regulador, corte, grifos y
   frenos; el fogonero no puede conducir ni aplicar órdenes de despacho. Cambiar
   el fogonero a **Espectador** devuelve el trabajo al fogonero automático.
7. Con **Espectador**, pulsar **Seguir** junto a cualquiera de los servicios y
   elegir **Exterior**, **Orbitar** o **Libre**. Cerrar el panel para mover la
   cámara. Se sigue el modelo real ya existente en la escena. No aparece un
   tren adicional y ninguna tecla de conducción mueve un tren. Sólo se reciben
   hasta ocho poses públicas; el espectador no recibe guardados ni estados de
   scripts o controladores.
8. Desconectar o cerrar el segundo conductor. En el anfitrión deben quedar su
   nombre **desconectado**, regulador cero y freno aplicado en vapor_2. El puesto
   permanece reservado. Volver con el mismo nombre desde el mismo equipo debe
   recuperar ese puesto, sin crear otro tren ni repetir órdenes pendientes. Si
   se desconecta el fogonero, se activa el automático. Se puede cambiar de rol
   expresamente para liberar un puesto; una desconexión por sí sola no lo libera.
9. En el anfitrión pulsar **Guardar sesión completa**. El panel indica la ruta
   real del archivo, dentro del directorio de datos del usuario, en
   `multiplayer/server.json`. Desconectar la sala, mantener cargado el mismo
   escenario y pulsar **Recuperar sesión del host**. Los demás clientes pueden
   reconectarse con la misma identidad. Deben continuar la posición, composición,
   horario, presiones, calderas, señales, cambios, reservas, clima y roles. Se
   renueva la concesión de red y se cierran los reguladores hasta recuperar el
   control: recuperar un guardado no arranca un tren desatendido.
   Si había reguladores locales, el conductor pulsa **Recuperar reguladores ·
   mando del conductor**. Esta orden autorizada elimina esos overrides y devuelve
   las locomotoras al mando compatible de la formación; después se liberan los
   frenos y se abre el regulador. No cambia sus masas, energías ni calderas.
10. Cerrar la partida o el visor. Se cierran los sockets y se espera a los hilos
    de red propios; no se crea un servidor externo que quede ejecutándose.

La cabina habitual corresponde al servicio principal. Al conducir o seguir otro
servicio se ofrece su vista exterior y los mandos del panel de red. No se promete
una cabina interior intercambiable para todos los servicios. La tripulación de
vapor actúa sobre la locomotora de la cabina activa. Otras calderas conservan sus
estados en la simulación y en el guardado; sus reguladores locales se cierran al
perder al conductor. El control individual de otra locomotora de la misma
formación no está expuesto todavía en este panel.

El perfil nativo de vapor OR 1.6.1 sólo admite fogonero manual. Si ese fogonero
se desconecta o cambia de rol, se cierran también los reguladores locales y se
aplica el freno del tren. El conductor debe recuperar una tripulación para
continuar con ese perfil; no se activa un automático que el equipo no admite.

La sala usa el TCS Rust de todos los servicios. Host, conexión y recuperación
rechazan antes de abrir sockets una sesión con host C# externo: el estado del
cliente no debe reconstruir un proceso C# por cada cuadro de red. La opción C#
de compatibilidad sigue disponible en las partidas individuales.

F9, B, patio y el mapa siguen disponibles para consulta. Sus operaciones físicas
locales, la selección de otra formación, cargar una partida individual y los
cambios locales de clima no sustituyen la autoridad de red: se usan los mandos de
la sala. Abrir el panel no pausa a los demás jugadores. Los clientes tampoco
consultan una API meteorológica propia: reciben hora real, zona, ubicación,
muestra y atmósfera del anfitrión.

## Reconexión y archivos privados

Cada nombre local obtiene una credencial aleatoria privada en
`multiplayer/identities/<hash-del-nombre>.key`. Dos visores con nombres distintos
no reescriben un archivo de identidades compartido. El nombre y la clave de sala
no permiten ocupar la identidad privada de otra persona. Si se quiere recuperar
esa identidad en otra computadora, hay que conservar su archivo privado.

El guardado del anfitrión contiene las credenciales necesarias para restituir
las asignaciones. Los nuevos archivos y sus temporales tienen permisos `0600`
en Unix. Las credenciales no aparecen en el roster ni en los cuadros de red;
`Debug` las oculta. No adjuntar el guardado completo ni los archivos `.key` a un
issue público. El ejemplo no contiene claves de una sala real.

Una orden lleva época de partida, concesión de conexión y secuencia. El servidor
rechaza sockets sustituidos, saltos, números fuera de rango y operaciones de otro
rol. Conserva el resultado de las últimas 64 órdenes por identidad para responder
a reintentos sin ejecutarlos otra vez. El guardado conserva esa posición; una
restauración cambia la época. No se aceptan mensajes de cliente que publiquen un
mundo, un tren o una reserva arbitrarios.

El tamaño máximo de solicitud es 16 KiB y el de un estado es 8 MiB. Hay límites
de conexiones, órdenes pendientes y 40 solicitudes por segundo y cliente. Un
cliente lento conserva sólo el estado más reciente, separado de las
confirmaciones de órdenes. Un espectador recibe un máximo de ocho trenes. La
publicación inicial es de 5 Hz; este alcance no promete interpolación de red ni
sincronización de cada cuadro de render.

## Arranque automatizado del mismo visor

La interfaz de menú es la entrada habitual. Para ensayos se pueden configurar
estas variables antes de iniciar el visor con `--live`:

- `OPENRAILSRS_NETWORK_HOST=127.0.0.1:30500` o
  `OPENRAILSRS_NETWORK_JOIN=127.0.0.1:30500`.
- `OPENRAILSRS_NETWORK_IDENTITY` y `OPENRAILSRS_NETWORK_ROLE`: `driver`,
  `fireman`, `dispatcher` o `spectator`; también se aceptan sus nombres españoles.
- `OPENRAILSRS_NETWORK_TRAIN=Jugador` o `vapor_2` cuando el rol necesita tren.
- `OPENRAILSRS_NETWORK_KEY_FILE`: archivo privado con la clave de sala. La clave
  no es un argumento visible en la línea de procesos. Si no puede leerse, no se
  abre ni se conecta la sala automáticamente.
- `OPENRAILSRS_NETWORK_RESTORE=1`: recuperar el guardado del anfitrión después de
  cargar el mismo escenario. Conserva los roles del guardado; la identidad del
  anfitrión también debe coincidir con su credencial local.

## Pruebas ejecutadas y evidencia

Las **14 pruebas del bloque multijugador pasaron** en la primera ejecución
conjunta: siete de protocolo y transporte, cinco del plugin del visor y dos de
persistencia. El [extracto de la salida real](fixtures/compatibility/twenty-20261015/multiplayer/cpu-tests-excerpt.txt)
y el [resumen con hashes](fixtures/compatibility/twenty-20261015/multiplayer/qa-summary.json)
conservan su procedencia. Esa primera ejecución tuvo fallos en otros bloques;
este resultado se refiere a multijugador y no certifica todo el workspace.

Una prueba posterior de fogonero nativo amplió el bloque a **15 pruebas
aprobadas**: ocho de protocolo y transporte, cinco del plugin y dos de
persistencia. Ese resultado de bloque está registrado también en la salida de
check05; aquella ejecución completa terminó con otro fallo y no se presenta
como un pase global. La prueba añadida rechaza el fogonero automático no
soportado y comprueba el cierre seguro de sus reguladores locales.

Las ocho pruebas de `crates/openrailsrs-game/tests/multiplayer.rs` verificaron
permisos, dos conductores, fogonero, duplicados y huecos de secuencia, rechazo de
época/concesión viejas, cambio atómico de rol, restauración, solicitudes
malformadas, límites de trama y TCP real con reconexión y cierre. Incluyen mandos
y guardado del modelo real de caldera.

Las cinco pruebas de `crates/openrailsrs-viewer3d/src/multiplayer/tests.rs` usaron
el **mismo MultiplayerPlugin del visor** en aplicaciones Bevy sin GPU. El caso
principal conectó cinco identidades simultáneas: despachador anfitrión, dos
conductores en trenes distintos, fogonero y espectador. Comprobó sus efectos
físicos, permisos separados, cámara de seguimiento, desconexión, reconexión y
cierre. Ocho identidades es el límite del protocolo; no se ensayaron ocho a la vez.

Otro caso guardó una sesión TCP en curso con dos conductores, cerró los extremos,
restauró un mundo cargado independientemente y reconectó la identidad. Conservó
roles, servicios, presión, reservas, hora, zona y clima con semilla, y renovó la
época de autoridad. Los valores físicos guardados se compararon antes del
siguiente paso de simulación; el regulador quedó cerrado para reanudar de forma
explícita.

El caso `cooperative_double_steam_keeps_both_thermodynamic_ledgers_in_the_server_save`
usó `examples/physics_lab/scenario_double_steam.toml`: guardó las dos calderas,
masas, energías, receptor compound y reguladores locales. Verificó la igualdad
termodinámica al recuperar, la reconexión del fogonero y el cierre seguro de los
reguladores. Después de **Recuperar reguladores**, ambas locomotoras produjeron
fuerza positiva y la formación volvió a moverse.

La prueba de latencia intercala un proxy TCP con **60 ms de demora por sentido**.
Pasó las condiciones declaradas: RTT desde 100 ms inclusive hasta 4 s exclusive,
entrega en menos de 4 s y reconexión en menos de 5 s. El log conserva el PASS,
pero no imprime el valor exacto de RTT de ese ensayo. Estos plazos contemplan la
publicación de 5 Hz y no son medidas de FPS.

También se ejecutaron tres capturas del visor normal con **Vulkan en AMD Radeon
RX 7600 (RADV NAVI33)** a 1280×720: [despachador anfitrión](fixtures/compatibility/twenty-20261015/multiplayer/host.png),
[conductor](fixtures/compatibility/twenty-20261015/multiplayer/driver.png) y
[espectador](fixtures/compatibility/twenty-20261015/multiplayer/spectator.png).
El anfitrión permaneció abierto y los clientes se capturaron sucesivamente, con
un máximo de dos visores simultáneos. El fogonero quedó cubierto por los ensayos
CPU; no se presenta una cuarta captura GPU.

Los tres PNG se inspeccionaron: los paneles iniciales se leen, entran completos
y muestran los mandos del rol correspondiente. Los clientes mostraron RTT de
25 ms por loopback. La diferencia entre el tiempo del estado mostrado y el
reloj de render fue 0,05 s en los clientes y 0,10 s en el anfitrión; no es una
medida de sincronización de relojes entre máquinas. Los tres visores y sus
compositores privados terminaron con código 0, sin procesos propios restantes.
La [revisión de píxeles](fixtures/compatibility/twenty-20261015/multiplayer/pixel-review.json)
queda separada del recibo original de captura. **El desplazamiento con rueda
requiere todavía la comprobación manual**; estas imágenes revisan el panel
inicial. Se publican imágenes y recibos saneados, sin claves, roster completo,
guardados de servidor ni directorios privados del jugador.

Para repetir las pruebas dirigidas con el perfil del workspace:

```sh
cargo test --workspace --all-features --test multiplayer -- --test-threads=1
cargo test --workspace --all-features --lib multiplayer:: -- --test-threads=1
```

Las pruebas CPU utilizan identidades desechables en memoria y archivos
temporales. No abren ventanas, no usan GPU, no ejecutan Wine y no consultan
servicios de clima. El ensayo de panel renderizado se registra por separado;
ninguno de estos resultados certifica una sesión WAN, ocho visores simultáneos,
interpolación de red, una sesión prolongada ni audio compartido.

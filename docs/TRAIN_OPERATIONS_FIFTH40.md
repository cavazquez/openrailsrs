# Operaciones del lote de 40 issues

La referencia de compatibilidad sigue siendo Open Rails 1.6.1. Los ejemplos de esta guía usan material propio y prueban operaciones del jugador; no certifican todos los trenes de la comunidad. Las rutas y formaciones del autor se conservan sin modificarlas.

Nueva partida comienza por **País**. Para los laboratorios de esta guía, elegí **Todos los países** y después el recorrido. El país limita las rutas y servicios, pero las formaciones siguen siendo globales: se puede elegir un tren francés para una ruta británica. Los avisos de electrificación, trocha y recursos permanecen visibles. **Sin dato de país** reúne paquetes sin una declaración conocida; no se deduce la procedencia por el nombre del archivo.

## Adelantamientos y encuentros (#335, #339)

En **Nueva partida → Ruta y servicio → Buscar servicios**, elegí **Tráfico: adelantamiento de local y expreso** o **Tráfico: encuentro en vía única**. También se pueden abrir directamente:

```bash
./run-viewer3d.sh examples/traffic_passing/scenario_overtake.toml --live
./run-viewer3d.sh examples/traffic_passing/scenario_meet.toml --live
```

El local empieza junto a su parada. Abrí sus puertas, completá el intercambio y cerralas. F9 muestra el apartadero elegido y el servicio al que debe esperar. Avanzá al apartadero a velocidad de maniobra y detenete antes de su salida; el límite mostrado baja al acercarte al punto de espera. En la cámara exterior debe circular el otro tren por la vía principal. El local sale sólo después de que la cola del otro servicio despeje el corredor. La parada del local se conserva al adoptar la alternativa.

Guardá con F8 mientras esperás y reanudá esa partida desde el menú. La espera y el tráfico deben continuar en sus posiciones. Un apartadero corto u ocupado produce un diagnóstico y no cambia la posición ni elimina paradas. Los planes usan alternativas declaradas sobre la red real y reservas compartidas; no inventan vías ni liberan el corredor por un temporizador.

## Cambio de conductor (#341)

Elegí **Cambio de conductor** en los ejemplos. Hay una formación del jugador y otra locomotora estacionada en el ramal. Detené ambas, poné el inversor del jugador en neutro y abrí F9. Elegí **Conducir** junto al otro servicio.

El visor reconstruye los modelos y la cabina para el nuevo tren. Deben conservarse sus frenos de mano, combustible, equipos, hora y posición. El servicio abandonado vuelve al conductor automático y continúa bajo las mismas reservas. Una locomotora estacionada conserva sus frenos de mano: liberarlos y preparar sus equipos forma parte de la salida. Guardar y reanudar conserva quién conduce cada tren; también se admiten varios cambios sucesivos.

Una transferencia hacia un tren en marcha, terminado o sin locomotora se rechaza. Después de recoger otra formación se debe completar esa operación antes de otro cambio de conductor. El guardado previo y la reconstrucción se realizan antes de aplicar la transferencia, de modo que un error de archivo o de recursos conserva la partida anterior.

## Recoger otra formación (#342)

Elegí **Recoger formación**. El enganche trasero de la formación del jugador está junto al delantero de la formación estacionada. En F9 elegí **Recoger formación**. Deben estar detenidas, sin tracción y con todos los vehículos del donante asegurados con freno de mano. Contacto admitido: menos de un metro entre los extremos compatibles.

Después del acople deben verse cuatro vehículos y desaparecer el servicio donante de la lista de tráfico. F9 debe mantener sus reservas de combustible y presiones. Conectá la manguera y abrí las dos llaves del nuevo corte, liberá los frenos de mano de los vehículos recogidos y comprobá una aplicación y liberación del freno antes de conducir. Guardá y reanudá: no deben aparecer vehículos duplicados.

El alcance inicial es extremo trasero a delantero, diésel o eléctrico, TCS Rust y dinámica compatible. Se rechazan frenos incompatibles, vapor, scripts externos o una disposición que desplazaría coches a otro ramal. Se mantiene la comprobación física de todos los coches al unirlos; el contacto de un solo enganche no permite trasladar una formación entre vías.

El [oráculo de acople](fixtures/compatibility/fifth40-20261013/coupling-original.json) ejecuta los métodos reales `UncoupleBehind`, `CheckForCoupling` y `ComputeCouplerForces` del DLL fijado de Open Rails 1.6.1, sin modificarlo. Registra la unión de cuatro más cuatro vehículos originales, conservación de presiones y combustible y retiro de la ocupación/reservas del donante. El cliente Rust conserva esos estados al unir y reanudar; después conduce los ocho vehículos 15,57 m en veinte segundos sobre una vía propia de prueba. La nueva ejecución contra las bibliotecas de la fuente 37 repitió los resultados de la fuente 32 byte a byte; los recibos conservan compilador, bibliotecas y fuente por separado. El depósito también conserva exactamente los estados observados en su comparación anterior.

Los tres estados de equilibrio —sin carga, tensión y compresión— difieren menos de 0,000385 N, frente a una tolerancia de 0,1 N; la aceleración usa una tolerancia de 0,00001 m/s². Se comparan las mismas masas y cargas externas, con la precarga elástica de Rust calculada a partir de esas entradas. El modelo rígido de OR y el elástico de Rust tienen distintas extensiones y transitorios; este ensayo no certifica impactos en movimiento ni sus curvas completas. OR exige movimiento con signo para su acople automático, mientras el comando manual del visor exige detener y asegurar la formación.

## Clasificación en patios (#343)

Elegí **Patio: clasificar y recoger vagones por destino** o **Patio: dos cuadrillas, dos destinos**. F9 muestra cada lote, su destino, vehículos y vía. Aplicá los frenos de mano de los coches 2 y 3 y desacoplá detrás de la locomotora. El lote Norte sólo aparece entregado cuando ambos vagones completos quedan dentro de su zona y asegurados. Volver a recogerlos retira esa aceptación.

En el ejemplo de dos cuadrillas, cambiá al servicio Este para entregar el otro lote. La identidad de los vagones sigue perteneciendo a su formación de origen incluso después de recogerlos; no sirve sustituirlos por coches de igual aspecto. Guardar y reanudar conserva la composición, la entrega y las reservas. Cada formación puede mantener una sección trasera desacoplada; este ejemplo no añade un patio por gravedad ni varios cortes independientes de la misma locomotora.

## Autorizaciones de maniobra (#340)

En una partida con señal de protección, abrí M, seleccioná una señal de tu itinerario a menos de 100 m y solicitá la autorización. La UI solicita 200 m; el núcleo admite de 10 a 500 m. F9 muestra distancia restante, vencimiento y límite de 15 km/h. La asistencia frena antes del extremo autorizado, aunque la asistencia habitual de tráfico esté desactivada.

No se autoriza entrar hacia otro tren ni una sección estacionada. Las ruedas de tu propia formación antes de la señal no bloquean la solicitud. La autorización vence a los 120 s y se revoca al invertir el itinerario, rebasar distancia/velocidad o cambiar la ocupación. El guardado conserva el tiempo restante. Una señal en otro itinerario o una distancia que no alcanza más allá de ella produce un error claro. Es una autorización acotada del despachador; no sustituye la lógica completa SIGSCR de todas las rutas.

## Depósito (#347)

Elegí **Depósito: preparación y mantenimiento**. En F9 prepará el tren detenido, sin tracción, en neutro, con motores apagados y frenos de mano aplicados. Elegí el trabajo del depósito presente en la misma vía. Se recuperan aceite y refrigerante mediante las reservas finitas y tiempos declarados del equipo; no se rellena combustible ni se borran averías arbitrariamente.

Guardá a mitad del trabajo y reanudá. Deben conservarse avance, reservas restantes, combustible, baterías, presiones y temperaturas. Si falta una reserva o el vehículo sale de la zona, la recuperación no debe seguir como si el depósito fuese infinito. La UI informa la operación y los errores de preparación.

El [ensayo de continuidad](fixtures/compatibility/fifth40-20261013/depot-state-continuity.json) parte de ambas baterías desconectadas, 469 L por motor, frenos aplicados y una parada en fase de intercambio con penalización previa 37,5. Guarda a mitad de la tarea y compara la recuperación con un tren que recibe los mismos controles y tiempo, sin mantenimiento. Combustible, baterías, frenos y servicio resultan idénticos al control; la temperatura de freno cambia de 76,57 a 73,68 °C en ambos. El mantenimiento transfiere 50 L de aceite y 70 L de refrigerante. Con sólo un litro de cada reserva, las averías permanecen y repetir el ciclo después de reanudar no crea material; completar el ciclo no significa reparar todos los motores.

## Jornadas (#350)

En **Nueva partida → Campañas y jornadas**, elegí **Dos jornadas · material propio**. Conducí la primera y revisá su resultado. La segunda se habilita al completar la primera y presenta su hora de salida. El inicio conserva el reloj acumulado si terminaste después de la hora prevista, además de combustible, energía, equipos y averías del mismo material.

Los temporizadores de relé y alimentación conservan su edad al comenzar la siguiente jornada, cuyo reloj de simulación empieza de nuevo. La energía acumulada se conserva y su próximo intervalo se calcula sobre el reloj nuevo, sin reiniciar los equipos ni dejar marcas de tiempo futuras.

Guardá durante una jornada, volvé al menú y reanudá. Al terminar, el progreso se guarda en la carpeta de datos del jugador. Los escenarios de la campaña siguen sin cambios. Si cambiaron los archivos de los vehículos, se rechaza la continuidad en lugar de aplicar reservas de otra formación.

## Editor gráfico de itinerarios (#396)

En **Nueva partida → Crear o editar un servicio propio**, abrí una ruta y usá sus puntos del mapa. La línea seleccionada muestra los tramos dirigidos. **Invertir y regresar** requiere las vías de regreso y conserva la posición de toda la formación. **Espera ±30 s** modifica la espera del último tramo, entre 0 y 1800 s. **Deshacer** recupera el estado anterior.

Guardá el servicio y elegilo en Nueva partida. Se escribe en `custom-services` dentro de los datos del jugador; no se sobrescribe ningún PAT. Una inversión desconectada, espacio insuficiente para el tren, vía desconocida o espera inválida se rechaza. El editor valida todos los tramos y la continuidad de cabeza, centro y cola, además de los horarios y recursos. El mapa muestra hasta 128 nodos cercanos para mantener una interfaz manejable en redes grandes.

## Actividades originales (#333)

La importación resuelve ACT → TRF → SRV → PAT → CON y el horario sobre las plataformas TDB originales. Los servicios anteriores al inicio de la actividad se simulan previamente con el mismo despachador; se conservan posiciones, combustible, frenos y reloj, en lugar de dibujarlos desplazados sin dinámica. Los errores de recursos identifican el servicio afectado y no eliminan trenes silenciosamente.

Para probar una instalación original, elegí la actividad del autor en su biblioteca, revisá el detalle de recursos y comenzá la partida. F9 debe mostrar los servicios originales activos y sus estados; M debe reflejar su ocupación. Guardar y reanudar conserva su circulación. Esta comprobación depende de los recursos realmente instalados: un paquete descargado no acredita todos los scripts ni la física de sus locomotoras.

El caso de aceptación utiliza **Chiltern v4 → RS_Football Special**, con la formación original del mismo nombre. F7 muestra Banbury General, Bicester North, Princes Risborough y High Wycombe; F4 y M permiten seguir las señales y la ocupación de los veinte servicios ACT originales, que aparecen según sus horas de salida. Un servicio previsto después del final del recorrido del jugador puede seguir esperando su salida.

La aceptación completó las cuatro paradas en 3781 segundos y 62.664,95 metros, con veinte AI y ningún servicio fallido ni error SIGSCR. Trece AI habían completado su itinerario y siete seguían aproximándose al finalizar el jugador; no se adelantan sus relojes para declararlos terminados. El recibo separa ese alcance del ensayo de restauración y conserva la incidencia del wrapper de ejecución.

Las inversiones y esperas conservan el itinerario PAT y todos los vectores que aún están bajo la formación. El despachador debe esperar cuando ese corredor está ocupado; no debe inventar un desvío que desconecte el tramo siguiente. Las agujas próximas a una señal se reservan antes de despejarla, respetando su margen de protección, para evitar que una formación se bloquee a sí misma al entrar en el siguiente bloque. La ocupación de otros trenes sigue impidiendo ese movimiento.

## Contenido sólo de trenes (#413)

En **Biblioteca → Descargas del autor → Instalar desde URL**, se puede comprobar el [repositorio original SNCF-TGV-Atlantique](https://github.com/Jeanprisca/SNCF-TGV-Atlantique). Revisá autor, revisión y resumen antes de confirmar la instalación. La biblioteca conserva los originales en la carpeta de datos del jugador; no se copian al repositorio ni dentro de `/snap`. Un paquete de trenes puede tener cero rutas y añadir sus formaciones a la lista de trenes de las rutas instaladas.

La auditoría de ese paquete encontró veinte CON originales y dos formaciones 325 completas: **SNCF_TGV-325-FR** y **SNCF_TGV-325-FR_Sharpe**, de doce vehículos. Las otras dieciocho mantienen los diagnósticos de recursos faltantes. La actualización MAJ instalada por separado sigue incompleta incluso con la base presente; no se superponen archivos para ocultar sus dependencias. El [recibo del caso](fixtures/compatibility/fifth40-20261013/train-only-original.json) separa la instalación, auditoría, carga y aceptación visual.

Para probar una formación completa, elegila en **Nueva partida → Tren**, abrí **Ver detalles** y comprobá recursos y compatibilidad de electrificación. Usá una ruta electrificada adecuada y verificá la cabina con 1 y el exterior con 2. Recursos completos no certifican scripts C#, sonidos o física completa del modelo. El laboratorio usado en QA es un escenario propio de 25 kV; no es una ruta original de ese autor.

La carga del candidato 25 abrió la cabina 3D completa, pero su exterior se rechazó por matrices anónimas omitidas en el parser. La corrección conserva los 134 índices originales y el candidato 26 ya muestra carrocería, nariz y techo ensamblados en exterior, además de la cabina completa. Las capturas cargaron doce vehículos, sin shaders fallidos o pendientes; no se usan para afirmar rendimiento ni paridad física. El Snap estricto alpha.6 x15 repitió la auditoría (20 CON, dos listas). El candidato x16 volvió a cargar el selector, cabina y exterior normales, con doce vehículos y sin shaders pendientes o fallidos. Se inspeccionaron los PNG nuevos y se registró `ScreenshotCaptured`. El suelo magenta es la geometría del laboratorio propio, no un escenario del autor.

También se recorrió el menú mediante clics y escritura reales: **Nueva partida → País → Reino Unido → Chiltern → servicio → Tren → buscar SNCF_TGV-325-FR_Sharpe → elegir → volver a Ruta y servicio**. El país británico y la ruta elegida se conservan al seleccionar el tren francés. Su aviso eléctrico permanece; esa combinación no se inició durante la prueba. El ensayo no fijó páginas o formaciones mediante variables de captura y conservó doce imágenes del flujo. Las pruebas x15/x16 quedan como antecedentes; el Snap final **alpha.6 x17**, de fuente limpia `bd0b5e4`, repitió el flujo real, la auditoría y las tres vistas con PNG nuevos y callbacks. La auditoría mantiene dos formaciones completas y dieciocho incompletas. Se separan auditoría, carga e inspección visual, sin certificar paridad física, sonido, scripts C# ni la prueba de estabilidad de memoria.

## Pruebas automatizadas

`check.sh` incluye los ensayos de encuentros, patios, transferencias, mantenimiento, guardados, editor y la regresión de oráculos fijados. Las capturas y los resultados de aceptación se registran por separado con hash del binario y configuración. Las extensiones propias se validan con invariantes de composición, posición, frenos y reservas; los oráculos Open Rails se usan para los sistemas equivalentes.

La evidencia automatizada y las capturas tienen alcances distintos:

- `passing_services` conduce los ejemplos de encuentro y adelantamiento hasta la llegada de ambos servicios, conserva la parada del local, restaura durante la espera y rechaza un apartadero corto.
- `parallel_traffic` y `saved_game` prueban transferencias sucesivas, estados conservados y reconstrucción atómica. Las capturas `control` y `coupling` del [recibo de operaciones](fixtures/compatibility/fifth40-20261013/graphics-and-operations.json) muestran tres y cuatro vehículos respectivamente; no certifican todas las cabinas de la comunidad.
- `yard_operations` verifica contacto, identidad, frenos y entrega de lotes. El ensayo de depósito restaura a mitad del trabajo, completa 50 L de aceite y 70 L de refrigerante y mantiene el combustible; sus temperaturas continúan evolucionando con el modelo.
- `shunt_authority` conduce hasta detenerse antes del extremo autorizado con la asistencia habitual desactivada, y comprueba límite, expiración, ocupación e inversión.
- `campaign_play::two_complete_driven_jornadas_keep_material_and_resume_mid_service` completa ambas jornadas de más de 9,9 km y reanuda cada una después de 60 s, conservando material, combustible, reloj y alimentación. La captura del menú 800×600 acredita presentación, sin reemplazar esas conducciones.
- `service_builder::graphical_itinerary_validates_all_legs_reversal_wait_undo_and_separate_save` comprueba inversiones, esperas, deshacer y guardado separado. La captura del editor acredita la UI, sin certificar todos los recorridos que un jugador pueda crear.
- [La actividad completa original](fixtures/compatibility/fifth40-20261013/native-activity-complete.json) y [su restauración](fixtures/compatibility/fifth40-20261013/native-activity-resume.json) son dos ensayos separados. El primero conserva el exit 143 del wrapper junto al resultado final Completed; el segundo verifica veinte AI entre 20→25 s y el rechazo atómico de un guardado de tráfico corrupto.

El [registro de `check.sh`](fixtures/compatibility/fifth40-20261013/check.json) identifica el binario y las pruebas ejecutadas. La aceptación del Snap final y las capturas TGV corregidas se registran aparte; los resultados del núcleo no se presentan como pruebas del paquete publicado.

Los clientes de acople y depósito se pueden repetir contra librerías coherentes ya compiladas. Sus salidas y fuentes de OR se verifican por hash, sin incluir bytes de vehículos descargados en Git:

```bash
python3 scripts/capture_coupling_reference.py --content /ruta/al/Chiltern-original --out-dir tmp/coupling-or
python3 scripts/capture_coupling_comparison.py --stock /ruta/al/Chiltern-original/TRAINS/TRAINSET/RF_Blue_Pullman --reference tmp/coupling-or/reference.json --deps-dir target/release/deps --out-dir tmp/coupling-rust
PYTHONPATH=scripts python3 -m unittest -v test_coupling_reference
```

La captura original requiere la instalación fijada OR 1.6.1 y el prefijo Wine privado; la comparación necesita los `rlib` coherentes de una compilación anterior. Los archivos del autor se leen o se copian únicamente a la carpeta de QA ignorada, sin modificarlos.

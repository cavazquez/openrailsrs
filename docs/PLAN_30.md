# Treinta mejoras de conducción, presentación y diagnóstico

Selección del 11 de octubre de 2026. Referencia fija: **Open Rails 1.6.1**; Bevy 0.19.1 y Hanabi 0.19.0.

La selección cubre tren, cabina, entorno, audio, guardado y evidencia. El código conserva los recursos originales y adapta la presentación a Bevy. No cambia las tolerancias de los oráculos físicos.

## Preparar la prueba manual

Con el visor recién compilado, abrí `./target/debug/openrailsrs-viewer3d --menu` y seleccioná la versión de Chiltern instalada. También podés seleccionar ejemplos de `traction_operation` y `electric_supply` en Nueva partida. Para los ensayos originales hace falta el contenido instalado fuera de Git. `./scripts/run_chiltern_service.sh` permite recompilar e iniciar si definís `CHILTERN_ROUTE` con la carpeta `ROUTES/Chiltern` correspondiente. Los binarios previamente distribuidos no incorporan cambios del código hasta publicar otro paquete.

`1` cabina, `Alt+1` 2D/3D, `2` exterior, `V` modos de limpiaparabrisas, `F6` ayuda/teclas, `F8` información avanzada, `F9` operaciones y `F10` ajustes. Los controles de cabina, faros, sonidos y agua necesitan recursos compatibles declarados por su autor. Al cambiar un ajuste persistente pulsá Guardar ajustes.

## Comprobación del lote

Estado: implementado y verificado. [Registro de aceptación](fixtures/plan30/2026-10-11/acceptance.json).

Pasaron `check.sh` (1.823 pruebas Rust y 94 Python), las tres suites de oráculos y la comprobación adicional de once perfiles del TRAINSET original. Nueve capturas cortas de Chiltern v4 pasaron en AMD RX 7600/Vulkan, incluidas cabina/exterior nocturnos, faros apagados/bajos/altos, luna llena, arena con lluvia, nieve, efectos CPU y vista elevada de Denham. Una captura adicional comprueba la superficie HWater nativa del ejemplo. No hubo shaders fallidos, cargas cercanas pendientes ni desbordes del presupuesto de 512 partículas del tren. El pico observado en las capturas de Chiltern fue 1.940 MiB RSS; no es una medición del recorrido completo.

La regresión gráfica conserva la imagen original y los límites de 16/255 por canal y 2% de píxeles distintos. Pasó en GPU AMD (0,089%) y Vulkan por software (0,028%). El oráculo de instancias comprueba posición y color con una luz solar, una luz lunar apagada y dos luces activas; los materiales no dependen del orden interno de las luces de Bevy.

Cinco pantallas del menú pasaron el control de disposición a 800×600. Cuatro mezclas PCM de los Pullman originales, de casi doce segundos cada una, tuvieron señal, cero WAV ausentes y cero muestras recortadas. Son comprobaciones automáticas; la guía siguiente permite revisar la conducción, imagen y sonido a mano.

`check.sh` ejecuta formato, clippy, workspace, scripts, build y los oráculos congelados. Las regresiones `plan30_` comprueban integración entre energía, wipers, movimiento, sonido, temperatura, guardado y compatibilidad. Las pruebas del original que requieren assets se ejecutan aparte sobre el paquete instalado.

## [Plan 038: Agregar vibraciones al pasar por juntas e irregularidades](https://github.com/cavazquez/openrailsrs/issues/248)

Estado: implementado y verificado.

Respuesta espacial en juntas de 18,3 m e irregularidades acotadas por velocidad y suspensión de cada vehículo. El movimiento parte de la pose física; no acumula desplazamientos.

Código: `train_motion.rs`. Regresiones: `plan30_spatial_joints`.

- [x] Generar respuesta acotada ligada a posición, velocidad y vehículo.
- [x] Evitar vibraciones dependientes del FPS y respetar pausa y suspensión desactivada.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** En F10 → Imagen y cabina activá suspensión Suave y conducí en recta. Probá también Apagado y una parada. Debés ver pequeños movimientos sin deriva; detenido o con suspensión apagada no hay vibración de juntas.

## [Plan 040: Reproducir configuraciones particulares de faros por locomotora](https://github.com/cavazquez/openrailsrs/issues/250)

Estado: implementado y verificado.

Se conservan posiciones, colores, condiciones alternativas y secuencias States/Cycle originales. Los haces de Bevy usan el estado efectivo de alimentación del vehículo; los sprites blancos originales sin cono aportan un haz bajo utilizable.

Código: `train_lighting.rs; formats/typed/vehicle_lights.rs`. Regresiones: `plan30_original_alternative_conditions; plan30_beams_respond`.

- [x] Respetar ubicación, color y patrones de luces del autor.
- [x] Comprobar modos apagado, bajo y alto desde cabina y exterior con alimentación real.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** De noche, alterná faros apagados, bajos y altos con el control de F6. Mirá desde 1 y 2: los bajos iluminan cerca y los altos más lejos. Cortá batería/alimentación en F9 y comprobá que se apaguen. Un vehículo con Lights propios conserva sus puntos y colores.

## [Plan 041: Mostrar luces de cola según extremo activo de la formación](https://github.com/cavazquez/openrailsrs/issues/251)

Estado: implementado y verificado.

Las condiciones First/Last y orientación se evalúan sobre la sección actualmente acoplada, con el sentido y la cabina activa. Los coches intermedios no heredan el marcador de cola.

Código: `train_lighting.rs`. Regresiones: `plan30_tail_conditions`.

- [x] Seleccionar marcadores según composición, sentido y cabina activa.
- [x] Actualizar al invertir o separar vehículos sin encender luces intermedias como cola.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Desde 2 mirá ambos extremos, invertí detenido y separá una sección asegurada con F9. Las luces declaradas como extremo deben seguir el nuevo extremo; no se debe encender una cola en todos los coches. Requiere una formación que declare esos marcadores.

## [Plan 045: Mostrar arena en el contacto entre rueda y riel](https://github.com/cavazquez/openrailsrs/issues/255)

Estado: implementado y verificado.

La emisión toma la entrega física de arena, no sólo el interruptor. Usa SanderSpecialEffects del autor o puntos de contacto en los bogies motores; CPU y Hanabi comparten el presupuesto y la pausa.

Código: `train_effects.rs; train_effects_gpu.rs`. Regresiones: `plan30_sand_reads_delivered; effects_update_uses_live_wind`.

- [x] Emitir sólo con entrega física disponible y desde tomas correctas.
- [x] Respetar sentido, reserva agotada, pausa y presupuesto de partículas.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** En lluvia, activá arena mediante la tecla mostrada en F6 y observá de cerca un bogie motor. Debe aparecer un chorro corto marrón junto al riel. Desactivala o pausá: deja de nacer arena. Si el equipo sólo entrega en marcha, hay que avanzar; sin reserva no hay emisión.

## [Plan 046: Diferenciar vapor y condensación según temperatura ambiente](https://github.com/cavazquez/openrailsrs/issues/256)

Estado: implementado y verificado.

La temperatura compartida cambia opacidad de condensación del vapor. Caudal, consumo y reservas siguen perteneciendo al simulador, y el viento desplaza las partículas.

Código: `journey_environment.rs; train_effects.rs`. Regresiones: `plan30_sand_reads_delivered`.

- [x] Variar apariencia de condensación sin alterar consumo físico de vapor.
- [x] Usar temperatura compartida y conservar viento, pausa y origen flotante.
- [x] Añadir ensayos representativos de fuerzas/estados y una prueba de conducción; usar oráculos OR 1.6.1 para lo equivalente y pruebas propias para extensiones.

**Prueba manual:** Elegí un ejemplo de vapor y compará F10 → Hora y clima → Temperatura a −10 °C y 30 °C manteniendo los mandos. El vapor frío se ve más blanco y persistente; no cambia el caudal físico por este ajuste visual. Pausa congela las partículas.

## [Plan 051: Ampliar controles originales operables con el mouse](https://github.com/cavazquez/openrailsrs/issues/261)

Estado: implementado y verificado.

Se amplían los bindings nativos, incluidos ORTS_, arranque/parada, freno independiente, batería, alimentación, emergencia y auxiliares de vapor. Ratón y teclado llaman a las mismas operaciones físicas.

Código: `cab_mouse.rs`. Regresiones: `plan30_native_mouse_aliases`.

- [x] Añadir bindings originales faltantes y áreas de interacción claras.
- [x] Conservar pasos discretos, enclavamientos y equivalencia con teclado sin conflictos.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** En una cabina 3D compatible, pasá el cursor por los controles originales: aparece el nombre. Arrastrá freno/regulador y probá arranque, batería o disyuntor. Las posiciones discretas y los enclavamientos deben coincidir con el teclado; no se puede invertir un tren en marcha.

## [Plan 052: Incorporar velocidades e intermitencia del limpiaparabrisas](https://github.com/cavazquez/openrailsrs/issues/262)

Estado: implementado y verificado.

V recorre Apagado, Intermitente, Lento y Rápido. Barrido, animación y audio derivan del mismo reloj; la alimentación de cabina determina si el equipo se mueve. Las cadencias ampliadas son una función del visor.

Código: `sim/wiper.rs; windshield.rs; cab_cvf.rs; audio/native.rs`. Regresiones: `plan30_wiper_cadence_power; playable_native_wiper`.

- [x] Mantener estados compatibles por equipo con cadencias reproducibles.
- [x] Sincronizar animación, barrido y sonido con batería, pausa y guardado.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Con lluvia, pulsá V sucesivamente. Mirá el modo del HUD, la hoja y el vidrio, y escuchá el cambio de cadencia. Intermitente tiene una espera entre barridos. Cortá batería, pausá, guardá y continuá: no debe perder fase ni sonar con el equipo detenido.

## [Plan 056: Guardar asiento y cámara por modelo de tren](https://github.com/cavazquez/openrailsrs/issues/266)

Estado: implementado y verificado.

Asiento, desplazamiento y orientación se guardan con identidad relativa del CVF debajo de TRAINSET. Se migran claves antiguas; los límites rechazan poses inválidas y Restablecer devuelve los valores originales.

Código: `cab_profile.rs; camera.rs; player_settings.rs`. Regresiones: `plan30_model_preferences`.

- [x] Conservar ajustes por identidad estable del modelo.
- [x] Recuperar posición válida entre sesiones y ofrecer restablecer valores originales.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** En F10 → Imagen y cabina ajustá asiento y desplazamiento, guardá ajustes y orientá la vista en cabina. Volvé a abrir el mismo modelo: recupera sus preferencias. Otro modelo conserva las propias. Restablecer debe quitar el desplazamiento y la orientación personalizada.

## [Plan 057: Afinar exposición al entrar y salir de túneles](https://github.com/cavazquez/openrailsrs/issues/267)

Estado: implementado y verificado.

La exposición detecta techo y paredes próximos del escenario, excluyendo el propio tren. La transición exponencial usa límites configurables de 0–3 EV sin modificar el clima ni las luminarias.

Código: `effect_obstacles.rs; camera.rs`. Regresiones: `plan30_tunnel_enclosure; plan30_tunnel_exposure`.

- [x] Adaptar iluminación con transición gradual y límites configurables.
- [x] Mantener legibilidad de instrumentos sin borrar paisaje ni alterar físicamente el clima.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Entrá y salí de un túnel desde cabina. La luminosidad debe cambiar gradualmente, sin salto ni instrumentos borrados. Ajustá Exposición en túneles en F10 → Imagen y cabina; en 0 no hay adaptación adicional. Una marquesina abierta no debe comportarse como un túnel cerrado.

## [Plan 058: Separar movimiento de pupitre, carrocería y cabeza](https://github.com/cavazquez/openrailsrs/issues/268)

Estado: implementado y verificado.

Carrocería, cabeza y pupitre tienen respuestas independientes sobre la misma suspensión física. Cabeza y Pupitre se regulan por separado y sus transformaciones se reconstruyen desde una pose base.

Código: `train_motion.rs; cab_view.rs; camera.rs`. Regresiones: `plan30_spatial_joints; suspension`.

- [x] Aplicar respuestas independientes y ajustables sobre la misma pose física.
- [x] Mantener selección Suave/Apagado y evitar deriva acumulada o movimiento dependiente del FPS.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Conducí con suspensión Suave. En F10 → Imagen y cabina compará Cabeza 0/Pupitre 1 y luego Cabeza 1/Pupitre 0. La mirada y el tablero deben responder por separado. Apagado elimina ambos; al cambiar entre 1 y 2 no se acumula inclinación.

## [Plan 059: Regular iluminación de instrumentos por sector e intensidad](https://github.com/cavazquez/openrailsrs/issues/269)

Estado: implementado y verificado.

Tres intensidades por modelo —instrumentos, pupitre e interior— reutilizan las variantes DAY/NIGHT/CABLIGHT ya cargadas en las cabinas 3D compatibles. Se respetan texturas originales y alimentación real de cabina. Las cabinas 2D conservan su selección nocturna del panel completo.

Código: `cab_cvf.rs; cab_profile.rs`. Regresiones: `night; cablight; brightness`.

- [x] Controlar zonas compatibles y conservar texturas NIGHT/CABLIGHT originales.
- [x] Respetar alimentación de cabina y persistir intensidad sin recrear bancos innecesariamente.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** De noche encendé la luz interna y ajustá cada sector en F10 → Imagen y cabina. Sólo las texturas compatibles con ese sector deben variar. Guardá y reabrí el modelo. Al cortar alimentación se pierde CABLIGHT y se conserva la variante nocturna correspondiente.

## [Plan 060: Explicar causas de pérdida de tracción y variaciones de corriente](https://github.com/cavazquez/openrailsrs/issues/270)

Estado: implementado y verificado.

F8 → Locomotora explica neutro, puertas, combustible, fase del motor, protección térmica, mando, alimentación, frenos y patinaje. La fuerza/corriente normal y la demanda auxiliar se leen del vehículo de cabina.

Código: `sim/live_drive.rs; player_ui.rs`. Regresiones: `plan30_traction_diagnosis`.

- [x] Presentar causas físicas actuales y acciones de recuperación entendibles.
- [x] Distinguir corte, límite de potencia, patinaje, freno y protección con datos del mismo vehículo.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Abrí F8 → Locomotora mientras acelerás; debés ver la causa actual y una acción concreta cuando se corta la tracción. Probá neutro, motor detenido, batería y freno independiente. A mucha velocidad, la caída normal de esfuerzo por límite de potencia no debe presentarse como avería.

## [Plan 061: Mostrar fallos de sistemas mediante indicadores originales](https://github.com/cavazquez/openrailsrs/issues/271)

Estado: implementado y verificado.

Los indicadores declarados reciben el estado físico del motor, emergencia, temperaturas y disyuntor. Los estados diésel conservan la numeración original Stopped/Stopping/Starting/Running; no se dibujan lámparas adicionales en modelos sin recursos.

Código: `cab_cvf.rs; sim/live_drive.rs`. Regresiones: `plan30_native_status_displays`.

- [x] Vincular luces y medidores declarados a estados reales de avería.
- [x] Restablecer indicadores al recuperar el sistema sin inventar señales en cabinas sin recursos.
- [x] Añadir ensayos representativos de fuerzas/estados y una prueba de conducción; usar oráculos OR 1.6.1 para lo equivalente y pruebas propias para extensiones.

**Prueba manual:** En un tren con controles de estado originales, paralo y arrancalo desde B; observá sus indicadores. En el ejemplo térmico o de poco combustible mirá también F8. Recuperar el sistema debe restablecer su indicador. Una cabina sin esa lámpara no recibe una inventada.

## [Plan 078: Animar agua conservando contenido original](https://github.com/cavazquez/openrailsrs/issues/288)

Estado: implementado y verificado.

Las superficies de agua compatibles conservan materiales y límites del tile. Ondas y reflexión son pequeñas variaciones verticales analíticas sobre el reloj ferroviario.

Código: `water.rs`. Regresiones: `plan30_water_uses_rail_clock`.

- [x] Aplicar animación coherente a superficies de agua compatibles.
- [x] Conservar límites y texturas originales, pausa y origen flotante.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Desde 2 o cámara libre observá agua del escenario: debe moverse suavemente sin cambiar su textura ni inundar bordes. Pausá y comprobá que se congele; alejate y volvé a cargar el sector para comprobar que conserva su altura.

## [Plan 080: Representar fases lunares e iluminación nocturna](https://github.com/cavazquez/openrailsrs/issues/290)

Estado: implementado y verificado.

La posición y fase lunar se calculan para fecha y coordenadas de ruta, con paralaje local y aproximación orbital para presentación. El aporte luminoso tiene su propia luz y depende de fase, horizonte y nubes. Los materiales eligen la iluminación dominante por intensidad y usan sus sombras correspondientes; una luz lunar apagada no oscurece el terreno de día ni una luna elevada activa el brillo diurno de noche.

Código: `moon.rs; scenery/assets/shaders/moon.wgsl`. Regresiones: `lunar; moon`.

- [x] Calcular fase y posición desde fecha y ubicación de la ruta.
- [x] Separar aporte lunar de luminarias y preservar cielo despejado, estrellas y hora elegida.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Elegí despejado, noche y Fecha lunar en F10 → Hora y clima. Cambiá de fecha/hora y observá la luna cuando esté sobre el horizonte. En 2000-01-21 está cerca de llena; en 2000-01-06, nueva. La fecha elegida define la luna; el sol conserva la estación del escenario en modo manual. Con hora local, ambos usan la fecha real. Hora, faros y estrellas siguen siendo controles independientes.

## [Plan 083: Mostrar pronóstico del viaje en menú y partida](https://github.com/cavazquez/openrailsrs/issues/293)

Estado: implementado y verificado.

El menú Hora y clima y F8 → Clima presentan el pronóstico correspondiente: manual fijo, secuencia aleatoria reproducible o estimación horaria de Open-Meteo. Datos del lugar incluyen antigüedad y probabilidad de precipitación.

Código: `forecast.rs; environment.rs; player_ui/launcher.rs`. Regresiones: `forecast_preserves; plan30_provider_forecast`.

- [x] Representar condiciones previstas y su procedencia manual, aleatoria o del lugar.
- [x] Mostrar antigüedad e incertidumbre de datos sin cambiar la elección del jugador.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Compará los tres orígenes antes de Jugar y en F8 → Clima. Manual informa condiciones constantes; Aleatorio muestra próximos cambios; Lugar indica fuente, edad y probabilidad cuando hay conexión. Sin datos muestra el respaldo elegido y no altera tu selección.

## [Plan 084: Incorporar temperatura ambiente a sistemas del tren](https://github.com/cavazquez/openrailsrs/issues/294)

Estado: implementado y verificado.

Una temperatura ambiente común admite estimación por ubicación/estación/hora, dato meteorológico o valor manual. Alimenta condensación y transferencia térmica diésel compatible, y se conserva junto al clima guardado.

Código: `journey_environment.rs; weather_state.rs; sim/diesel_operation.rs`. Regresiones: `plan30_ambient_temperature; plan30_wiper_cadence_power`.

- [x] Unificar temperatura por ubicación, hora y selección de clima.
- [x] Exponerla a sistemas compatibles y conservar fase y valor al guardar.
- [x] Añadir ensayos representativos de fuerzas/estados y una prueba de conducción; usar oráculos OR 1.6.1 para lo equivalente y pruebas propias para extensiones.

**Prueba manual:** Compará temperatura automática y manual en F10 → Hora y clima; F8 → Clima muestra la efectiva. En un diésel compatible, mantené una carga durante varios minutos en frío y calor: cambia la refrigeración. Guardá y continuá: conserva valor y fase meteorológica.

## [Plan 088: Agregar ráfagas de viento sobre vegetación y partículas](https://github.com/cavazquez/openrailsrs/issues/298)

Estado: implementado y verificado.

Las ráfagas varían suavemente intensidad y dirección con semilla y reloj de simulación. Humo, lluvia/nieve y vegetación consumen el mismo viento efectivo; no hay retroalimentación que aumente el viento en cada cuadro.

Código: `journey_environment.rs; weather_state.rs`. Regresiones: `climate_uses_location; effects_update_uses_live_wind`.

- [x] Generar ráfagas suaves con semilla y reloj compartidos.
- [x] Aplicar respuesta consistente al humo, precipitación y vegetación preservando pausa.
- [x] Capturar cabina/exterior o vistas relevantes con condiciones reproducibles; comprobar pausa, cambios de cámara, origen flotante y coste del efecto.

**Prueba manual:** Con viento y lluvia o nieve, observá precipitación, vegetación y escape. Su dirección debe variar de forma coherente. Pausá: se congela. Repetir la misma semilla y fase reproduce las ráfagas; cambiar de cámara no inicia otra secuencia.

## [Plan 094: Diferenciar sonido de lluvia sobre vidrio y cubiertas](https://github.com/cavazquez/openrailsrs/issues/304)

Estado: implementado y verificado.

Lluvia exterior, parabrisas, techo metálico y cubierta del escenario tienen bancos acústicos distintos, con transición gradual e intensidad común. Son efectos meteorológicos del visor.

Código: `audio/weather.rs; audio/native.rs; native_audio.rs`. Regresiones: `plan30_rain_surface_banks`.

- [x] Seleccionar respuesta acústica según cámara y cubierta próxima.
- [x] Compartir intensidad efectiva y suavizar transiciones evitando fuentes duplicadas.
- [x] Comprobar disparadores y mezcla sin saturación, usando recursos disponibles del autor; hacer escucha manual en cabina y exterior.

**Prueba manual:** Con lluvia intensa y audio activado, alterná exterior, cabina y pasajeros; luego colocá la cámara bajo una marquesina. Debés escuchar diferencias y una transición suave, sin duplicar volumen. Bajar intensidad reduce el sonido; pausa detiene la reproducción.

## [Plan 095: Afinar transiciones de motor, turbo y carga con audio original](https://github.com/cavazquez/openrailsrs/issues/305)

Estado: implementado y verificado.

La mezcla utiliza RPM, carga y estado por motor sobre los SMS/WAV originales. El avance de cámara/volumen conserva los bucles y el historial de eventos; no reinicia archivos cada cuadro.

Código: `native_audio.rs; audio/native.rs`. Regresiones: `plan30_sms_mix_camera; native_`.

- [x] Usar estados por motor y recursos SMS/WAV disponibles.
- [x] Comprobar ralentí, carga, cambios y parada sin reinicios de bucles por cuadro.
- [x] Añadir ensayos representativos de fuerzas/estados y una prueba de conducción; usar oráculos OR 1.6.1 para lo equivalente y pruebas propias para extensiones.

**Prueba manual:** Con audio original disponible, escuchá ralentí, aceleración, carga, cierre de regulador, freno y parada del motor. En un diésel con varios motores, aislá uno en B. El sonido debe seguir el motor correspondiente sin tartamudeo; un turbo sólo suena si lo aporta su SMS.

## [Plan 096: Mejorar chirridos de ruedas según curva y vehículo](https://github.com/cavazquez/openrailsrs/issues/306)

Estado: implementado y verificado.

Se leen programas de curva referenciados por ttype.dat y se evalúa radio/velocidad por coche en su ubicación nativa. Se conservan los SMS opcionales sin hacer sonar curvas en recta ni tapar el programa normal.

Código: `native_audio.rs; audio/native.rs`. Regresiones: `curve; plan30_sms_mix_camera`.

- [x] Relacionar sonidos compatibles con radio, velocidad y ruedas del coche.
- [x] Evitar chirridos en recta y conservar continuidad entre cámaras.
- [x] Añadir ensayos representativos de fuerzas/estados y una prueba de conducción; usar oráculos OR 1.6.1 para lo equivalente y pruebas propias para extensiones.

**Prueba manual:** Con un paquete que declare sonido de curva, avanzá por curva cerrada y luego recta. El chirrido corresponde al coche que pasa por la curva y cesa en recta. Alterná 1/2 sin reiniciar el banco. Sin referencia original no se inventa un chirrido.

## [Plan 097: Relacionar golpes de enganches con fuerzas físicas](https://github.com/cavazquez/openrailsrs/issues/307)

Estado: implementado y verificado.

Los eventos originales Couple/Uncouple se disparan en cambios de composición; el golpe físico exige cambio de contacto, fuerza y velocidad relativa. Contacto estable y carga inicial no generan repeticiones ficticias.

Código: `audio/native.rs; native_audio.rs`. Regresiones: `plan30_coupler_contacts`.

- [x] Disparar golpes por impactos y cambios de holgura efectivos.
- [x] Evitar repetir sonidos con contacto estable y respetar material de audio disponible.
- [x] Comprobar disparadores y mezcla sin saturación, usando recursos disponibles del autor; hacer escucha manual en cabina y exterior.

**Prueba manual:** Desde exterior acelerá/frená con una formación de enganches dinámicos y SMS compatibles. Debe sonar al cambiar holgura con impacto, no continuamente al mantener contacto. Separar y unir una sección en F9 activa el evento del enganche afectado, sin multiplicarlo por todos los coches.

## [Plan 098: Mejorar sensación acústica de trenes que pasan cerca](https://github.com/cavazquez/openrailsrs/issues/308)

Estado: implementado y verificado.

Las fuentes siguen cada coche y su velocidad física, incluida la sección estacionada. El Doppler usa velocidad relativa del servicio y del oyente, y excluye saltos de cámara como fuente de velocidad.

Código: `native_audio.rs; train_effects.rs`. Regresiones: `placement; radial; vehicle_world_velocity`.

- [x] Conservar trayectoria espacial por vehículo durante aproximación y alejamiento.
- [x] Ajustar mezcla y Doppler sin interpretar saltos de cámara como velocidad.
- [x] Comprobar disparadores y mezcla sin saturación, usando recursos disponibles del autor; hacer escucha manual en cabina y exterior.

**Prueba manual:** En Chiltern con tráfico activo, escuchá un servicio que se aproxima, pasa y se aleja desde cámara exterior. Debe desplazarse de lado y atenuarse con la distancia. Saltar entre cámaras o vehículos no debe crear un silbido Doppler artificial.

## [Plan 103: Ofrecer volúmenes separados por grupo de sonido](https://github.com/cavazquez/openrailsrs/issues/313)

Estado: implementado y verificado.

F10 → Sonido ofrece Motor, Rodadura, Cabina y controles, Ambiente y Anuncios, además del volumen general. Clasificación por recursos/control/eventos y grupo explícito para los bancos de curva.

Código: `audio/mix.rs; player_settings.rs; player_ui.rs`. Regresiones: `plan30_difficulty_and_independent_audio; plan30_sms_mix_camera`.

- [x] Controlar motor, rodadura, cabina, ambiente y anuncios por separado.
- [x] Persistir preferencias y aplicar cambios sin recargar los bancos.
- [x] Comprobar disparadores y mezcla sin saturación, usando recursos disponibles del autor; hacer escucha manual en cabina y exterior.

**Prueba manual:** Poné cada grupo en 0 y luego restauralo: sólo se silencia su categoría. Cambiar Motor no silencia lluvia ni limpiaparabrisas. Guardá Ajustes y reiniciá el visor; conserva los cinco niveles. No se recarga el contenido al mover un volumen.

## [Plan 105: Suavizar sonido al cambiar de cámara o coche](https://github.com/cavazquez/openrailsrs/issues/315)

Estado: implementado y verificado.

Mezcla de audibilidad interior/exterior y distancia interpolada durante 0,18 s; todos los programas mantienen sus disparadores y bucles incluso inaudibles. Cambiar el coche de escucha utiliza su cabina y paso acústico propios.

Código: `audio/native.rs; native_audio.rs`. Regresiones: `plan30_sms_mix_camera`.

- [x] Interpolar mezcla interior/exterior según ubicación real del jugador.
- [x] Mantener bucles y estados de disparadores evitando cortes y duplicación.
- [x] Comprobar disparadores y mezcla sin saturación, usando recursos disponibles del autor; hacer escucha manual en cabina y exterior.

**Prueba manual:** Alterná repetidamente 1/2 y cambiá de coche de pasajeros. Debe haber una transición breve y continua, sin comenzar otra vez el audio del motor ni doblar bucles. En F8 → Diagnóstico la cantidad de voces debe permanecer estable.

## [Plan 106: Mostrar sonidos activos y causas en un diagnóstico](https://github.com/cavazquez/openrailsrs/issues/316)

Estado: implementado y verificado.

F8 → Diagnóstico identifica tren/coche, programa SMS, stream, archivo, disparador, grupo, ganancia y pausa; muestra avisos y coste de mezcla. Los nombres de recursos se muestran sin prefijos de carpetas personales.

Código: `audio/native.rs; audio/mix.rs; player_ui.rs`. Regresiones: `plan30_sms_mix_camera; resource_name`.

- [x] Identificar programa, recurso, disparador y ganancia de cada voz.
- [x] Presentar recursos ausentes y coste sin exponer rutas personales en reportes públicos.
- [x] Comprobar disparadores y mezcla sin saturación, usando recursos disponibles del autor; hacer escucha manual en cabina y exterior.

**Prueba manual:** Activá sonido y abrí F8 → Diagnóstico. Debés ver los archivos activos y el último disparador, con ganancia/grupo. Pausá y cambiá volumen/cámara: cambia el estado, no los recursos. Si falta un WAV aparece su nombre, sin ruta personal en el reporte de audio.

## [Plan 149: Agregar perfiles de dificultad con ayudas identificadas](https://github.com/cavazquez/openrailsrs/issues/359)

Estado: implementado y verificado.

Guiado, Práctica, Horario normal y Actividad estricta agrupan las tres ayudas existentes. Se puede personalizar; ajustes de la partida guardada se restauran sin cambiar referencias físicas.

Código: `player_settings.rs; saved_game.rs; player_ui.rs`. Regresiones: `plan30_difficulty_and_independent_audio`.

- [x] Agrupar ajustes actuales en perfiles entendibles y editables.
- [x] Mostrar ayudas activas y conservar configuración al guardar sin alterar oráculos.
- [x] Documentar una prueba manual desde el menú y un caso de aceptación reproducible; comprobar persistencia y errores relevantes para este alcance.

**Prueba manual:** En F10 → Juego elegí cada perfil y leé las ayudas activas. Práctica hace embarque breve; Horario normal conserva espera; Estricta termina ante parada omitida. Cambiá una ayuda y debe figurar Personalizado. Guardá partida y retomá: conserva esas ayudas.

## [Plan 159: Mostrar nivel de validación por formación y sistema](https://github.com/cavazquez/openrailsrs/issues/369)

Estado: implementado y verificado.

Vista y diagnóstico separan recursos, circulación y sistemas. Once perfiles de freno originales se identifican por SHA-256 del ENG/WAG efectivo y todos sus Includes, con referencia OR y evidencia; ningún recurso completo certifica paridad general.

Código: `content_validation.rs; train/consist_audit.rs; oracles/content-validation.json`. Regresiones: `evidence_requires_exact; plan30_content_fingerprint; test_content_evidence_hashes`.

- [x] Separar recursos completos, circulación probada y sistemas contrastados.
- [x] Mostrar evidencia y versión sin certificar trenes sólo porque existan sus archivos.
- [x] Guardar configuración, versiones, hashes y resultados reproducibles; separar alcance verificado de sistemas que continúan pendientes.

**Prueba manual:** En Nueva partida → Tren desplazate hasta la vista del tren y abrí Recursos/diagnóstico. Mirá los tres niveles separados. Los vehículos con captura coincidente muestran sistema, OR 1.6.1 y evidencia; otro archivo o un Include modificado pierde esa coincidencia. Circulación sin ensayo registrado se muestra explícitamente.

## [Plan 168: Agregar historial de partidas con búsqueda y filtros](https://github.com/cavazquez/openrailsrs/issues/378)

Estado: implementado y verificado.

Continuar reúne slots e historial privado, con ruta/tren/fecha/progreso, buscador y cuatro filtros. La comprobación en segundo plano usa los validadores de jugador, tráfico y concesiones compartidas; no ejecuta scripts al listar.

Código: `save_history.rs; saved_game.rs; player_ui/launcher.rs`. Regresiones: `plan30_history_filters_metadata`.

- [x] Listar partidas por ruta, tren, fecha y progreso con búsqueda directa.
- [x] Abrir, continuar y distinguir partidas incompatibles usando la misma validación de carga.
- [x] Documentar una prueba manual desde el menú y un caso de aceptación reproducible; comprobar persistencia y errores relevantes para este alcance.

**Prueba manual:** Guardá partidas de dos rutas y varios avances; volvé a Continuar. Buscá ruta, tren o año y alterná Todos/Compatibles/Incompatibles/Con avance. Cargar usa el mismo proceso normal. Si falta contenido, la entrada explica incompatibilidad y no promete que se pueda retomar.

## [Plan 194: Actualizar ROADMAP con estado real y pendientes](https://github.com/cavazquez/openrailsrs/issues/404)

Estado: implementado y verificado.

ROADMAP se reescribe por fases de producto con código, aceptación verificable y límites actuales. Se retiran estados obsoletos, incluido audio pendiente, y se enlazan los asuntos abiertos por tema.

Código: `ROADMAP.md; docs/PLAN_30.md`. Regresiones: `check.sh; documentación`.

- [x] Contrastar cada fase con código, pruebas y límites actuales.
- [x] Retirar estados obsoletos y enlazar evidencia e issues activos sin prometer paridad global.
- [x] Revisar enlaces, estados y correspondencia con código/evidencia; indicar fecha del estado y guía de lectura.

**Prueba manual:** Abrí ROADMAP y seguí una fase hasta su guía/evidencia. El audio debe figurar implementado; las limitaciones de SIGSCR, scripts, actividades y sistemas deben estar expresas y enlazadas a asuntos todavía abiertos. La fecha y el alcance de las comparaciones son visibles.

## Alcance de los resultados

Los bucles y estados SMS son originales; los fundidos entre cámaras, las cuatro superficies acústicas de lluvia, ráfagas, vibración de juntas, luna y los modos ampliados de limpiaparabrisas son funciones de presentación de openrailsrs. Clasificar sonido por nombre/evento/control admite los recursos disponibles y no convierte un archivo ausente en un ensayo aprobado.

La temperatura estimada no es una observación meteorológica. El dato del lugar procede de [Open-Meteo](https://open-meteo.com/en/docs); el modo manual sigue disponible. La luna usa una aproximación de los [elementos orbitales de Paul Schlyter](https://stjarnhimlen.se/comp/ppcomp.html), contrastada en pruebas con las [fases publicadas por NASA para 2000](https://eclipse.gsfc.nasa.gov/phase/phases0001.html). Es una efeméride para el cielo del juego.

El registro de sistemas se regenera, con el TRAINSET original ya instalado, mediante `python3 scripts/build_content_validation.py --trainset /ruta/TRAINS/TRAINSET --check`. Sus hashes incluyen dependencias; las capturas y la procedencia permanecen intactas. Un freno contrastado no certifica todo el tren, sus scripts o cualquier combinación de ruta y actividad.

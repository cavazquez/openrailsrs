# Desarrollo de openrailsrs

Estado revisado el **11 de octubre de 2026**. El producto es un simulador ferroviario jugable en Rust y Bevy, con un núcleo que también se ejecuta sin gráficos. La referencia de compatibilidad sigue fijada en **Open Rails 1.6.1**. Cada comprobación tiene su propio contenido, condiciones y tolerancia; terminar una fase no certifica todos los sistemas de Open Rails.

## Cómo leer este estado

Las fases describen lo que ya se puede usar, dónde se comprueba y qué límites conserva. [El lote de treinta mejoras](docs/PLAN_30.md) contiene los issues seleccionados, criterios y pruebas manuales. El [plan abierto en GitHub](https://github.com/cavazquez/openrailsrs/issues?q=is%3Aissue%20is%3Aopen%20label%3Aplan-200) conserva los demás objetivos y sus etiquetas de tema/dificultad.

## 1. Elegir contenido e iniciar una partida

Implementado: menú de cuatro entradas, listas con buscador, previews de ruta/formación, contenido de distintos paquetes y recursos faltantes seleccionables con diagnóstico. La biblioteca instala versiones originales del autor en datos del usuario y conserva copias anteriores fuera de Git. Los niveles de recursos, circulación y subsistemas tienen evidencia independiente.

Código: `player_ui/`, `player_launch.rs`, `content_validation.rs`, `openrailsrs-content` y `consist_audit.rs`. Verificación: [contenido nativo](docs/NATIVE_CONTENT_UPDATE.md), [biblioteca y descargas](docs/OFFICIAL_CONTENT.md), pruebas de launcher/auditoría y [diez rutas de comunidad](docs/fixtures/compatibility/community-routes-2026-10-11/README.md).

Límite: los trenes pueden elegirse en otros mapas, pero trocha, suministro y recursos se comprueban aparte. El catálogo no es una clasificación universal de popularidad ni garantiza que cualquier actividad termine. Continúan abiertos [armado visual de formaciones #365](https://github.com/cavazquez/openrailsrs/issues/365) y [estimación previa de disco/RAM/VRAM #370](https://github.com/cavazquez/openrailsrs/issues/370).

## 2. Conducir y completar servicios

Implementado: cabinas 2D/3D, ratón sobre controles originales, conducción exterior, estaciones, pasajeros, horario, tráfico, libreta, mapa, operaciones F9 y ayudas editables de dificultad. Guardado/restauración conserva física y tráfico; Continuar permite buscar y filtrar historial.

Código: `openrailsrs-sim/live_drive.rs`, `operations.rs`, `live_traffic.rs`, `saved_game.rs` y `save_history.rs`. Verificación: [pruebas manuales del jugador](docs/PLAYER_MANUAL_TESTS.md), regresiones de servicios, inversiones y snapshots; [las treinta mejoras](docs/PLAN_30.md) documentan los nuevos controles y el historial.

Límite: se admiten operaciones concretas de horarios e inversiones; no todo evento ACT, pool, división entre formaciones independientes o maniobra general de OR. Pendientes: [rodear/reacoplar y terminar MidEast #211](https://github.com/cavazquez/openrailsrs/issues/211), [eventos originales de carga/descarga #212](https://github.com/cavazquez/openrailsrs/issues/212) y [retroceder una práctica #360](https://github.com/cavazquez/openrailsrs/issues/360).

## 3. Física, energía y seguridad

Implementado: tracción y resistencias por vehículo, enganches opcionales, frenos de aire/EP/vacío en los perfiles contrastados, diésel con RPM/reservas/refrigeración, vapor con subsistemas acotados y suministro eléctrico dependiente de captador/tensión. Las cabinas y diagnósticos leen los estados físicos. C# sigue siendo una opción de compatibilidad separada del funcionamiento Rust habitual.

Código: `openrailsrs-sim`, `openrailsrs-train` y host opcional `tcs-host`. Verificación: [alcance de tracción](docs/TRACTION_SUPPORT.md), [capturas y tolerancias OR](docs/OR_PARITY.md), suites de frenos/alimentación, vapor/diésel y cuatro ensayos cortos, Pullman completo y Class 47.

El servicio Pullman contrastado registra RMS 0,2081 m/s, pico 1,0921 m/s y odómetro máximo 39,61 m; Class 47, 0,2382 m/s, 1,9415 m/s y 25,79 m. Cumplen sus presupuestos originales de 0,75 m/s, 2 m/s y 45 m. Son ensayos específicos, no una tolerancia global para todo contenido.

Límite: termodinámica de vapor, equipos auxiliares y scripts personalizados aún requieren ampliación; no hay cremallera. La temperatura ambiente aplicada a refrigeración y condensación no implementa hielo/deshielo. Pendientes: [traza completa LIRR #220](https://github.com/cavazquez/openrailsrs/issues/220), [fuerzas verticales por irregularidades #325](https://github.com/cavazquez/openrailsrs/issues/325) y [auxiliares de vapor #330](https://github.com/cavazquez/openrailsrs/issues/330).

## 4. Vías, señales y operaciones originales

Implementado: TDB/PAT/ACT, geometría curva y dirección, itinerarios nativos, SIGSCR contrastado con funciones originales, ocupación dirigida y reservas del próximo bloque en recorridos admitidos. Los errores de contenido y funciones no soportadas tienen diagnóstico o aspecto restrictivo.

Código: `openrailsrs-formats`, `openrailsrs-route`, `native_signals.rs` y despachador de simulación. Verificación: oráculos originales de parser/señales y [alcance de señalización y contenido](docs/SIGNALS_AND_CONTENT_SCOPE.md).

Límite: no está certificado el despachador general de OR, todos los enlaces, memorias, pools ni coordinación entre actividades arbitrarias. [Objetivos de señalización y operaciones abiertos](https://github.com/cavazquez/openrailsrs/issues?q=is%3Aissue%20is%3Aopen%20label%3Aplan-200%20label%3Atema%3Asenalizacion).

## 5. Escenario, cabina y entorno

Implementado: streaming de terreno/objetos/trenes originales, LOD, origen flotante, materiales y transparencia, agua animada, faros/marcadores nativos, instrumentos nocturnos y exposición gradual en túneles. Suspensión, cabeza y pupitre tienen respuesta independiente. Hay lluvia/nieve/humo con Hanabi y alternativas CPU/híbrida, vidrio con barrido, niebla y tormentas. Viento y temperatura son compartidos; luna, hora/clima elegidos, clima aleatorio y datos actuales del lugar permanecen independientes.

Código: `openrailsrs-viewer3d`, `openrailsrs-bevy-scenery`, `train_effects*`, `weather_state.rs`, `moon.rs` y shaders. Verificación: [visor y límites](docs/VIEWER3D.md), [goldens y referencias visuales](docs/fixtures/visual/player_goldens/README.md), capturas de estaciones, pruebas de streaming y [mejoras del lote](docs/PLAN_30.md).

Límite: un shader compatible no certifica identidad de píxeles con OR. Modelos sin recursos declarados no reciben equipos ficticios. CPU significa renderizado por software donde el adaptador lo permite; simulación y streaming no se transfieren íntegramente a GPU. Pendientes: [presupuesto conjunto de efectos #390](https://github.com/cavazquez/openrailsrs/issues/390) y [pruebas prolongadas de memoria #403](https://github.com/cavazquez/openrailsrs/issues/403).

## 6. Sonido

Implementado: programas SMS/WAV originales por vehículo, eventos nativos, estado/RPM/carga, fuentes espaciales y Doppler, curvas compatibles y enganches físicos. Cambio de cámara conserva bucles con transición de mezcla. Cinco grupos de volumen, lluvia sobre distintas superficies y diagnóstico de recursos/voces/coste están disponibles.

Código: `openrailsrs-audio`, `native_audio.rs` y ajustes/HUD. Verificación: [audio nativo y oráculo](docs/NATIVE_AUDIO.md), regresiones SMS, mezcla PCM sin saturación y [pruebas manuales de las ampliaciones](docs/PLAN_30.md).

Límite: SMS personalizados fuera del vocabulario admitido y WAV ausentes se informan. Los efectos meteorológicos y fundidos de cámara son presentación del visor; no se afirma identidad acústica completa con OpenAL. [Objetivos de audio abiertos](https://github.com/cavazquez/openrailsrs/issues?q=is%3Aissue%20is%3Aopen%20label%3Aplan-200%20label%3Atema%3Aaudio).

## 7. Distribución y mantenimiento

Implementado: CLI y visor separados, paquete portátil, receta Snap y alpha Linux en edge. Los ejemplos/shaders del paquete permiten ejecutar fuera del checkout; las descargas permanecen en datos persistentes del usuario. `check.sh` verifica formato, clippy, pruebas Rust/Python, generación web, compilación y oráculos sin reemplazar las referencias originales.

Verificación: [distribución](docs/DISTRIBUTION.md), [QA](docs/PLAYER_POLISH_QA.md) y [índice de documentación](docs/README.md). Pendientes: [reporte de diagnóstico desde el juego #400](https://github.com/cavazquez/openrailsrs/issues/400), [joysticks #406](https://github.com/cavazquez/openrailsrs/issues/406) y los [objetivos de distribución](https://github.com/cavazquez/openrailsrs/issues?q=is%3Aissue%20is%3Aopen%20label%3Aplan-200%20label%3Atema%3Adistribucion). Un push de código no publica automáticamente un binario nuevo en Snap.

## Próximas decisiones

Se prioriza una partida consistente y verificable: ampliar contenido/operaciones que hoy no terminan, profundizar sistemas concretos y sostener la continuidad visual/sonora con presupuestos medidos. Los issues conservan su dificultad estimada y tema para seleccionar lotes; sólo se cierran con criterios y evidencia terminados. La actualización de Bevy no forma parte de este lote.

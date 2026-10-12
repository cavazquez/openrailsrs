# Aceptación de la tercera entrega

12 de octubre de 2026. Referencias conservadas: Open Rails 1.6.1, Bevy 0.19.1 y Hanabi 0.19.0. La implementación abarca los 30 issues de [PLAN_30_3.md](PLAN_30_3.md). Se completaron y cerraron 26; cuatro de audio siguen abiertos hasta completar su escucha manual. La [guía manual](PLAYER_30_3_MANUAL.md) indica cómo probar cada uno.

## Verificación integrada

`check.sh` pasó formato, Clippy sin advertencias, pruebas Rust/Python, construcción, ensayos nativos del contenido instalado, oráculo de escenario y servicio de estaciones. Se registraron 1896 ejecuciones de pruebas Rust, incluidas las nativas habilitadas; 718 corresponden al visor. Las pruebas que necesitan otras instalaciones originales siguen ignoradas: no se cuentan como cobertura del contenido ausente.

Los seis oráculos físicos conservan sus tolerancias. Pullman completo: RMS de velocidad 0,2081 m/s, pico 1,0921 m/s y diferencia máxima de odómetro 39,61 m. Class 47: 0,2382 m/s, 1,9415 m/s y 25,79 m. Sus límites siguen siendo 0,75 m/s, 2 m/s y 45 m. Los cuatro ensayos cortos también pasaron. No son una certificación global de todas las locomotoras.

Las regresiones cubren conexión perdida con espera finita y guardado; obra temporal hasta despejar la cola; cierre con señal SIGSCR restrictiva y desvío por apartadero existente; fin de cierre sin reactivación por otro tren y comparación de pasos largos/cortos; decisión de incidente; nueve pasos físicos del tutorial de vapor; retroceso con todo el estado del tráfico; campañas con estado material entre jornadas; CON propio con orden/inversión sin modificar fuentes; servicios con horarios, deshacer y persistencia; recursos compartidos confinados; traslado de partida a otra carpeta con contenido idéntico y rechazo de modificación; reproducción de comandos con diagnóstico del primer resultado diferente; desconexión y devolución del mando al teclado.

El ejemplo `rust_vehicle_extension` se ejecutó: reloj final 3 s y 2 s acumulados sin suministro de baja tensión, incluyendo guardado/restauración. No necesitó .NET.

## Capturas y presentación

Nueve pantallas a 800×600 pasaron controles de límites y pipelines: inicio, ruta, tren, clima, controles, herramientas, campañas, formación y servicio. Se inspeccionaron las imágenes de campaña y formación, además de las seis vistas nativas de Northolt Park, South Ruislip y Gerrards Cross desde cabina/exterior. Los editores usan desplazamiento para listas largas.

El laboratorio propio comprobó ventanas a las 21:00 y 03:00: dos partes controladas, dos encendidas al comienzo y una a las 03:00. La cabina y el exterior conservaron materiales y semilla. Una captura detectó que los modelos con un único LOD estaban fuera del registro; se corrigió el acceso a su malla principal. Las luces de coches respondieron a suministro real y se limitaron por distancia y presupuesto.

El tráfico se verificó sobre una vía paralela sintética: otro servicio efectivamente salió, recorrió 292,29 m y usó dos faros propios bajo niebla volumétrica de visibilidad 120 m. Se fijó la calidad para mostrar sus haces; la calidad adaptativa puede reservar un presupuesto reducido al tren del jugador. Los haces del tráfico nunca sustituyen su estado físico o reservas.

La aguanieve se capturó con lluvia 0,25, nieve 0,25 y temperatura 0,5 °C en CPU, GPU y Mixto. La prueba descubrió que la atmósfera estaba mezclada pero los sprites aún usaban un único tipo: ahora un campo persistente asigna ambos subtipos por semilla, sin duplicar el presupuesto. CPU utilizó 1024 partículas; GPU, 1024; Mixto, 768 GPU + 256 CPU. En GPU y Mixto quedó una sola inicialización de semillas, sin actualización de malla por cuadro. Pasaron los pipelines reales de Hanabi y del respaldo CPU, además del ensayo de proporción y extremos de lluvia/nieve. Los ensayos de reloj, cámara y origen flotante siguen en la suite.

## Rendimiento comparable

Referencia ejecutable `ac9ec45` frente al visor del commit `50f87bc`, antes del ajuste de obras vencidas (este tramo no contiene obras), mismo equipo, GPU/driver, 1280×720, recorrido, formación Pullman, cámara, semilla 42, lluvia y calidad. Cada ejecución condujo 100,1 m. P50/P95: 25/25 ms en ambas; P99: 27 → 25 ms. Ningún cuadro de conducción superó 100 ms. RAM del informe: 1277,18 → 1283,89 MiB; VRAM máxima del proceso: 1381,48 → 1124,87 MiB. Todo quedó dentro del presupuesto fijado. Inicio separado: cuadro más largo 1565,09 → 1455,64 ms. Estos resultados describen este ensayo y no predicen el FPS de otro equipo.

[Contrato, hardware, hashes y métricas](fixtures/third30-2026-10-12/performance.json). El comparador también rechaza condiciones diferentes, menos de 300 cuadros y valores no finitos; ausencia de VRAM sigue siendo desconocida.

## Sonido, seguridad y límites

Se renderizaron 12 s de SMS/WAV originales de Birmingham Pullman, RS_Football Special y 1960CentralWR8Car, en cabina y exterior: seis casos con señal, cero WAV faltantes y cero muestras saturadas. Cuatro muestras adicionales ejercitan apertura gradual, reverberación y bocina. Se verificaron ausencia de voces duplicadas, emisor independiente de fuga, cese al reparar y persistencia de eventos de anuncios. No se declara realizada una escucha que el usuario todavía no pudo hacer: #237, #309, #310 y #312 permanecen abiertos.

La revisión de seguridad encontró dos hallazgos confirmados: lectura no acotada de escenarios de campaña y rutas personales en campos de salida/validación de respaldos. Se corrigieron con confinamiento, archivo regular y límites de lectura, y normalización de esos campos. Sus regresiones pasaron.

Las ventanas, suspensión y emisores necesitan datos compatibles; gálibo desconocido no equivale a compatibilidad. Los servicios propios cubren un itinerario existente y un sentido. El formato portátil exige instalaciones registradas y contenido idéntico; una referencia fuera de ellas se rechaza. La reproducción registrada cubre simulación Rust y no audio/GPU, abastecimiento activo o C#. El mando se verificó con eventos Bevy y arranque del backend; no había un joystick físico disponible. Snap incorpora dependencias y permiso, pero esta entrega no publica un nuevo paquete ni da por probada la conexión física.

[Resumen de capturas y aceptación](fixtures/third30-2026-10-12/acceptance.json). Los PNG, WAV, registros y trazas completos se guardan localmente en `tmp/third30-20261012`; no se distribuye contenido original descargado. Los seis archivos de trazas que ya tenían cambios antes de esta tarea se conservaron byte por byte y se excluyen del commit.

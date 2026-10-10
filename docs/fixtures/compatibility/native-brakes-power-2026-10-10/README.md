# Frenos, alimentación, audio y recorrido — 10 de octubre de 2026

## Comparación con Open Rails

El ayudante ejecuta las DLL originales verificadas de Open Rails 1.6.1, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`. Dos capturas independientes producen el mismo archivo SHA-256 `723251e0418d42acb9d0c965a786afc682bc0d289c28395463406150c04e61e8`.

Rust pasa 320 puntos de vacío, 384 de EP antiguo y 303 estados de alimentación. Las tolerancias son 0,0005 PSI para vacío, 0,001 PSI para EP y 5 N para fuerza de zapata; los estados eléctricos deben coincidir exactamente. Los oráculos anteriores del servicio Pullman y la Class 47 mantienen sus referencias y tolerancias.

La prueba de integración de frenos recorre las siete formaciones numéricas: aplicación, liberación y persistencia por coche. En vacío también comprueba aislamiento por llave y pérdida de vacío por manguera abierta. Los fixtures contienen parámetros de freno/alimentación, sin los modelos originales; los motores de vapor se leen como campos comunes de freno en esta prueba, sin fingir que son una inicialización térmica completa.

[Implementación, formaciones y límites](../../../NATIVE_BRAKES_POWER.md).

## Audio bajo confinamiento estricto

El defecto original era reproducible aun con audio-playback conectado: ALSA no encontraba su configuración ni el puente PulseAudio. La corrección agrega esas dependencias, una configuración de reproducción y la resolución del socket de sesión, conservando XDG_RUNTIME_DIR y una elección explícita de PULSE_SERVER.

Se ejecutó el mezclador nativo del Pullman bajo el perfil estricto de la revisión 1 instalada, con un payload corregido y bibliotecas de Ubuntu 24.04/core24. Cabina y exterior abren el dispositivo real. Ambos decodifican 16 programas, 58 streams y 69 muestras, sin avisos. Los WAV de 11,982 s contienen señal y cero muestras saturadas: RMS 0,1014 en cabina y 0,1805 en exterior. Sus picos son −3,90 y −1,00 dBFS. El limpiaparabrisas utiliza el respaldo documentado si el stock carece de su evento.

El dispositivo se abrió con volumen cero: no es una escucha humana. No se sustituyó el Snap del usuario. **La revisión 2 publicada en edge todavía no contiene esta corrección**; necesita otra construcción y publicación. Pasan las 14 pruebas de empaquetado con sockets locales.

## Lluvia y nieve intensas en Paddington

Dos pruebas con Birmingham Pullman v4, ocho coches originales, RX 7600/RADV/Vulkan, resolución 1280×720, radio 2 km, niebla volumétrica de 64 pasos y 8192 partículas Hanabi GPU. Se avanzan 150 m por el itinerario de 436 m a tiempo ×4 y se pausa para estabilizar recursos. Los contadores de partida excluyen la carga inicial; los diagnósticos generales incluyen el asentamiento pausado. Weston/Fifo condicionan la cadencia.

Lluvia: P50/P95/P99 de partida **25/25/28 ms**. Nieve: **25/34/41 ms**. Ninguna tuvo cuadros de partida mayores a 100 ms. La carga inicial conserva picos de **1,60/1,63 s**. El pico RSS externo es **2675,6/2675,5 MiB** y la VRAM máxima del proceso **1927,9/1933,7 MiB**; no se suman como si fueran la misma memoria.

Shaders y subidas de recursos terminan sin errores. Las ocho posiciones de coche pasan la validación; el error máximo de espaciado es 0,018 m. Las partículas GPU no generan actualizaciones de malla CPU. Son ejecuciones cortas, una por clima, sin una comparación antes/después ni una garantía de FPS para otros equipos.

## Viaje completo con clima cambiante

El servicio nativo de Chiltern v4 recorre **62,7 km**, Banbury General → Bicester North → Princes Risborough → High Wycombe, en invierno. Termina las cuatro paradas, sin omisiones, con error máximo de parada **3,49 m** y velocidad de llegada menor a **0,1 m/s**. Conserva 381 observaciones acotadas, cinco condiciones de clima y 66 muestras de frenado húmedo con el tren en movimiento. Shaders, señales, terreno y subidas pendientes pasan la validación al terminar.

La prueba usa radio 450 m, GPU real y tiempo **×16**. P50/P95/P99 de partida: **25/45/77 ms**; hubo **19** cuadros mayores a 100 ms y un máximo de **924 ms** durante la partida. El pico RSS externo fue **2809,2 MiB** y la VRAM del proceso **1660,9 MiB**. La carga inicial tuvo un pico de **1,68 s**. El viaje acelerado sigue mostrando tirones de streaming; estas medidas no certifican fluidez a tiempo normal ni dentro del Snap.

Las mediciones de render y audio usaron el payload anterior a la corrección final que evita retirar el modelo mecánico de vapor al cortar su batería; sus hashes se conservan por separado. Esa corrección sólo cambia las condiciones del material a vapor y se valida con la prueba de operación y el check final.

## Reproducción

Los comandos de los ensayos usan el contenido original instalado fuera de Git y carpetas de datos de jugador independientes. [Guía de rendimiento](../../../WEATHER_RENDERER_QA.md) y [pruebas manuales, sección 53](../../../PLAYER_MANUAL_TESTS.md#53-frenos-y-alimentación-rust-por-vehículo).

[verification.json](verification.json) conserva condiciones, huellas y medidas sin rutas personales. Los WAV, descargas y reportes completos con rutas locales quedan fuera de Git.

## Verificación final

`check.sh` terminó sin errores después de la corrección de vapor: formato, Clippy sin advertencias, tests del workspace, suites Python, build, integridad de los oráculos fijados, comparaciones del servicio Pullman y la Class 47 y servicio completo. La prueba de vapor mantiene movimiento y estado de caldera con batería cortada, apaga faros/cabina/limpiaparabrisas y reproduce el estado al guardar y reanudar. La Class 47 conserva auxiliares/ETS con el contactor abierto y pierde la salida al detener el motor.

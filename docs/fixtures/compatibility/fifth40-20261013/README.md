# Aceptación del quinto lote de cuarenta issues

La referencia de compatibilidad es Open Rails 1.6.1; Bevy permanece en 0.19.1.
Los recursos descargados de los autores permanecen fuera de Git y del Snap.
Los informes registran identidad de binarios, casos, métricas y límites, sin
copiar las rutas ni los modelos originales al repositorio.

- [Tren y escenario](../../../validation/FIFTH40_VISUAL_ACCEPTANCE.json): uniones,
  articulación, puertas, espejos, materiales, pasos a nivel, tránsito y nieve.
- [Operaciones y capturas](graphics-and-operations.json): casos de juego y
  reconstrucción de modelos al cambiar de conductor o recoger una formación.
- [Recorrido original completo](native-activity-complete.json): cuatro paradas,
  veinte trenes AI, sin fallos de servicio ni errores SIGSCR. Los actores que
  siguen aproximándose al terminar el jugador no se declaran terminados.
- [Restauración de actividad original](native-activity-resume.json): veinte AI,
  mismo estado físico y aspectos SIGSCR, rechazo atómico de un PAT corrupto.
- [Contenido y Snap](../content-runtime-20261013/receipt.json): almacenamiento,
  instalación desde origen, auditoría y partida con Blue Pullman.
- [Check de aceptación](check.json): formato, Clippy, tests del workspace,
  Python, oráculos fijados y servicio de estaciones. La versión del recibo
  identifica las correcciones cubiertas por cada ejecución.
- [Diagnóstico prolongado](endurance-diagnostic.json): dos horas completadas,
  con crecimiento de GTT registrado y estabilidad rechazada.
- [Seguridad](security.json): decodificación ACE acotada e identidades de origen
  y revisiones pendientes en la cola de instalación.

Las cifras de carga se separan de conducción. Las extensiones visuales propias
se prueban por continuidad, coste y comportamiento; no se exige igualdad de
píxeles con Open Rails. La presencia de archivos no certifica todos los sistemas
de un tren, y una formación incompleta conserva el diagnóstico de faltantes.

Las pruebas aún en ejecución no se consideran aprobadas. El recibo final debe
incluir el check completo, el recorrido original sin servicios fallidos y las
dos horas reales de prueba integrada antes de cerrar los cuarenta issues.
El recorrido y el check ya cuentan con recibo aprobado; el crecimiento de GTT
en la prueba prolongada continúa investigándose y no se presenta como estabilidad.

La corrección posterior reutiliza tres texturas de partículas entre escenas.
El check cubre doce recargas y datos de shelter renovados; la nueva prueba de
dos horas añade asignaciones vivas y bloques reservados de Vulkan. El test
unitario no sustituye la aceptación de estabilidad.

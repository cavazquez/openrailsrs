# Recorrido, estaciones, segunda formación y paquete Linux

Las seis imágenes de estaciones se inspeccionaron contra las referencias
originales de Denham, Denham Golf Course y Gerrards Cross, desde cabina y exterior.
`station-cameras.json` incluye las doce vistas de Chiltern, con los límites
originales de 3 m, 1°, 0,05° de FOV y 0,001 de aspecto. Las imágenes no certifican
paridad de iluminación ni una coincidencia global de píxeles.

`chiltern-performance.json` registra el viaje completo en la RX 7600, Vulkan,
1280×720, radio 450 m y simulación ×16. `chiltern-outcome.json` es la repetición
sin render, usando el mismo conductor. Los dos servicios de tráfico solo están
presentes en la prueba gráfica. La formación del jugador es el CON original.

`class47-physics.json` corresponde al ensayo original de 250 segundos; no es el
recorrido completo de Demo Model 1. `class47-cab-comparison.json` comprueba su
cámara y geometría. Los dos informes de audio cuentan los SMS y WAV originales
cargados y detallan los archivos ausentes del paquete oficial. No se versionan
los WAV de demostración ni los recursos descargados.

`images.json` permite verificar las capturas. Los valores de rendimiento son
mediciones de este equipo; el cambio de horario reduce el tiempo real del viaje
acelerado y no se interpreta como una mejora del renderizador.

`checks.json` resume el chequeo completo. `linux-package-check.json` y
`linux-package-menu.png` registran la extracción real, el chequeo de dependencias,
una simulación y el menú desde una carpeta con espacios. La construcción de QA
probada parte del commit anterior con estos cambios locales; `source_dirty`
lo indica. El archivo final se genera después del commit y conserva la identidad
de sus fuentes en `BUILD.json`. Estos binarios requieren glibc 2.43.

`demo-performance.json`, `demo-outcome.json` y `demo-service-completed.png`
registran la repetición completa con Class 47: tres paradas y 28,08 km. Conserva
los 600 s de espera final y llega 105/108 s tarde en las dos últimas estaciones.
Esta prueba de continuidad gráfica se distingue del oráculo fijo de 250 s.

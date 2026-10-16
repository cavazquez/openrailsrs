# Mesas operativas: evidencia de #344 y #345

Las cuatro imágenes originales muestran [mesa giratoria intermedia](turntable-mid.png),
[mesa giratoria alineada](turntable-final.png), [transferencia intermedia](transfer-mid.png)
y [transferencia alineada](transfer-final.png). Son vistas exteriores relevantes de
geometría propia de laboratorio a 1280×720, renderizadas en Vulkan con RX 7600.
El tablero y su vehículo permanecen juntos; las salidas se ven alineadas al final.

[qa-summary.json](qa-summary.json) conserva hashes, controles, estado, transformaciones,
órdenes y resultados reales. Se aplicaron freno 100%, neutro, pausa y restauración JSON
sin prescribir posición o velocidad. La cámara visitó 5000 m y volvió a orbitar, ejerciendo
el origen flotante real. Los cuatro visores/compositores aceptados salieron con código 0.

Se conserva la historia de intentos: yard01 tenía un error de mando del arnés; yard02
pasaba datos pero su menú de pausa tapaba la maniobra. En yard03 se aceptaron tres casos;
el cuarto fue interrumpido antes de producir PNG (sus returncodes originales siguen
null). El reintento final sí terminó 0 y produjo la cuarta imagen publicada. Ningún fallo
previo se reemplaza por un resultado aprobado.

Los contadores corresponden a toda la aplicación: bins de cuadro 25 ms y HUD alrededor
de 30,8 FPS, con picos RSS 527–563 MiB. No hay baseline sin mesa ni medición de coste marginal;
no se deduce estabilidad de memoria ni rendimiento aislado de estas capturas.

La [guía de patios](../../../../OPERATIONAL_YARDS.md) separa esos resultados de las
pruebas CPU y explica el recorrido manual desde el menú. No se publican datos privados.

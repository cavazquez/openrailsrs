# Snap alpha.10: paquetes por URL

Publicado el 18 de octubre de 2026: **0.1.0-alpha.10**, revisión **9**, canal
**latest/edge**, Linux amd64, core24 y confinamiento estricto.

La descarga de Snap Store coincide byte por byte con el paquete probado.
SHA-256: `f7af69fa24a26fe0f577e2160fd6a17bcd81fef2a0224edf76f6e2c6c50f376b`.

Pasaron las tres importaciones HTTPS reales de los ejemplos GPL: sólo ROUTES,
sólo TRAINS y ambas carpetas. Se comprobaron el hash publicado, la importación
nativa de la ruta y las formaciones sin recursos faltantes. También pasaron el
catálogo, un servicio con paradas, el ejemplo de vapor y la captura de Biblioteca
con el botón para vincular originales dentro de la ventana. Las pruebas se
hicieron con un usuario sin privilegios dentro del Snap instalado.

Se conservaron tamaño y fecha de modificación de 71.700 archivos existentes.
Las pruebas usaron una carpeta de jugador aislada. La captura usa Vulkan por
software; no añade una certificación de otras GPU o sesiones Wayland. Los
paquetes de ruta son ejemplos mínimos del formato, sin actividades ni paisaje
completo. Los recursos comerciales permanecen fuera de la distribución.

El [recibo](release.json) registra hashes, fuentes y alcance de las pruebas.
La [guía](../../../CONTENT_PACKAGES.md) incluye los manifiestos públicos para
repetir la importación y los comandos para crear paquetes.

Las [fuentes exactas](https://github.com/cavazquez/openrailsrs/releases/tag/v0.1.0-alpha.10)
se adjuntan como `openrailsrs_0.1.0-alpha.10_source.tar.gz`, con el manifiesto
`packaging/source-build.json`. GitHub confirmó su SHA-256 después de la subida.

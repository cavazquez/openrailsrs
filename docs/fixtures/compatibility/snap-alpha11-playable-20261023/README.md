# Snap alpha.11: escenarios con trenes disponibles

La revisión **10** se publicó en **latest/edge** y se descargó otra vez desde la tienda. Sus bytes coinciden con el candidato probado. El [recibo](release.json) registra hashes, fuentes y alcance.

Con un usuario vacío en un contenedor core24 y confinamiento estricto:

- El Snap anterior mostraba **0** trenes disponibles al entrar en una ruta sin paisaje y con una cabina faltante.
- El candidato revisó los **63** servicios incluidos. Ofrece **53**; los otros **10** quedan fuera del selector.
- La primera selección muestra **19** formaciones disponibles y un modelo 3D. Se prioriza un tren con modelo y un paisaje completos.
- Se comprobó el inicio de cada servicio ofrecido, se cargó una partida gráfica y se completó el recorrido de laboratorio Northolt Park–West Ruislip.
- Pasaron **848** pruebas del visor y **17** de empaquetado; **46** pruebas que requieren recursos o entornos externos conservan su exclusión.
- Los recursos del paquete son idénticos a los de la revisión 9. No se agregaron originales comerciales.

Las capturas son [antes](before.png), [menú corregido](menu.png) y [laboratorio cargado](scene.png). Son ejercicios técnicos con modelos sintéticos propios. Esta comprobación no certifica toda la física, todas las actividades completas ni una matriz de GPU físicas.

Las fuentes completas están en el adjunto de [alpha.11](https://github.com/cavazquez/openrailsrs/releases/tag/v0.1.0-alpha.11); el manifiesto de hashes identifica el árbol usado para compilar. El código de la rama de desarrollo y el archivo automático del tag tienen un ciclo de actualización diferente: para reproducir esta entrega se usa el adjunto indicado.

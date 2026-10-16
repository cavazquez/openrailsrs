# Cabina y exterior en las seis estaciones de Chiltern

Comparación con Open Rails **1.6.1**, usando las referencias originales
congeladas en `fixtures/visual/or_reference/chiltern_station_views/`.
Las doce capturas Bevy y sus metadatos están en
`fixtures/visual/bevy_station_views_20261015/`.
Se usa la misma instalación Chiltern de esas referencias, no una captura nueva
de Chiltern v4; la auditoría de señales de v4 tiene su evidencia independiente.

## Condiciones y resultado

Northolt Park, South Ruislip, West Ruislip, Denham, Denham Golf Course y Gerrards
Cross: 09:55, verano, despejado, 1280×720, alcance de 450 m, cabina a 45° y
exterior a 160 m. La posición difiere como máximo **1,73 m**, dentro del límite
previo de 3 m; orientación, proyección y vector solar también pasan sus límites
fijados. Las doce vistas requieren cero subidas GPU, terreno y formas próximas
pendientes. Las tres casas de referencia pasan composición y transformación
contra los originales con el mismo límite de 1 mm.

La revisión visual comprueba estación, pasarelas, edificios, cercos, vegetación,
vías y paneles opacos de cabina. Las superficies recortadas conservan ventanas y
follaje, y no se reemplazan casas o materiales del autor. La formación del
jugador puede verse por el parabrisas en una curva: OR oculta sus coches en esta
captura de cabina. Los recursos ausentes del paquete tampoco se inventan.

Se corrigió la configuración de sombras: el presupuesto visual sobreescribía
las cascadas de inicio y desplazaba su primera transición de 10 a 24 m. A calidad
completa se conserva ahora la misma configuración durante el juego, con el
detalle cercano de ruedas, edificio y follaje. Las calidades reducidas mantienen
su presupuesto explícito. La regresión CPU comprueba el arranque y la aplicación
del presupuesto sobre la misma luz.

El sol de Bevy continúa moviéndose de forma continua. OR conserva aquí una tabla
de cielo actualizada por intervalos de veinte minutos: a las 09:55 utiliza el
anclaje de las 09:40. Para comparar sombras se agregó una opción **exclusiva de
captura** que congela el vector original. No cambia el modelo del jugador.
Se conservan el cielo, tonemapping, sombras y transiciones LOD de Bevy, sin copiar
el recorte negro del terreno lejano visible en las referencias OR.

## Pausa, cámara y coste observado

Un segundo ensayo mueve sólo la cámara 7,05 km y vuelve al inicio, con la física
pausada. Completó la trayectoria y los desplazamientos del origen flotante,
manteniendo el odómetro exactamente en 0 m. No dejó terreno, formas próximas ni
shaders pendientes al terminar. El cambio cabina/exterior se captura además en
cada estación.

En esa trayectoria, RX 7600/Vulkan con Weston privado: 2.119 cuadros de juego,
P95 de 25 ms, P99 de 26 ms, cuadro máximo de 53,3 ms y cero cuadros de juego de
más de 100 ms. El arranque registró un máximo de 812 ms. Está activo el límite
de 40 FPS; esto comprueba el comportamiento observado, **no** la capacidad máxima
de la GPU ni un viaje ferroviario completo. Los metadatos conservan memoria y
envíos WORLD/sombras, separados de UI, PBR convencional y partículas.

## Repetir

```bash
python3 scripts/capture_route_views.py --route-root "$CHILTERN_ROUTE" \
  --with-cab --capture-or-focus --match-native-lighting \
  --headless-wayland --require-hardware --out-dir tmp/nueva-comparacion
python3 scripts/compare_station_views.py --captures tmp/nueva-comparacion \
  --station northolt-park --station south-ruislip --station west-ruislip \
  --station denham --station denham-golf-course --station gerrards-cross \
  --output tmp/nueva-comparacion/comparison.json
```

El comparador verifica los hashes originales, las poses, el reloj, el sol y la
carga terminada. Sus controles negativos rechazan un cambio de cámara, un sol
diferente y una referencia modificada. No asigna una puntuación global de
parecido entre motores.

Para probar jugando, seleccionar **Chiltern extendido**, verano y despejado;
usar **1** y **2** al detenerse en cada estación. Con **P**, acercar y alejar la
cámara: las sombras próximas deben seguir visibles, el sol y el tren quedan
quietos y el LOD puede terminar de cargar sin mover la formación. Al continuar,
el paisaje debe conservar las mismas posiciones después del cambio de sector.

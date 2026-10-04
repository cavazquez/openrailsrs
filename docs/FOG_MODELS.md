# Modelos de niebla para el recorrido

La búsqueda se hizo sobre fuentes primarias y se contrastó con Bevy 0.19.1
instalado. El resultado es una selección en F10 que conserva **Atmosférica** como
valor predeterminado y ofrece **Volumétrica · 32 pasos** y **64 pasos**.

## Distancia y dispersión atmosférica

`DistanceFog` permite caída exponencial, exponencial cuadrática y atmosférica.
La atmosférica separa extinción y dispersión, con coeficientes por color. Es
adecuada como bruma lejana estable y económica en un recorrido largo. El alcance
visual depende del clima y del sol, independientemente del radio de carga.
Fuente: [Bevy FogFalloff](https://docs.rs/bevy/latest/bevy/prelude/enum.FogFalloff.html).

## Niebla volumétrica y luz de faros

`FogVolume` admite una textura de densidad tridimensional y `VolumetricFog`
integra la dispersión con luces. Se usa una densidad que decrece con altura,
con variación horizontal suave, para concentrar la bruma junto a la vía.
La integración nativa permite que los faros participen en el volumen.
Más pasos mejoran la integración y aumentan el costo; por eso la calidad alta
es opcional. Se deja `ambient_intensity=0` sin mapa de entorno y `jitter=0`
sin antialiasing temporal, siguiendo la documentación y el ejemplo oficial.
Fuentes: [FogVolume](https://docs.rs/bevy/latest/bevy/light/struct.FogVolume.html),
[VolumetricFog](https://docs.rs/bevy/latest/bevy/light/struct.VolumetricFog.html),
[ejemplo oficial](https://docs.rs/crate/bevy/latest/source/examples/3d/volumetric_fog.rs).

La técnica de Wronski combina medios de densidad variable, varias luces y
sombras volumétricas. Refuerza la elección de un volumen para lluvia, niebla y
faros. Su implementación con compute/froxels es una referencia para una futura
optimización; este cambio utiliza el renderer nativo de Bevy.
Fuente: [Wronski, SIGGRAPH 2014](https://bartwronski.com/publications/).

## Prueba y límites

Compará las tres opciones desde la misma posición y hora, con Niebla y faros
altos. F8 → Diagnóstico muestra percentiles por cuadro y RAM del viaje; el JSON
de captura conserva las mediciones. F deshabilita/habilita la niebla. El
volumen sigue el centro de la cámara y la altura ferroviaria, sin reconstruir
su textura cada cuadro. No se agrega una dependencia externa de clima.

La calidad atmosférica sigue siendo el punto de partida para equipos modestos.
Una prueba con lavapipe sirve para comprobar shaders y recursos, pero sus FPS
no representan una GPU dedicada. El perfil de niebla es una adaptación Bevy,
no una reproducción píxel a píxel del clima de OR 1.6.1.

La inspección nocturna inicial detectó una franja negra al usar un volumen
uniforme de 12 km. Se sustituyó por una capa local de 1800×120×1800 m, centrada
en la cámara y la vía, con densidad exponencial por altura y transición radial
suave a cero en sus bordes. La textura 32³ usa filtrado lineal y ClampToEdge.
Las capturas nocturnas de cabina y exterior con 32/64 pasos ya no muestran esa
pared; los faros siguen iluminando la vía y las sombras están habilitadas.
La regresión portable comprueba bordes nulos y transmisión horizontal mayor
al 85 % a la altura de cabina. La atmosférica sigue siendo la opción económica
y predeterminada; el volumen agrega dispersión local.

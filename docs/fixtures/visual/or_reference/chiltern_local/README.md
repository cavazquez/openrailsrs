# Estaciones de Chiltern, Open Rails 1.6.1

`northolt_exterior.png` es una captura del render original con la actividad
local generada, pausada a las 09:55. El commit nativo, hash de imagen, posición,
matrices de cámara, ventana 1280×720, órbita de 160 m y distancia de dibujo
450 m quedan en `northolt_exterior.json` y `oracles/openrails-reference.toml`.

El consumidor `tools/openrails-reference/SceneryCapture.cs` carga las DLL
originales sin modificarlas. Sólo congela la simulación, fija la cámara y
desactiva los contadores CPU/memoria del Host, cuya función PDH no está
implementada en Wine. El render se ejecutó con D3D11 integrado de Wine y
llvmpipe, en un prefijo privado. Se capturó una sola aplicación a la vez.

Esta referencia sirve para casas, caminos, andén y terreno. Tres coches del
Content original remiten a nombres de shape ausentes; por eso no se usa esta
imagen como aceptación del consist. La cámara Bevy utiliza un foco adelantado
respecto de la cabeza, y su iluminación física difiere de la nativa. Todavía
no se establece un presupuesto de píxeles entre ambos renders.

La composición de casas sí se comprueba contra la DLL original en
`oracles/chiltern-scenery.json`: conteos exactos y distancia LOD ±1 mm.
El oráculo de instancias GPU tiene su propio presupuesto de silueta ≥98 %.

`northolt_low_exterior.png` registra la misma estación a las 09:55 con yaw
1,6, pitch −0,15 y distancia 160 m. Se observa el reverso abierto de caminos,
jardines y viviendas también en Open Rails 1.6.1. Sirve para distinguir esta
limitación del contenido de un error de altura del importador Bevy. La cámara
se fija con `OPENRAILS_REFERENCE_CAM_PITCH=-0.15`; pose, matrices y hashes
quedan en `northolt_low_exterior.json`. El solapamiento visual no se mide contra
esta vista porque el visor jugable limita la cámara exterior sobre el RAW.

`south_ruislip_exterior` y `west_ruislip_exterior` registran las otras dos
estaciones a las 09:55 de verano, con yaw 1,6, pitch 0,6 y distancia 160 m.
Se cambia únicamente el objetivo de cámara; el tren permanece en Northolt.
Sirven para comparar edificios, andenes, caminos y terreno. El consumidor
espera que la ventana WORLD contenga el tile objetivo antes de escribir los
metadatos; éstos incluyen tiles cargados, dirección solar y hash del consumidor.
Los márgenes negros del terreno corresponden al render nativo con distancia
de dibujo de 450 m; no se reproducen como requisito en Bevy.

El cielo nativo interpola muestras solares cada 20 minutos. Bevy evalúa la
ecuación directamente para la hora de la partida y conserva su iluminación
física y tonemapping. El oráculo de ecuación solar y el de composición de casas
siguen midiendo compatibilidad; estas imágenes no establecen igualdad de píxeles.

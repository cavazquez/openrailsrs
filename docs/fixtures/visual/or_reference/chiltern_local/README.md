# Northolt Park, Open Rails 1.6.1

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

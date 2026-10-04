# Escenario, noche y tráfico — aceptación 2026-10-04

Referencia: Open Rails **1.6.1**, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`. Renderer: Bevy **0.19.1**.
Recorrido: Northolt Park → South Ruislip → West Ruislip, Pullman de ocho coches.

## Evidencia original y límites de comparación

Se agregaron seis capturas nativas, con sus matrices y hashes, en
`docs/fixtures/visual/or_reference/chiltern_station_views/`: cabina y exterior
para cada estación, 09:55 de verano, 1280×720, distancia 450 m y cabina a 45°.
Las imágenes originales anteriores y las DLL de OR conservan sus hashes.

El oráculo espacial sitúa las tres estaciones sobre sus plataformas originales
con error 0,000 m en este Content; la tolerancia sigue siendo 3 m. Las tres
casas de referencia pasan composición, índices y transformaciones con tolerancia
1 mm. Las diferencias de encuadre, iluminación y material rodante impiden
interpretar una resta global de píxeles entre motores como fidelidad física.

Un import a `bevy_pbr::pbr_lighting` mantenía shaders en espera y ocultaba el
terreno. Bevy 0.19.1 registra ese módulo como `bevy_pbr::lighting`. Se corrigió
y las capturas ahora exigen cero pipelines pendientes o fallidos, además de
la terminación de la carga de WORLD. Un timeout falla en lugar de aceptar
una imagen incompleta.

Las seis capturas Bevy pasaron carga y compilación GPU: 3–4 tiles WORLD activos,
cero objetos cercanos pendientes y pico máximo **3721 MiB RSS** a 450 m. Se
inspeccionaron los seis pares. Andenes, casas, fábrica, cercos y puente conservan
sus geometrías originales; follaje y ventanas mantienen recortes transparentes.
La cabina conserva el puesto original y sus instrumentos. Bevy conserva su
tonemapping y sombras: no copia los triángulos negros visibles en los bordes
de algunas capturas del renderer original.

Se conservan diferencias de encuadre entre motores. El visor ahora precarga
pequeños grupos de objetos estáticos del mismo modelo con una esfera conservadora,
incluyendo 100 m de visibilidad nativa, en celdas de 256 m. Así se completan las
hileras próximas sin cargar de golpe todo un tile. El margen de precarga es 128 m;
la distancia predeterminada sigue siendo 2000 m. Los grupos de instancias respetan
histéresis del 8 % y cambian de LOD con 0,35 s de tramado complementario en color
y sombras. El resto del escenario conserva la histéresis sin fundido.

## Noche, faros y clima

La prueba en Xvfb privado con Vulkan lavapipe confirma estrellas en Despejado
nocturno, terreno oscuro, variantes Night, instrumentos luminosos y el cambio
de iluminación interior con I. Las estrellas son procedurales y repetibles;
no representan un catálogo astronómico.

Los faros siguen las posiciones ENG y el marco de cada vehículo, incluida
su inversión. Bevy convierte su intensidad de foco dividiendo por 4π; se expresa
el haz alto como 250.000 cd y el bajo como 50.000 cd antes de convertir al valor
de entrada. Las condiciones originales siguen decidiendo qué lámpara se activa:
el Pullman declara blancos en altos y marcadores rojos en bajos. En una región
fija de vía de 240×65 píxeles, la media RGB pasó de **5,08 apagados a 30,00 altos**;
80,0 % de los píxeles cambiaron más de ocho niveles. Esto comprueba el efecto del
haz, no la paridad fotométrica con OR. Las sombras permanecieron habilitadas.

Lluvia sobre vidrio y limpiado por dos escobillas utilizan el reloj de simulación,
con máscara de profundidad para tablero y marcos. El barrido es una proyección
aproximada, no un perfil calibrado para toda cabina instalada. La niebla
volumétrica 32/64 es opcional; la atmosférica sigue siendo el valor predeterminado.
La investigación y fuentes primarias están en `docs/FOG_MODELS.md`.

Se inspeccionaron lluvia sin barrido, con barrido y desde exterior: las gotas
quedan sobre las ventanas, desaparecen en los arcos limpiados y no se superponen
a la pantalla exterior. Las dos calidades volumétricas compilaron sin pipelines
pendientes; con faros altos se ve la vía y no hay estrellas en Niebla. El perfil nocturno anterior presentaba una franja negra en el horizonte.
La nueva capa local de 1800×120×1800 m, con densidad exponencial por altura y
bordes radiales suaves, elimina esa pared en las capturas de cabina y exterior
con 32/64 pasos y sombras habilitadas. La sesión anterior alcanzó 2989 MiB RSS
y comprobó el retorno al menú liberando el renderer anterior.

## Formaciones instaladas

El auditor inspeccionó **184** archivos CON: **153 utilizables** y **31 con
archivos faltantes**, principalmente recursos de `common.cab`. Se corrigieron
cinco rechazos por `Graphic (None)` y una CVF con sus bloques completos pero sin
el cierre exterior, aceptada también por el lector original. No se agregaron
cabinas o texturas ficticias para ocultar los faltantes reales. Verifica parámetros,
ENG/WAG, modelos, texturas declaradas y gráficos de cabina; no certifica física,
sonido, scripts C# ni todos los sistemas de cada locomotora. El menú bloquea
formaciones incompletas e informa material estático y cabinas genéricas.

```bash
target/debug/openrailsrs audit-consists "/ruta/Chiltern/TRAINS/CONSISTS" --json
```

## Comprobaciones reproducibles

```bash
CARGO_BUILD_JOBS=2 OPENRAILSRS_NATIVE_ROUTE="$CHILTERN_ROUTE" ./check.sh
python3 scripts/capture_route_views.py --route-root "$CHILTERN_ROUTE" \
  --with-cab --cab-fov-deg 45 --software --out-dir tmp/station-materials
python3 scripts/check_viewer_streaming.py --route-root "$CHILTERN_ROUTE" \
  --scenario examples/chiltern_traffic/scenario.toml --checkpoint terminal \
  --software --timeout-s 480 --out-dir tmp/traffic-streaming
```

La prueba headless de 1600 s completa tres paradas del jugador, dos del servicio
adelantado y una del contrario. Verifica señal 688 Alto → Vía libre, bloqueo por
ocupación de formaciones completas y guardado/restauración conjunta.

También se completó el recorrido con el renderer: **7054,10 m, 3/3 paradas**, dos
paradas del adelantado y una del contrario. En la terminal quedaron seis tiles
WORLD activos, cero objetos próximos pendientes y cero pipelines pendientes o
fallidos. Las tres formaciones reúnen 339 piezas animadas y comparten ocho modelos.
El informe reproducible queda en `tmp/traffic-streaming-final/report.json`.

Una captura exterior a 4333,57 m muestra los dos Pullman cruzándose en vías
contiguas. El contrario circulaba a 24,28 km/h, con su formación original visible;
el jugador ya había atendido dos paradas. La captura y sus estados están en
`tmp/traffic-crossing-wide/`, con cero objetos próximos pendientes y ocho modelos
compartidos entre las tres formaciones.

La medición anterior a esta iteración, a 1280×720, 2000 m de alcance y reloj ×16, registró **5876 MiB
RSS máximo**. En lavapipe los percentiles fueron P50 115 ms, P95 220 ms y P99
268 ms, con un máximo de 4506 ms; incluyen cuadros de carga y compilación. Esto
confirma cobertura hasta la terminal dentro del límite de 6 GiB, pero no demuestra
fluidez en una GPU dedicada ni una reducción de memoria frente a una versión
anterior. Hace falta repetir la misma medición con aceleración de hardware.

`check.sh` pasó formato, clippy sin warnings, tests seriales, geometría nativa,
composición de casas, cuatro oráculos físicos breves y el servicio completo
funcional. El ajuste posterior de intensidad tiene regresión de iluminancia y
clippy adicionales. La comparación física del viaje completo continúa fallando:
RMS de velocidad 7,5153 m/s y error máximo de odómetro 3029,15 m. Estas mejoras
visuales y operativas no declaran cerrada esa brecha.

El smoke visual de la iteración anterior repitió la captura con 0,000 % de píxeles fuera de tolerancia
(tolerancia 16, máximo 2 %). Se revisó y actualizó su encuadre, que antes colocaba
la cámara debajo del terreno; los motivos y hashes están en su procedencia.
El oráculo GPU de instancias alcanzó 1,000000 de coincidencia de silueta en sus
tres regiones, manteniendo el mínimo 0,98. Su ejecutable utiliza ahora la misma
carpeta de shaders compartidos que el visor.

Las instrucciones del jugador están en `docs/PLAYER_MANUAL_TESTS.md`, apartados
9–14. El audio original está habilitado y tiene volumen y silenciamiento en F10. Las pruebas gráficas usan un renderer por
vez y un límite de 6 GiB RSS; sus FPS no representan una GPU dedicada.


## Iteración de detalle, cielo, memoria, sonido y formaciones

El cielo usa ahora un material específico con gradiente vertical, nubes altas,
paletas continuas de amanecer/atardecer y cobertura según clima. Comparte el color
del horizonte con la niebla atmosférica durante todo el día; el paso por altura
solar cero no cambia abruptamente el modelo de niebla. Despejado nocturno deja
el cielo descubierto para las estrellas. La textura de densidad volumétrica
decae exponencialmente con altura y se desvanece radialmente, en un volumen local
de 1800 × 120 × 1800 m centrado sobre la vía. La bruma distante sigue a cargo de
DistanceFog. No es una simulación meteorológica ni un catálogo astronómico.

Las mallas de los LOD se generan en el trabajador de formas, conservando los
índices de bandas vacías. Los ShapeFile compartidos usan Arc; los lotes de parseo
y decodificación están acotados. Las transiciones reutilizan buffers uniformes y
bind groups. Las texturas estáticas usan RenderAssetUsages::RENDER_WORLD: Bevy
libera su copia CPU después de subirlas. Cabina y vehículos conservan sus píxeles
para instrumentos y variantes. Esta distinción se validó con una captura del
escenario real, además del smoke sintético.

La primera prueba con más precarga se detuvo al superar 6 GiB. Después del ajuste
de residencia de texturas, el mismo servicio con tráfico completó **7054,10 m**,
**3/3 paradas**, seis tiles WORLD en GPU, cero objetos próximos pendientes y cero
pipelines pendientes/fallidos. Pico **5394,9 MiB**, frente a **5875,7 MiB** de la
medición previa (−8,18 %). Informe: `tmp/traffic-streaming-five-resident/report.json`.
P50/P95/P99: **114/225/293 ms**, máximo **4631 ms**, con carga y compilación incluidas.
Las métricas se obtuvieron en lavapipe con comprobaciones de Rust concurrentes:
no demuestran mejora de FPS ni fluidez en hardware. `--require-hardware` rechaza
un resultado por CPU y el JSON identifica adaptador, backend y tipo de dispositivo.

El audio carga SMS/WAV por vehículo, con muestras compartidas entre los servicios.
Reconoce curvas de volumen y frecuencia, disparos de conducción, bucles con
introducción, marcadores y salida, y cambios de cabina/pasajeros/exterior.
Bocina mantenida produce flancos de encendido/apagado; los eventos de vapor siguen
la fase de ruedas. Cada coche atenúa por su posición y el tráfico conserva su
identidad. El canal acotado evita acumular mensajes y el banco limita la memoria
decodificada a 128 MiB. Reiniciar/restaurar reinicializa los disparos sin releer
las muestras. El oráculo WAV usa el mismo runtime y mixer, sin dispositivo.

El Pullman carga **10 programas, 39 streams y 69 muestras**, **10,45 MiB** decodificados,
sin advertencias de lectura. Se renderizaron demostraciones de cabina y exterior;
las pruebas portables verifican bucles/salida, cambio de escucha, silencio y flancos.
Esto no certifica paridad acústica completa: filtros de activación especializados,
variables combinadas, aleatoriedad/volumen de todos los disparos y scripts C#
siguen fuera del alcance actual. Curvas, disparos y comandos desconocidos
informan advertencias; esto no detecta todas las diferencias de comportamiento.

La validación gráfica de stock detectó que las formaciones nativas podían pasar
el auditor pero perder todos sus modelos: el catálogo sólo recogía carpetas con
SHAPES. Cada visual conserva ahora el directorio ENG/WAG de su entrada CON y
resuelve desde esa identidad, incluyendo modelos directamente en TRAINSET y
archivos con el mismo nombre en distintas carpetas. La cabina usa el mismo stock.
`check_rolling_stock.py` exige cero vehículos de reemplazo, cabina CVF cargada y
shaders listos; conserva las capturas y el auditor completo junto a los WAV.

Las cabinas clásicas CVF ya no requieren un modelo `.s` ni una CABVIEW3D.
Su fondo ACE y sus instrumentos se cargan desde el CVF propio del ENG;
al alternar 2D/3D se reconstruyen los enlaces de matrices del modo correspondiente.
Esta condición se verifica con una regresión portable y con el Class 121 nativo.

El chequeo completo posterior a la corrección de cabinas pasó: formato, clippy
con `-D warnings`, todos los tests del workspace y oráculos nativos. El visor
registra **518 tests pasados y 40 ignorados** en la suite portable; las dos
regresiones nativas de recorrido y estaciones se ejecutaron aparte y pasaron.
La prueba de apertura de audio del equipo, con volumen cero, cargó los diez
programas del Pullman y confirmó salida disponible sin advertencias.

Las capturas del Class 121 detectaron además un origen de cámara a nivel del
coche y referencias SMS compartidas que no se resolvían sin una carpeta Sound
local. Se usan Position/Direction del CVF en cada vista 2D, como CabCamera
en el código original, y se normalizan lexicalmente los segmentos `..` de
rutas SMS/WAV antes de buscar con equivalencia de mayúsculas. Las regresiones
comprueban altura, cambio de punto de vista, Flip y rutas hacia Kiha31 sin
una carpeta Sound local. Los recursos compartidos originales sí están instalados.

Los SMS de LUR y vapor contienen tablas de puntos negativos descendentes.
El lector conserva el orden y los límites primero/último del interpolador
OR 1.6.1, en vez de rechazar esas tablas o reordenarlas. Una regresión usa
los valores nativos de volumen/frecuencia. Las variables diésel son fracciones
y las eléctricas/vapor porcentajes; carga eléctrica y presión de admisión
siguen aproximadas por demanda, pues todavía no se expone toda esa telemetría.

Las condiciones CabCam/PassengerCam/ExternalCam del SMS prevalecen sobre el
bloque Wagon/Engine que contiene su referencia. King declara su programa externo
en Engine; clasificarlo únicamente como cabina dejaba el exterior casi silencioso.
Los servicios ajenos se escuchan con condiciones de cámara exterior, como
SoundSource.ConditionsMet de OR. Las referencias de audio prefieren el ENG/WAG
de la subcarpeta OpenRails cuando existe, conservando Sound relativo al stock.
Esto recupera LURcab en el eléctrico 1960 y los programas actualizados de vapor;
no incorpora las modificaciones físicas completas de esas variantes.

Un limitador de mezcla a −1 dBFS, compartido por dispositivo y oráculo WAV,
evita saturación al sumar motores/coches y bocina, conservando las muestras
tranquilas. Se renderizaron **12 WAV de 12 s**, cabina y exterior de las seis
formaciones: todos tienen señal, **0 % de muestras saturadas y cero advertencias**.
Los bancos tienen 69/48/32/54/93/93 muestras respectivamente. Evidencia:
`tmp/native-audio-five-complete/report.json`. Nivel RMS y ausencia de saturación
no certifican identidad acústica con OR.

La cabina Hall reveló que CabViewWindow es el rectángulo del parabrisas,
no el tamaño del panel. Los controles CVF mantienen la grilla **640×480**
que usa CabControlRenderer original; una ventana de 300×400 ya no desplaza
instrumentos ni agranda las palancas. Se conservan los fondos y gráficos
originales, con letterbox al adaptar su proporción a la pantalla.

Las seis formaciones se capturaron con sus modelos nativos: **13 vistas**
exteriores/cabinas, incluyendo 2D y 3D del Pullman, **cero vehículos de reemplazo**,
cero shaders pendientes/fallidos y cero objetos próximos pendientes.
El máximo observado fue **2901,7 MiB RSS** a 450 m. Las imágenes se inspeccionaron
individualmente; esta comprobación de recursos no valida todos los sistemas
de conducción de cada locomotora. Informe: `tmp/rolling-stock-five-complete/report.json`.

El smoke final pasó con **0,012 %** de píxeles fuera de tolerancia y RMS **0,730**
(16 niveles/canal, máximo 2 %). Se revisó y actualizó sólo la referencia interna
por el nuevo cielo; las capturas originales de OR mantienen sus hashes.
El oráculo GPU de instancias conserva **1,000000** de coincidencia en sus tres
regiones, frente al mínimo 0,98. `check.sh` completo pasó después de todas las
correcciones; también pasó la verificación de integridad de la referencia OR 1.6.1.

Una sesión de jugador a **23:02**, Despejado, confirmó estrellas visibles y
terreno oscuro. F8 mostró **salida de audio activa**, diez programas y 69 muestras
del Pullman, con volumen cero durante la inspección. Al restaurar Lluvia, las
estrellas desaparecieron y el parabrisas mostró gotas con faros sobre la vía.
Con V se despejaron los arcos barridos; al salir al exterior desapareció el
efecto de gotas sobre la pantalla y se mantuvo la precipitación. El pico de
esta sesión fue **3163,6 MiB RSS** con un único renderer.
Capturas: `tmp/weather-five-final/`. La inspección usa partidas y ajustes privados;
los guardados del jugador se conservaron.

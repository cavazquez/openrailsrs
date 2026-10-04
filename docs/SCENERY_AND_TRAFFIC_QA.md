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
cero objetos cercanos pendientes y pico máximo **3335 MiB RSS** a 450 m. Se
inspeccionaron los seis pares. Andenes, casas, fábrica, cercos y puente conservan
sus geometrías originales; follaje y ventanas mantienen recortes transparentes.
La cabina conserva el puesto original y sus instrumentos. Bevy conserva su
tonemapping y sombras: no copia los triángulos negros visibles en los bordes
de algunas capturas del renderer original.

Quedan diferencias de encuadre y detalle lejano. OR agrupa instancias estáticas
por tile y amplía su esfera de visibilidad; el visor activa objetos próximos
individualmente antes de agruparlos. A 450 m algunas hileras lejanas aparecen
antes en OR. La distancia predeterminada de juego sigue siendo 2000 m y la
histéresis del 8 % evita alternancias de LOD, pero todavía no hay fundido de mallas.

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
pendientes; con faros altos se ve la vía y no hay estrellas en Niebla. El perfil
nocturno aún presenta una franja demasiado oscura en el horizonte del volumen;
su transición requiere ajuste. La sesión de menú/lluvia/niebla alcanzó 2989 MiB
RSS y comprobó el retorno al menú liberando el renderer anterior.

## Formaciones instaladas

El auditor inspeccionó **184** archivos CON: **147 utilizables** y **37 con
archivos faltantes**, principalmente recursos de `common.cab`. Verifica parámetros,
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

La prueba gráfica a 1280×720, 2000 m de alcance y reloj ×16 registró **5876 MiB
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

El smoke visual repitió la captura con 0,000 % de píxeles fuera de tolerancia
(tolerancia 16, máximo 2 %). Se revisó y actualizó su encuadre, que antes colocaba
la cámara debajo del terreno; los motivos y hashes están en su procedencia.
El oráculo GPU de instancias alcanzó 1,000000 de coincidencia de silueta en sus
tres regiones, manteniendo el mínimo 0,98. Su ejecutable utiliza ahora la misma
carpeta de shaders compartidos que el visor.

Las instrucciones del jugador están en `docs/PLAYER_MANUAL_TESTS.md`, apartados
9–13. El audio permanece desactivado. Las pruebas gráficas usan un renderer por
vez y un límite de 6 GiB RSS; sus FPS no representan una GPU dedicada.

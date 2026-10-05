# Verificación de las cinco mejoras de partida

La ampliación del 4 de octubre de 2026, al final de este documento, agrega los
modos de nieve, los goldens y el host C# opcional a las verificaciones anteriores.

Referencia: Open Rails 1.6.1, fijada por el proyecto. Verificación local del
4 de octubre de 2026, Rust 1.97.1 y Bevy 0.19.1. Las instrucciones para el jugador
están en [PLAYER_MANUAL_TESTS.md](PLAYER_MANUAL_TESTS.md), secciones 15–22.

## Implementación

- Cabinas: perfiles por CVF para altura/distancia del asiento 3D y alcance del
  limpiaparabrisas, con persistencia y restauración. Instrumentos de carga
  separados de RPM; ventana CVF y disposición de escobillas por cabina.
- Efectos: emisores ENG originales para escape diésel y vapor, partículas en
  coordenadas del mundo, sin sombras, limitadas a 512. Mojado gradual de terreno
  y materiales opacos PBR, con actualizaciones acotadas y sin duplicar texturas.
- Servicio: Northolt Park, South Ruislip, West Ruislip, Denham, Denham Golf Course
  y Gerrards Cross, 15,32 km, con dos servicios de tráfico. PAT/TDB y frenos de
  los ocho vehículos quedan identificados por SHA-256 en la procedencia.
- Detalle y memoria: transición de 0,35 s para mallas WORLD rígidas, máximo 64
  mallas salientes; telemetría de cuadros/RAM y comprobación de cobertura de los
  sectores en el viaje real. Las piezas animadas conservan sus enlaces.
- Física del recorrido: límites por posición/sentido, aplicación de aumentos
  después de la cola, pendientes por sección y parámetros EP/aire por vehículo.
  El tren inicia frenado en el andén para sostener la pendiente.

El embarque y la espera de horario se muestran por separado. La práctica rápida
reduce el embarque a cinco segundos y omite la espera de horario. La salida
anticipada con puertas cerradas permite continuar y queda registrada; las
puertas abiertas mantienen el corte de tracción actual. El horario del servicio
normal y los oráculos congelados permanecen intactos.

## Comprobaciones automatizadas

Los resultados de esta sección y del primer viaje gráfico corresponden a la
base `0dd3502`; la ampliación con nieve y SIGSCR se identifica más abajo.

`OPENRAILSRS_NATIVE_ROUTE="$CHILTERN_ROUTE" CARGO_BUILD_JOBS=2 ./check.sh` pasó:
formato, Clippy con advertencias como errores, 1416 pruebas Rust, 11 regresiones
Python, geometría/movimiento nativos, composición de edificios, cuatro oráculos
físicos y el servicio corto completo. La lectura de un único emisor STF tiene una
regresión dedicada; las mallas de partículas vacías mantienen buffers válidos.

El generador extendido produjo nuevamente todos sus archivos con los mismos
bytes. Las seis plataformas coinciden con su TDB dentro de 0,002 m en las
coordenadas de renderizado de esta prueba, con tolerancia de 3 m. El servicio
extendido sin renderer terminó 6/6 en 1997,05 s y 15318,77 m; el mayor error de
detención fue 3,94 m y todas las velocidades de llegada fueron menores a 0,1 m/s.

## Viaje gráfico con GPU

Se ejecutó un único visor a la vez, 1280×720, distancia de escenario 2000 m,
conducción automática y tiempo ×16. Renderer: **AMD Radeon RX 7600 (RADV NAVI33),
Vulkan, GPU dedicada**; Mesa 26.0.8. La presentación utilizó Weston 14.0.2 en un
compositor privado. Xvfb no ofrece DRI3 para esta presentación AMD y su intento
fallido no se considera un resultado válido.

```bash
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/radeon_icd.json WGPU_BACKEND=vulkan \
python3 scripts/check_viewer_streaming.py \
  --route-root "$CHILTERN_ROUTE" \
  --scenario examples/chiltern_extended/scenario.toml \
  --checkpoint terminal --headless-wayland --require-hardware \
  --timeout-s 900 --out-dir tmp/five-gpu-journey-final
```

- Servicio completado: 6/6, 15318,81 m y 1996,45 s de simulación. Mayor error de
  detención: 3,94 m. Ambos servicios AI llegaron con sus paradas registradas.
- Captura en 177,0 s reales. Pico de RSS externo: **3007 MiB**, por debajo del
  límite de 6144 MiB. Seis sectores GPU, 6519 entidades y cero shapes cercanos
  pendientes al capturar.
- Histograma de toda la sesión: 6002 cuadros; P50 **25 ms**, P95 **34 ms**, P99
  **48 ms**. Incluye 4457 cuadros con carga activa. Hubo tres cuadros de más de
  100 ms; el mayor fue 3917 ms. Este resultado no afirma ausencia de tirones.
- Pipelines: cero pendientes y cero fallidos; log sin errores Bevy ni panic.
  Seis emisores originales, 77 partículas al capturar y cero transiciones LOD
  pendientes. No se usó lavapipe para estas mediciones de rendimiento.

PNG, JSON y log están en `tmp/five-gpu-journey-final/`. Se conservan como salidas
locales de QA y no se agregan al repositorio.

## Prueba de controles y cabina

Una sesión Xvfb/lavapipe separada verificó el menú extendido, lluvia y embarque
normal con los dos contadores visibles. En F10 se activó práctica y se guardaron
altura +5 cm, asiento hacia atrás +5 cm y barrido 105 % para `PULLMAN_GR.cvf`;
el archivo de ajustes contiene esos valores y la preferencia de práctica. Tras
reiniciar el servicio, Q/V completaron el embarque rápido y activaron el barrido.
F7 registró Northolt Park, 20 pasajeros y las seis estaciones, con práctica,
cinco segundos de pasajeros y espera de horario cero. Se inspeccionó el vidrio
despejado sin gotas sobre el tablero. La sesión terminó sin errores de renderer,
con pico de 3009 MiB. Sus capturas están en `tmp/five-gui-controls/`; estas pruebas
de interfaz no se usan para informar rendimiento de GPU.

`check_rolling_stock.py --formation KingLE.con --headless-wayland
--require-hardware` verificó también exterior y cabina 2D originales de la King:
ambas vistas cargaron sin geometría de sustitución ni errores, con catorce
emisores originales de escape/vapor detectados. Pico de RSS: 1422 MiB en exterior y 1415 MiB en
cabina. Estas capturas detenidas certifican recursos; la intensidad de las purgas
y del silbato requiere la prueba de movimiento/bocina de la sección 17 del manual.
La revisión final agregó el caso de amperímetros con escala bipolar: cero en
ralentí y carga positiva en tracción, verificado por pruebas de los controles CVF.

## Límites de esta entrega

La composición geométrica y la partida completa están comprobadas. La paridad
física de la captura histórica completa sigue fuera de tolerancia: RMS 7,5749
m/s y diferencia máxima de odómetro 2717,27 m, sin cambiar la referencia ni sus
umbrales. El servicio extendido ahora usa el subconjunto SIGSCR original de
Chiltern; no se afirma compatibilidad completa SIGSCR ni ejecución de C#.
Los parámetros nativos mejoran el modelo de
cilindros, pero no reproducen todas las válvulas/depósitos del original. El
amperímetro eléctrico sigue siendo una estimación. La nieve es cobertura visual;
no incluye temperatura ni adhesión por hielo. Algunas mallas originales pueden
necesitar ajustes específicos de normales/texturas para un acabado uniforme.

## Ampliación: señales originales, nieve y carga GPU

- Las seis estaciones tienen referencias cabina/exterior de Open Rails 1.6.1.
  Las tres nuevas referencias se capturaron con el adaptador de plataforma
  descrito en [su procedencia](fixtures/visual/or_reference/chiltern_station_views/README.md).
  Se verificaron los hashes de los binarios y del código de referencia antes y
  después; no se modificó el adaptador original fijado.
- Chiltern extendido incorpora 66 programas SIGSCR con dirección TDB, aspectos
  normal/distante originales y ocupación longitudinal de los coches. El guardado
  y restauración recalculan las señales del jugador y del tráfico sin avanzar el
  reloj. El intérprete limita tamaño, anidamiento y ejecución y falla en Alto.
- Nieve seleccionable: copos exteriores lentos, depósito en el vidrio y barrido,
  cobertura del terreno y de superficies superiores, variantes nativas según
  estación y visibilidad inicial de 500 m. Lluvia/nieve alcanzan los materiales
  de terreno, los originales opacos y las instancias GPU; mantienen el recorte
  alfa. El manómetro utiliza el cilindro del vehículo principal.
  Los modelos PBR utilizan una extensión del material de Bevy que conserva sus
  texturas y el material original para mojado y cambios de LOD; no tiñe toda la
  fachada al cubrir un techo.
- Las cargas GPU tienen un presupuesto flexible de 8 MiB por cuadro. Bevy 0.19
  omite una especialización si la malla aún no está subida: se agregó un reintento
  al terminar esa subida, también para recursos reemplazados durante streaming.
  La prueba visual confirmó tren y cabina con el presupuesto activo. El HUD se
  dibuja en la cámara de interfaz posterior al efecto del parabrisas.
- La pantalla inicial espera mallas, imágenes y shaders de GPU; no consume
  tiempo del servicio. La telemetría conserva las mediciones globales y separa
  el máximo de arranque de los cuadros de juego de más de 100 ms.
- La comparación detectó terreno desplazado en West Ruislip y Gerrards Cross:
  las entidades diferidas podían nacer con el origen anterior al recentrado del
  mismo cuadro. La creación progresiva ahora sucede después del cambio de origen.
  Una regresión de creación/recentrado simultáneos comprueba coordenadas y altura
  MSL; las capturas finales muestran la vía despejada en cabina y exterior.

Verificación final de esta ampliación: `check.sh` pasó con **1428 pruebas Rust**,
**11 Python**, Clippy, formato, geometría/movimiento de contenido nativo, los
cuatro oráculos físicos cortos y el servicio corto completo. Los archivos
generados del servicio extendido se reprodujeron byte por byte. Se conservaron
las salidas de simulación que ya estaban modificadas antes de este trabajo.

Se inspeccionaron las doce vistas de las seis estaciones con GPU dedicada,
1280×720, FOV de cabina 45° y radio 450 m. Se recapturaron las cuatro vistas
afectadas por el defecto de origen. Pico de RSS entre **1110 y 2045 MiB**,
66 programas nativos, cero errores SIGSCR, cero shaders fallidos/pendientes y
cero subidas GPU pendientes al capturar. Los resultados finales están en
`tmp/visual-snow-six-stations-final/`; las imágenes anteriores al arreglo se
conservan en `tmp/visual-snow-terrain-before/` para diagnóstico.

Nieve, lluvia y noche despejada se inspeccionaron en Northolt Park, cabina y
exterior, con tren/cabina visibles y HUD legible. Se observó nieve en suelo y
superficies superiores, lluvia limitada al vidrio y estrellas/instrumentos
luminosos a las 02:00. Evidencia local en `tmp/visual-snow-pbr-final/`,
`tmp/visual-snow-rain-final/` y `tmp/visual-snow-night-final/`. West Ruislip se
recapturó también con nieve tras corregir el origen, en
`tmp/visual-snow-aligned-snow/`, con pico de RSS de 1832 MiB. Estas capturas
detenidas no certifican por sí solas el costo del clima durante todo un viaje.

El Pullman de Bristol recorrió **80,88 m** con 2 emisores nativos/60 partículas,
y la King **81,39 m** con 14 emisores/90–108 partículas. Pasaron cinco vistas
de exterior/cabinas originales, sin geometría de sustitución; RSS 1432–1807 MiB.
Las cuatro mezclas SMS/WAV fuera del renderer pasaron sin advertencias ni
clipping. PNG, JSON y WAV están en `tmp/visual-snow-moving-stock/`; esta prueba
no reproduce todas las maniobras de purga/silbato de la locomotora de vapor.

El último viaje gráfico, después de corregir el terreno, utilizó la misma RX
7600/Vulkan, Weston privado, 1280×720, radio 2000 m y tiempo ×16:

- Completó **6/6**, **15318,90 m** y **2545,60 s** de simulación. Error máximo
  de detención **4,03 m**; las seis llegadas fueron a menos de 0,1 m/s. Ambos
  servicios de tráfico llegaron y registraron sus paradas.
- El servicio terminó con demoras: hasta **713,65 s** en Gerrards Cross. Este
  resultado acredita que el viaje se puede completar con las señales nuevas;
  no acredita cumplimiento del horario ni paridad física con OR.
- Captura en **211,5 s** reales; pico externo de RSS **3008 MiB**, seis sectores
  GPU, 6560 entidades y cero objetos cercanos pendientes.
- **7387 cuadros**, P50 **25 ms**, P95 **29 ms**, P99 **43 ms**. Incluye 4586
  cuadros con carga de WORLD. Máximo de arranque **3885 ms**; durante el juego
  hubo **un cuadro de 107 ms**. Se mantienen tirones medibles, aunque la subida
  a GPU esté distribuida.
- Cero errores SIGSCR, shaders pendientes/fallidos y subidas GPU pendientes al
  finalizar. Seis emisores nativos y 76 partículas, dentro del límite de 512.

PNG, JSON y log finales: `tmp/visual-snow-aligned-journey/`. Las métricas
anteriores de la base `0dd3502` se conservan identificadas arriba; cambiaron la
lógica de señales y la duración del servicio, por lo que no son un ensayo de
rendimiento equivalente cuadro por cuadro.

## Ampliación del 4 de octubre de 2026: nieve, web y issues pendientes

El check completo del workspace pasó formato, Clippy con advertencias como errores,
1434 tests Rust, tests Python, compilación, cuatro oráculos físicos congelados y
el servicio corto de tres estaciones. La revisión final del host añadió un plazo
para escritura y respuesta; pasaron nuevamente Clippy, los tests de fallo del host
y la aceptación Linux con el fixture C# consumido por Rust.

La matriz final de nieve ejecutó secuencialmente GPU, CPU y Mixto de efectos, y
renderizado completo por lavapipe. Misma cámara, hora, escena, resolución y 2048
partículas; verificó cargas de malla y adaptador. Los resultados y hashes se
conservan en [el fixture de ejecución](fixtures/weather/execution-2026-10-04.json).
El viaje GPU Auto llegó a seis de seis estaciones con nieve: pico RSS 2695 MiB,
pico VRAM del proceso 3191 MiB y una subida de semillas. Sus tiempos por cuadro
coincidieron con compilaciones; los [límites de interpretación](WEATHER_EXECUTION.md)
distinguen memoria, presentación limitada y rendimiento máximo.

Para [#170](https://github.com/cavazquez/openrailsrs/issues/170), la aceptación
completa sin `--record` pasó siete vistas, dos comparaciones con regiones de OR
1.6.1 y cuatro fallos provocados en el renderer. El [informe versionado](fixtures/visual/player_goldens/acceptance.json)
conserva métricas y hashes. UV reflejado, forward lateral, oclusor negro y tren
oculto fueron rechazados. El [manifiesto y la guía](fixtures/visual/player_goldens/README.md)
fijan tolerancias y enlazan las regresiones #165–#169. Esa aceptación no certifica
el paisaje completo ni los modelos ausentes del paquete de referencia OR.

Para [#164](https://github.com/cavazquez/openrailsrs/issues/164), se eligió host .NET
separado opcional y Rust por defecto. SDK .NET 10.0.401 en Linux: compilación,
protocolo JSONL, ACK, menú, restricción a 18 km/h y frenado físico pasaron. Los
fallos de proceso, lectura/escritura bloqueadas, secuencia y respuesta enorme
producen intervención. El visor lee el estado de la sesión. El [contrato](TCS_CSHARP_HOST.md)
detalla el subconjunto del API y la ausencia de persistencia del script.

La web se reconstruyó con seis páginas, plantilla común y contenido generado a
partir de las versiones del repositorio. Se probaron generación reproducible,
enlaces/anclas, paridad de `website/` y `docs/`, capturas y textos alternativos.
En el navegador se verificaron escritorio y móvil, menú y Escape, selección de
estaciones, galería y copia de comandos, sin errores de consola ni desbordamiento
horizontal. Las imágenes provienen del juego y tienen su procedencia registrada.

El preparador de pilotos nativos pasó los tests PAT/plataformas/texto y un servicio
Chiltern con formación/tráfico originales y tres paradas completas. Belgrano CC
continúa pendiente de sus archivos originales: el enlace público devuelve HTML de
acceso. No se incluyeron una ruta ni material rodante argentino ficticios. La
[guía de preparación](NATIVE_ROUTE_PILOT.md) documenta el siguiente paso.

Las [pruebas manuales](PLAYER_MANUAL_TESTS.md) incluyen los pasos 23–26 para copos,
modos de cálculo, siete vistas, host C# y nueva web, con el resultado esperado.

El renderer de instancias retiene todas las dependencias de archivo declaradas
por su shader embebido, también en aplicaciones sin materiales de terreno. El
oráculo independiente de instancias pasó con coincidencia de silueta **1,0 en
las tres regiones**; conserva el mínimo de 0,98 y no modifica sus capturas de referencia.
Cabina frontal y órbita conservaron también los goldens propios y las regiones
nativas OR. [Resultado de esa regresión](fixtures/visual/player_goldens/asset_import_check.json).

## Ampliación del 4 de octubre de 2026: hora y clima del lugar

El check completo pasó formato, Clippy con advertencias como errores, **1447
tests Rust**, tests Python, compilación, cuatro oráculos físicos congelados y el
servicio de tres estaciones. La prueba opcional del proveedor consultó realmente
Open-Meteo y validó fecha UTC y zona IANA Europe/London. Tras ajustar la emisión
del rayo, pasaron nuevamente Clippy, compilación y los casos de entorno/tormenta.
También se prueba el botón de Ajustes desde el menú y durante la partida: elegir
clima manual conserva la hora real seleccionada.

Cinco combinaciones se capturaron con el renderer real de Chiltern, 1280×720,
radio 900 m, RX 7600/Vulkan y compositor privado: ambos modos manuales, tormenta
manual nocturna, hora real con nieve manual, clima real con hora manual y ambos
actuales. La hora londinense incluyó horario de verano; el servicio conservó las
09:55. Hubo cuatro sectores GPU, cero shaders fallidos/pendientes y cero subidas
GPU pendientes en cada captura. Los hashes, selección, relojes y memoria quedan
en [el fixture de ejecución](fixtures/weather/live-environment-2026-10-04.json).

Se inspeccionó el rayo ramificado y su destello desde cabina. Los tests verifican
su posición fija, pausa, limpieza al cambiar de clima y demora del trueno por
distancia. La muestra de audio pasó límites de amplitud y decaimiento; las
capturas deshabilitan audio y no certifican una escucha manual. Caché vencida,
fallos de conexión, respuestas inválidas y cambio a manual se probaron sin red.
Estas capturas detenidas comprueban integración visual; no miden rendimiento
máximo ni un recorrido completo con consultas meteorológicas.

Los [pasos manuales 27](PLAYER_MANUAL_TESTS.md#27-hora-real-clima-del-lugar-y-tormentas)
permiten comprobar los selectores, pausa/aceleración, audio, guardado y respaldo.
La [guía de entorno](LIVE_ENVIRONMENT.md) distingue condiciones estimadas de
rayos procedurales y documenta atribución, ubicación y frecuencia de consulta.

## 5 de octubre de 2026: edificios, ejemplos y carga de texturas

- Nieve excluida de edificios y demás shapes WORLD, tanto en materiales
  estándar como en el shader original y las instancias GPU. Las variantes Snow
  de esos shapes también se excluyen al construir las bandas LOD. Se conservan
  día/noche y las variantes estacionales normales.
- Capturas en Northolt Park (despejado y nieve) y Gerrards Cross (nieve),
  1280×720, radio 900 m y RX 7600/Vulkan: fachadas y techos conservan textura;
  4 tiles GPU por vista, sin cargas ni shaders pendientes y sin fallos de shader.
  Son poses de estación; no certifican un nuevo viaje completo.
- Comparación Northolt Park, misma pose/hora/despejado: VRAM del proceso
  **1064,4 MiB en RGBA → 808,2 MiB en BC** (24,1% menos). Error RGB medio del
  área de escenario: **0,014/255**, percentil 99 **1/255**, sin píxeles con error
  mayor a 8/255 en esa región. El ahorro depende de la escena.
- Exportadas **12624 ACE** a DDS sin recompresión: 3858 DXT1 y 8766 RGBA8.
  Payload agregado: 21,22 → 17,87 GB (15,78% menos que RGBA). Una muestra de
  cada formato conserva exactamente mip0 al decodificar con Pillow; su fuente
  mantiene el hash. La copia masiva temporal se limpió para recuperar unos
  17 GiB; quedaron muestras y reporte en `tmp/cabin-dds-verification/`.
- Pruebas de bloques BC1/2/3, mips 4/2/1, alfa parcial y alternativa CPU,
  dimensiones con bloques parciales, exportación sin sobrescritura, escenarios
  anidados y materiales compartidos que excluyen edificios.
- Verificada la tecla **I** con entrada real en una ventana Xvfb privada y
  Vulkan software: HUD «cabina Sí/No» y aumento de brillo del interior 3D de
  noche. Xvfb no permite presentación Vulkan sobre AMD; las pruebas de GPU
  anteriores usan Weston privado. No se opera el escritorio del usuario.
- `check.sh`: **1455 pruebas aprobadas**, 43 ignoradas, 0 fallos; formato, clippy,
  build, pruebas Python, oráculos fijados y servicio completo sin ventana
  aprobados. Restaurados y verificados los 134 outputs generados, preservando
  los seis archivos que ya tenía modificados el usuario.

Evidencia con hashes, parámetros y mediciones:
[fixtures/textures/2026-10-05.json](fixtures/textures/2026-10-05.json).
Pruebas manuales: [sección 28](PLAYER_MANUAL_TESTS.md#28-luz-interior-ejemplos-y-dds).

## 5 de octubre de 2026: señales, formaciones, C# y peralte

- SIGSCR valida todas las ramas y reinicia los locales por actualización. Las
  órdenes de Alto se propagan antes de evaluar la señal anterior. INFO conserva
  su representación sin crear autoridad de parada. El corredor tiene 66
  cabezas, 15 tipos de programa: 44 NORMAL, 21 DISTANCE y un INFO.
- Reservas exclusivas del siguiente bloque en recorridos fijos: la regresión
  con dos trenes opuestos comprueba concesión única, Alto para el otro,
  guardado/restauración y liberación cuando sale la formación completa.
  Pasa además el servicio de seis estaciones con ambos trenes AI.
- Auditoría del Content instalado de Chiltern: **184 formaciones, 1980
  vehículos**, 153 formaciones con recursos obligatorios completos y aptas para
  iniciar; 31 incompletas. Los errores incluyen 33 referencias de cabina y una
  textura. Las 184 tienen algún aviso de compatibilidad: recursos completos
  no certifican sistemas. El JSON de evidencia enumera las formaciones
  incompletas; los sonidos opcionales se informan sin bloquear el inicio.
- Aceptación .NET real aprobada: ocho aspectos nativos, señales/postes por
  índice, máxima del tren, valores de enums OR, entradas inválidas, ACK/menú,
  errores y transferencia SIGSCR → C# → freno físico sobre Chiltern. Persistencia
  del script y hosts de freno/alimentación siguen pendientes.
- Oráculo aislado de la expresión C# original de OR 1.6.1: **10 casos**, error
  máximo **0,0000118253 m/s**, tolerancia fija **0,00002 m/s**. Una captura nueva
  coincide byte a byte; el capturador rechaza carpetas ya existentes.
- La inspección visual detectó que los fixtures reducidos del Pullman omitían
  su trocha y déficit. Se importaron los campos originales con sus unidades y
  hashes, conservando el replay histórico. Las pruebas cubren regeneración
  estable, campos ausentes y el alias nativo Carriage → Passenger.
- Entrada real en Xvfb privado, Vulkan software, 1280×720 y radio 450 m:
  F8 → Locomotora muestra la curva TSection de 2000 m, peralte redondeado 0 mm
  y confort **164,3 km/h** con los parámetros originales. F8 → Despachador muestra
  sus intervalos; el menú muestra los ocho avisos del Pullman y vuelve mediante
  un proceso nuevo después de cerrar la partida. RAM máxima observada en esta
  sesión de interfaz: 2787,2 MiB; no es una medición de GPU ni de viaje completo.
- `check.sh`: **1468 pruebas Rust aprobadas**, 43 ignoradas, 0 fallos; formato,
  clippy, build, pruebas Python, referencias fijadas y servicio completo sin
  ventana aprobados. Aceptación C# adicional aprobada. Restaurados y verificados
  los 134 outputs, preservando los seis cambios previos del usuario.

Evidencia y límites:
[fixtures/compatibility/2026-10-05.json](fixtures/compatibility/2026-10-05.json).
Pruebas manuales: [sección 29](PLAYER_MANUAL_TESTS.md#29-señales-compatibilidad-c-y-peralte).
Alcance técnico: [SIGNALS_AND_CONTENT_SCOPE.md](SIGNALS_AND_CONTENT_SCOPE.md).


## 5 de octubre de 2026: peralte, itinerarios, persistencia y descargas

- Perfiles compartidos por la pose del tren/cámara y la deformación de las
  mallas originales de vía. Oráculo nuevo: diez casos de `MarkSections`,
  estándares y conversiones C# originales de OR 1.6.1, con error máximo de
  cant/roll **0 m / 0 rad** frente a tolerancias **0,00001 m / 0,000001 rad**.
  Contacto geométrico de vía/pose por debajo de 1 mm, con grandes coordenadas.
- Despachador: bloque/agujas concedidos en conjunto, protección de la cola,
  posiciones compartidas y rechazo de órdenes manuales sobre agujas reservadas.
  Regresión de cruce por apartadero, itinerario alternativo que conserva destino,
  bloqueo hasta despejar la cola, ciclos de espera y guardados incompatibles.
  El servicio completo con tráfico pasa. La autoridad móvil sin señales no
  acumula una reserva por quantum.
- Save/Restore real de un TCS C#: reinicio con reconocimiento, límite y salida
  retenidos; guardado sin `Update`, identidad/tamaño validados y restore corrupto
  rechazado antes de modificar la sesión. El SDK .NET sigue siendo opcional y
  se requieren ambos hooks del script; no se habilitan otros hosts C#.
- Descarga real del ZIP oficial Demo Model 1: **272422379 bytes**, SHA-256
  registrado, **329994809 bytes** instalados en una carpeta de prueba independiente.
  Auditoría/importación: **23 formaciones**, **9** con recursos completos y
  tracción para iniciar, **1 actividad**. Las nueve alternativas son AI y no
  incluyen cabina; la formación del jugador requiere ampliar `Include`.
  Se conservan los archivos originales,
  los avisos y la instalación Chiltern. Esto no certifica todos sus sistemas.
- Arranque nativo SCE comprobado después de corregir una falsa alineación entre
  el centro del recorrido y la estación inicial. El terreno conserva el marco
  TDB; una cola inicial vacía durante streaming termina sin acceder fuera de
  sus límites. En la actividad sin paradas programadas, el HUD muestra distancia
  al destino y no anuncia llegada al inicio. Estas tres regresiones tienen tests. Inversor/regulador reales alcanzaron
  **4,6 km/h** y la distancia a señal bajó de **211 a 205 m**. Es una prueba
  corta de arranque/controles con formación AI, no una validación del viaje SCE
  completo ni de su cabina original.
- Security Review local: un hallazgo Medium confirmado en redirecciones.
  Corregido al validar cada destino antes de emitir el siguiente GET; prueba
  en memoria con redirección a HTTP/localhost u otro proveedor, sin contactar
  el destino. Además se confinan rutas canónicas y se rechazan metadatos del
  importador incluidos en un ZIP. No se demostró un ataque remoto completo a
  partir de un manifiesto local manipulado.
- GitHub también informó 18 alertas previas de Pillow en las herramientas
  visuales. Se actualizó el pin de 12.1.1 a **12.3.0**, la versión corregida
  indicada por los avisos. En un entorno temporal con Python 3.14 pasaron las
  tres pruebas de goldens, las cuatro del oráculo de escenario y `pip check`.
- Entrada real en Xvfb privado, Vulkan software, 1280×720 y radio 450 m:
  curva original de **2000 m**, peralte observado **15 mm** en transición,
  confort **172,2 km/h**, vía/texturas originales, vistas de cabina/exterior,
  F8 Despachador, selector F10 y guardado del jugador/tráfico. RAM máxima
  **2767,2 MiB**. No es una medición de GPU ni de viaje completo.
- `check.sh`: **1482 pruebas Rust aprobadas**, 43 ignoradas, cero fallos;
  formato, clippy, compilación, Python, oráculos fijados y servicio sin ventana.
  Aceptación .NET adicional aprobada. Los 134 outputs se restauraron y sus
  hashes se verificaron, conservando los seis cambios previos del usuario.

Evidencia y capturas:
[fixtures/compatibility/next-three-2026-10-05.json](fixtures/compatibility/next-three-2026-10-05.json).
Pruebas manuales: [sección 30](PLAYER_MANUAL_TESTS.md#30-peralte-despachador-guardado-c-y-contenido-oficial).

## 5 de octubre de 2026: biblioteca persistente, ediciones e Include

- `./check.sh`: **1499 pruebas Rust pasaron, 43 ignoradas**; formato,
  Clippy, regresiones Python, build, oráculos fijados y servicio de estaciones.
  Se restauraron y verificaron SHA-256 de los 134 resultados preexistentes.
  Tras corregir la selección del nombre faltante para búsquedas, pasaron sus
  cinco regresiones y Clippy del workspace otra vez.
- 13 regresiones del instalador: actualizaciones con dos commits y ETag,
  conservación de ambas copias, consulta fresca del autor antes de reutilizar,
  cancelación, redirecciones HTTPS, extracción y metadatos inseguros.
  La consulta real de Chiltern resolvió `DocMartin7644/Chiltern-Route-v4`,
  commit `8236df20ed9f596b8cf15720c43bfbdd0c43125d`, fecha del repositorio
  `2026-09-09T15:48:33Z`. Es evidencia de la consulta, no una versión fijada.
- CLI copiada a una carpeta independiente: `content --list` devolvió las
  15 entradas, usando instalador/catálogo embebidos y Python 3, sin checkout.
  Pruebas de ubicación XDG, Windows, macOS, Snap común entre revisiones y
  copia de preferencias/partidas sin reemplazar datos del destino.
- Demo Model 1 conserva el ZIP oficial de 272422379 bytes, SHA-256
  `5e4a64e1cd44e23ee7833230348b6b76c43fc555060b152f6c6547e832a5189b`.
  Con Include expandido: **15/23 formaciones con recursos obligatorios
  válidos; 12/23 con tracción**. El player push-pull original tiene siete
  vehículos, **318800 kg y 139,9032 m**, con cabina 3D. Se advierte la
  referencia 2D ausente; se permite la alternativa 3D válida.
- Cabina original comprobada en Xvfb/Mesa lavapipe: **106 partes texturizadas,
  22 controles CVF y 62 enlaces de matrices**. W seleccionó adelante y D
  aumentó el regulador; el tren aceleró y la palanca/aguja de carga cambiaron.
  Cambiar a exterior mantuvo el escenario nativo. El techo y carteles de
  la estación pueden ocultar el tren según dónde se coloque la cámara.
  [Cabina al iniciar](fixtures/compatibility/content-library-2026-10-05/demo-original-cab.png),
  [tracción a 2,3 km/h](fixtures/compatibility/content-library-2026-10-05/demo-cab-traction.png)
  y [procedencia](fixtures/compatibility/content-library-2026-10-05/verification.json).
  La primera sesión usó `--route-root` relativo y detectó errores de caché
  de terreno en AssetServer. Se normalizó el argumento al iniciar la CLI,
  usando la misma resolución canónica que el menú. El inicio se repitió
  con el argumento relativo y cero errores de assets.

La actividad SCE probada no incluye paradas programadas y sigue mostrando
distancia al destino; esta prueba cubre inicio, cabina y controles, no un
servicio SCE completo ni paridad física. Las formaciones incompletas siguen
bloqueadas. La política actual de faltantes se describe en la sección siguiente;
el CAF sintético carece de un origen identificado para el modelo. Se probó la ubicación de datos de Snap, no un
paquete Snap instalado. Los recursos descargados se conservan fuera de Git.

Pruebas manuales: [sección 31](PLAYER_MANUAL_TESTS.md#31-actualizaciones-biblioteca-del-usuario-y-binarios).
Contenido: [OFFICIAL_CONTENT.md](OFFICIAL_CONTENT.md).


## 5 de octubre de 2026: faltantes y repositorio original

- Check final completo: **1504 pruebas Rust aprobadas, 43 ignoradas**; formato,
  Clippy, Python, build, oráculos fijados y servicio de estaciones. Se restauraron
  y verificaron los 134 resultados preexistentes, incluidos los seis del usuario.
- Seis pruebas de procedencia: la URL sale del catálogo integrado; se rechazan
  orígenes no registrados y nombres de paquete incoherentes. `revision.repository`
  y `download_url` no pueden desviar la búsqueda. Solo se envía el nombre del
  faltante y una formación externa no hereda el origen del escenario.
- Siete pruebas de auditoría y nueve de Include: todos los gráficos/texturas
  ausentes, destinos absolutos, alternativas de cabina opcionales, referencias
  anidadas, diferencias de mayúsculas y límites del contenido. Un Include ausente
  que escape directamente o por enlace no se ofrece como destino de instalación.
- Reauditoría: una prueba de biblioteca agrega y quita un modelo propio de ensayo;
  el estado se actualiza sin cambiar ruta, servicio, formación, recorrido ni hora.
- Xvfb privado, Mesa lavapipe y 1280×720: se seleccionó Mitre/CAF, se abrió el
  detalle, se pulsó **Reauditar esta formación** y se volvió a nueva partida.
  La ventana muestra `caf6000_motor.s`, su ENG y las dos ubicaciones admitidas.
  El modelo original sigue sin origen identificado y ausente; la formación
  permanece bloqueada y no ofrece descargas alternativas. No se descargó ni
  añadió un modelo sustituto para hacer pasar esta comprobación.

[Captura del diagnóstico](fixtures/compatibility/original-content-2026-10-05/caf-missing-files.png)
y [registro de verificación](fixtures/compatibility/original-content-2026-10-05/verification.json).
La prueba visual cubre el menú y la auditoría, no una partida completa ni la
paridad física/visual de un CAF original.

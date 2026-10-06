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

La composición geométrica y la partida completa están comprobadas. La validación histórica del 4 de octubre aún fallaba; la ampliación del 5 de
octubre, al final de este documento, registra el servicio completo dentro de
tolerancia con referencia y presupuestos intactos. El servicio extendido ahora usa el subconjunto SIGSCR original de
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


## 5 de octubre de 2026: KTX2, catálogo, terreno y paquetes

`./check.sh` pasó formato, Clippy, **1517 pruebas Rust y 44 ignoradas**, regresiones Python, dos pruebas nativas, build, oráculos cortos fijados y servicio corto. Tras completar la carga asíncrona, se repitieron Clippy/build y las ocho pruebas de assets, incluida la regresión nueva. Se restauraron y verificaron SHA-256 de los 134 resultados preexistentes, conservando los seis archivos del usuario.

### Escenario y texturas

Las tres estaciones de Chiltern se inspeccionaron en exterior y cabina a las 09:55, radio 450 m y FOV 45°, contra las referencias OR 1.6.1 existentes. RX 7600/Vulkan cargó las seis vistas sin pipelines fallidos/pendientes, uploads pendientes ni shapes cercanos sin activar. Picos RSS: 1296–1419 MiB exterior y 1574–1594 MiB cabina.

Northolt Park contra RGBA sin caché conservó recortes/transparencias: error RGB medio 0,0166/255 exterior y 0,0086/255 cabina, en el rectángulo de escena `[310,160,660,490]`. El pequeño plano claro de un abedul aparece en ambas variantes: sigue requiriendo revisión del material y no se considera corregido por la caché. Cuatro texturas sumaron 43,58 ms en frío y 17,06 ms en caliente, con igual payload 4,17 MiB; esto no mide una aceleración del arranque completo.

Demo Model 1 se comparó con capturas nuevas de OR 1.6.1, actividad 0930 Edinburgh–Glasgow y formación original Class 47 con seis Mk2. Se corrigieron tres causas:

- El inicio del PAT se trataba como cabeza, aunque OR lo usa para la cola; ahora el menú aplica los 139,9032 m de la formación seleccionada.
- Los buffers RAW pedidos en minúsculas existían con nombres en mayúsculas. Se resuelven sin renombrarlos, en lectura directa y en AssetServer.
- `Demo Model 1/ROUTES/SCE` no encontraba su propia carpeta `global/`. Ahora usa sus **314 modelos originales de vías/carreteras**, sin los 162 reemplazos generados de la primera captura.

Las capturas finales muestran suelo continuo y superficies originales de carreteras. Cero faltantes de terreno y errores de shaders. RSS: **744 MiB exterior y 837 MiB cabina**. La calibración exacta del centro del tren/cámara, iluminación, SIGSCR y la inversión del PAT siguen fuera de esta aceptación; estas vistas no son goldens de paridad por píxel.

[Exterior corregido](fixtures/compatibility/five-items-2026-10-05/openrailsrs-exterior.png), [OR 1.6.1 exterior](fixtures/compatibility/five-items-2026-10-05/or161-exterior.png), [cabina corregida](fixtures/compatibility/five-items-2026-10-05/openrailsrs-cab.png) y [OR 1.6.1 cabina](fixtures/compatibility/five-items-2026-10-05/or161-cab.png).

### Recorrido y distribución

El visor final completó **6/6 paradas y 15318,73 m** con streaming, 66 programas SIGSCR y ambos servicios AI llegados. Mayor error de detención: 4,03 m. Conductor automático al 75 %, tiempo ×16 y pasajeros normales: 2538,4 s simulados y 230,4 s reales. Llegó 706 s tarde a Gerrards Cross; completar el viaje no certifica horarios ni paridad física.

Pico RSS: **2064 MiB**. P50/P95/P99: 25/62/82 ms; 25 cuadros de más de 100 ms, el mayor 4525 ms al inicio, y 18 tirones durante la partida. Seis sectores GPU, 6450 entidades, cero errores SIGSCR/pipelines y cero uploads/shapes cercanos pendientes al final. No se afirma que hayan desaparecido los tirones.

El paquete portátil incluye 185 archivos de recursos/binarios y abrió una partida desde una carpeta vacía, usando ejemplos/shaders empaquetados. La CLI listó quince entradas; el menú mostró tildes y los nuevos botones. La partida usó RX 7600, pico RSS 1575 MiB y cero errores de shaders. La primera CLI bajo el sandbox no podía escribir en XDG; se repitió con datos de QA aislados y sin depender del checkout.

Se construyó el Snap real core24. El usuario pospuso la instalación; **confinamiento, portapapeles y persistencia en un Snap instalado siguen pendientes**. Belgrano CC espera la ruta y los trenes originales del autor. `--inspect` informa rutas absolutas y ubicaciones faltantes sin crear una ruta ficticia.

El catálogo web se probó en escritorio/móvil, sin errores JS ni desbordamiento. El juego abre la carpeta exacta y copia el diagnóstico; conserva ediciones y reauditoría. Solo se ofrecen orígenes originales y las descargas quedan fuera de Git.

La revisión de seguridad encontró un KTX2 RGB8 truncado que podía provocar un panic en Bevy. Se corrigieron tamaño/DFD con regresiones para RGB8 válido e inválido. El servicio falló después del hallazgo; **no se declara una revisión automática completa**.

[Servicio completado](fixtures/compatibility/five-items-2026-10-05/chiltern-completed.png) y [mediciones/procedencia](fixtures/compatibility/five-items-2026-10-05/verification.json). Pruebas manuales: [sección 32](PLAYER_MANUAL_TESTS.md#32-catálogo-web-biblioteca-ktx2-y-paquete-trasladable). Distribución: [DISTRIBUTION.md](DISTRIBUTION.md).

## 5 de octubre de 2026: fluidez, transparencias y cámaras

La comparación de streaming usa el mismo recorrido extendido de seis estaciones,
RX 7600/Vulkan, 1280×720, radio 450 m, FOV 60°, conductor al 75 %, tiempo ×16 y
pasajeros normales y la formación histórica del ejemplo. Ambas partidas
completaron **6/6 paradas y aproximadamente 15318,75 m**. El registro anterior
tuvo 18 cuadros de juego mayores a 100 ms, con máximo 345,34 ms. La prueba
final tuvo **uno**, de **150,33 ms**, y P50/P95/P99 de **25/25/48 ms**. Tiempo
real del viaje: 230,4 → 190,9 s; pico RSS: 2064 → 2022,4 MiB; pico de VRAM
del proceso: 2435,44 → 1651,46 MiB. El peor cuadro de arranque sigue siendo
de **4,29 s**; esta aceptación no afirma que el inicio sea fluido ni que se
cumpla el horario.

El registro anterior podía perder la cabina y otras mallas durante el viaje.
La prueba final conserva la cabina visible hasta Gerrards Cross y pasa cinco
de las seis regiones del control gráfico. Las cifras anteriores sirven como
registro histórico de la misma configuración; la carga visual no era equivalente.

La preparación de texturas y mallas del terreno se hace en trabajadores. Se
publican las mallas con un presupuesto suave de 4 ms por cuadro y con el origen
actual de la cámara. Se reutilizan handles ya subidos a GPU sin releer la textura.
Las alturas siguen disponibles en CPU para posicionar cámara y escenario. El
diagnóstico de rayos de cabina sólo recorre las mallas cuando se activa
explícitamente, y los mensajes normales se actualizan dos veces por segundo.

Las seis vistas de las tres estaciones cargaron sin texturas, terreno ni modelos
cercanos pendientes. El rectángulo claro señalado en las pruebas anteriores era
**OldOakTree.s/OldOaktree.ace**: su fondo blanco tiene alfa de aproximadamente
6 %. Se clasifica la transparencia del mip original, incluso al reutilizar KTX2.
Los atlas WORLD con alfa de fondo y follaje opaco usan el recorte nativo de OR;
los cristales con alfa intermedio conservan su mezcla. No se modifica el asset
descargado ni se aplica este recorte a la cabina.

Se corrigió el anclaje de cada coche: el viajero de simulación representa el
frente del tren y el origen del modelo está en el centro del vehículo. El primer
coche queda a media longitud detrás de la cabeza y los siguientes respetan sus
propias longitudes. La ocupación de vía usa esos mismos centros. El enfoque
especial de captura reproduce el centro de OR; el FOV 60° y la cámara exterior
habitual de Bevy se conservan en la partida.

`scripts/check_station_cameras.py` compara posición, dirección y proyección
contra las **seis referencias OR 1.6.1 sin modificarlas**. Todas pasan los límites
de 3 m, 1°, 0,05° de FOV y 0,001 de aspecto: máximo 1,72 m y 0,58°. La auditoría
de tres edificios nativos también pasa. Esto verifica geometría/materiales y
cámaras; no certifica igualdad de iluminación por píxel.

[Roble desde cabina](fixtures/compatibility/polish-2026-10-05/northolt-oak-cab.png),
[cámara exterior](fixtures/compatibility/polish-2026-10-05/northolt-camera-exterior.png),
[viaje con streaming](fixtures/compatibility/polish-2026-10-05/chiltern-streaming-completed.png)
y [mediciones y referencias](fixtures/compatibility/polish-2026-10-05/verification.json).

### Demo Model 1: servicio nativo completo

La actividad original `MT_MT_0930 Edinburgh-Glasgow Queen Street.act` contiene
una sección jugable Edinburgh Waverley → Haymarket → Linlithgow. Se importa su
horario de jugador, los pares de plataformas TDB, su PAT y la formación original
Class 47 + seis Mk2. La salida conserva la posición de la cola y suma los
139,9032 m de la formación elegida para situar la cabeza. Los coches conservan
sus pasajeros y tiempos originales: la parada final tiene **600 s** de intercambio;
el modo de práctica permite reducir esa espera a 5 s.

La ruta usa su velocidad de diseño original de **100 mph**, las ramas reales
de los `TrPin`, la identidad de cada vector paralelo y las agujas que atraviesa
el PAT. Las señales ya no se fuerzan a verde: se importaron y evaluaron **678
cabezas SIGSCR**, sin errores. Las funciones NORMAL, DISTANCE, INFO, REPEATER y
SHUNTING conservan sus aspectos; los indicadores auxiliares no crean autoridad
de circulación. Un UID que aparecía como primer campo WORLD ya no mezcla las
características de dos postes distintos.

La prueba sin ventana terminó con **3/3 paradas, 28075,40 m y 2360,95 s**
simulados, sin fallo. El visor Vulkan/RX 7600 repitió la partida en **178,8 s
reales**, con **1023,1 MiB** de pico RSS, cuatro sectores GPU y cero terreno o
uploads pendientes al terminar. Llegó tarde a Haymarket y Linlithgow: completar
la actividad valida su continuidad y sus paradas, no puntualidad ni equivalencia
física de este Class 47 con OR. Los cuadros de partida P50/P95/P99 fueron
25/25/25 ms, sin tirones mayores a 100 ms; arranque máximo de 1,37 s. Dos
sectores periféricos SCE carecen de fuente de terreno en el paquete original;
esta prueba no certifica cobertura de todas las esquinas del mapa.

[Cabina al terminar en Linlithgow](fixtures/compatibility/polish-2026-10-05/demo-model-1-completed.png).

### Cabina y escenario después de varios kilómetros

La prueba de viaje detectó un error que los contadores de recursos no mostraban:
las mallas normales de Bevy podían desaparecer mientras los objetos WORLD
instanciados seguían dibujándose. La cámara permanecía dentro de la cabina y
los assets estaban listos. Los dibujos WORLD usan sus propias matrices y
buffers de instancias; ahora se registran como `NonMesh` tanto en la escena
como en las sombras, evitando entrar en el procesamiento de mallas ordinarias.
La cámara se crea con `NoIndirectDrawing`, como requiere el
[ejemplo oficial de instancias personalizadas de Bevy 0.19.1](https://github.com/bevyengine/bevy/blob/v0.19.1/examples/shader_advanced/custom_shader_instancing.rs).
Se conservan el renderizado, las instancias WORLD y la preparación de matrices
en GPU; la selección visible y el envío de dibujos ordinarios usan CPU.
El informe de cámara registra `indirect_drawing: false`. Esta configuración
evita la desaparición de mallas y no se presenta como ejecución íntegra en GPU.

La captura registra la posición mundial real de la cámara, su posición relativa
a la cabina y las piezas visibles. La regresión gráfica compara seis zonas
opacas de la cabina Pullman, con asiento, luz diurna y FOV 60° fijos. Exige
cuatro zonas dentro de un error RGB medio de 30/255; permite variaciones de
iluminación y excluye parabrisas, instrumentos y resumen del servicio. Rechaza
las capturas del fallo aunque sus contadores de visibilidad sean correctos.
La referencia es interna del visor y no certifica paridad por píxel con OR.
El presupuesto se fijó usando la cabina inspeccionada al inicio y al final del
viaje, y rechaza las capturas anteriores sin cabina. La referencia inicial,
las seis zonas y todos los presupuestos de paridad con OR permanecen intactos.

```bash
python3 scripts/check_viewer_streaming.py \
  --route-root "$CHILTERN_ROUTE" \
  --scenario /ruta/al/escenario-con-formacion-original.toml \
  --checkpoint terminal --headless-wayland --require-hardware \
  --view-radius-m 450 --timeout-s 420 \
  --pullman-cab-reference docs/fixtures/compatibility/polish-2026-10-05/pullman-cab-foreground-reference.png \
  --out-dir tmp/chiltern-native-streaming
```

El escenario de esta prueba conserva las seis estaciones de
`examples/chiltern_extended/scenario.toml` y usa el `Birmingham Pullman.con`
original instalado. Las rutas absolutas locales y las descargas no se versionan.

### Física del servicio completo dentro de tolerancia

El 5 de octubre se reprodujo la captura nativa de las tres estaciones con la
misma versión OR 1.6.1 y la misma referencia SHA-256. Resultado: RMS 0,2081 m/s,
pico 1,0921 m/s y diferencia máxima de odómetro 39,61 m; cobertura del 100 %.
Las cinco fases cumplen RMS ≤1,10 m/s. Se conservan los umbrales de 0,75 m/s,
2 m/s y 45 m. No se reemplazó ni ajustó ningún baseline aceptado.

El replay adopta el corredor con pendientes por sección y la formación nativa
extraída solo como datos numéricos. Corrige ton UK, motor auxiliar con su propio
gobernador, descenso de RPM original, regulación por escalones, corte de potencia
hasta liberar la presión EP, carrera del pistón, zapatas P10, resistencia térmica,
patinaje Pacha y conexiones rígidas. El entorno original pausado queda identificado
en `examples/chiltern_local/physics/environment.json`; la media del ruido de
adherencia y los subpasos deterministas dejan diferencias pequeñas, cuantificadas
por el oráculo. Este pase certifica el ensayo fijo, no todos los climas ni trenes.

`check.sh` exige también `run_oracles.py --suite service`. El informe reproducible
se guarda en `tmp/service-parity-check/report.json`. Los gráficos descargados
permanecen fuera del repositorio; la biblioteca continúa usando el origen del autor.


Verificación final del 5 de octubre: `check.sh` pasó **1547 pruebas Rust**,
**49 pruebas Python** y **dos pruebas nativas adicionales** con el contenido
Chiltern instalado. Quedaron 44 pruebas que requieren recursos específicos
sin ejecutar en la batería habitual. Pasaron también la preparación del menú
nativo de Demo Model 1, las seis cámaras y la regeneración byte a byte de los
diez fixtures físicos. Los cuatro oráculos cortos y el servicio completo usan
las referencias y tolerancias originales.

El guardado JSON conserva exactamente los números de coma flotante. Las
pruebas de física nativa comprueban guardar/reanudar sin divergencia, rechazo
de estado corrupto antes de modificar la partida y temperatura de los coches
retenidos al desacoplar. El modo de cuerpo único conserva resistencias y
pendientes por coche; una formación rígida produce el mismo resultado con
ambos modos. Una actividad que referencia señales como plataformas se rechaza
con un diagnóstico de edición incompatible; no se inventa una parada.

Se restauraron y verificaron los 134 outputs de simulación existentes,
preservando los seis archivos que el usuario ya había cambiado. Los contenidos
descargados, logs, cachés y ensayos intermedios continúan fuera de Git.


El último viaje gráfico usó la **formación original completa** del Pullman,
las seis estaciones y los dos servicios AI: **6/6 paradas, 15320,22 m**,
**199,1 s reales** y **2027,5 MiB** de pico RSS. Los dos servicios AI llegaron,
66 cabezas SIGSCR sin errores, cero terreno o subidas GPU pendientes. Las
velocidades de llegada fueron menores a 0,1 m/s. Captura:
[servicio con formación original](fixtures/compatibility/polish-2026-10-05/chiltern-original-service-completed.png).

Esta carga nativa adicional tuvo **un cuadro de juego mayor a 100 ms**,
con máximo **148,3 ms** y P99 **48 ms**; el arranque máximo fue **4373,0 ms**.
La cabina permaneció visible y pasó cinco de las seis regiones del control
gráfico, con la cámara real dentro de su geometría y cero errores de pipelines.
La formación histórica también tuvo un tirón en su prueba final; estos resultados
no garantizan fluidez para todos los modos y contenidos. El horario sintético de
seis estaciones sigue siendo exigente para el conductor automático nativo:
completar la ruta no certifica puntualidad.

El viaje completo en **exterior** terminó también 6/6, con ocho modelos de
vehículo compartidos, ambos servicios AI llegados y ningún recurso pendiente.
RX 7600/Vulkan: **195,1 s reales**, **1717,9 MiB** de RAM, P50/P95/P99 de juego
**25/25/48 ms**, un cuadro mayor a 100 ms (máximo **122,3 ms**) y arranque
máximo de **1513,0 ms**. Se inspeccionaron el tren, la vía y el entorno al final:
[formación original en exterior](fixtures/compatibility/polish-2026-10-05/chiltern-original-exterior-completed.png).


## 5 de octubre de 2026: cabina asíncrona, seis estaciones y Class 47

Esta revisión reemplaza las cifras anteriores de arranque y horario de Chiltern.
Se conserva el mismo Pullman original, 1280×720, FOV 60°, radio 450 m,
RX 7600/Vulkan y conductor 75 % a tiempo ×16. La lectura CVF/shape/ACE y su
preparación se hacen en un trabajador; las texturas únicas y los instrumentos
se reutilizan al regresar de exterior. Los diagnósticos detallados sólo recorren
las mallas cuando se activan. La carga espera también la cabina.

El peor cuadro inicial pasa de **4372,98 a 1477,56 ms**. La preparación de cabina
registró 633,5 ms en el trabajador y 493,4 ms al publicar recursos; esta última
fase todavía se puede mejorar. En partida: **25/25/51 ms** P50/P95/P99, **dos**
cuadros mayores a 100 ms y máximo **122,52 ms**. Pico RSS **2089,34 MiB** y
VRAM del proceso **1644,26 MiB**. Terminaron **6/6 paradas, 15320,21 m**,
ambos servicios de tráfico y cero shaders, uploads, terreno o modelos cercanos
pendientes. El control de cabina visible se conserva durante todo el viaje.

Se corrigió la precaución de señales: una señal ya pasada o una amarilla después
de una verde cercana no reduce el límite de todo el vector de vía. Se verificó
con la DLL original el límite de 15 mph y se conservó. El horario propio de la
extensión ajusta las dos últimas llegadas a 10:21:30 y 10:29:30, con salidas a
10:22 y 10:30. En la prueba gráfica todas las llegadas quedaron a tiempo y no
hubo salidas anticipadas. La simulación sin ventana repite 2101,05 s y seis
paradas. El menor tiempo real total también depende del horario revisado;
no se presenta como una mejora de FPS.

Las doce cámaras de las seis estaciones pasan contra referencias OR 1.6.1
intactas: máximo **1,7203 m / 0,6541°**, con presupuestos 3 m / 1° y proyección
sin cambios. Se inspeccionaron Denham, Denham Golf Course y Gerrards Cross en
cabina y exterior. El cerco blanco de Golf Course, edificios, andenes, puente
y árboles quedan presentes y anclados. Se mantienen diferencias de iluminación
y detalle distante; esta aceptación no certifica igualdad por píxel.

La Class 47 + seis Mk2 tiene una nueva captura de 250 s realizada con la DLL
original, formación/actividad/controles fijados y referencias SHA-256. Pasa
**RMS 0,2382 m/s**, pico **1,9415 m/s**, posición **25,79 m**, cobertura 100 %,
con presupuestos originales 0,75 m/s / 2 m/s / 45 m. Se leen parámetros Include
con precedencia de último valor y bloques de alimentación parciales. El motor
conserva 450 RPM para alimentación y el freno normal usa cilindro, distribuidor,
relación de presiones, fricción de zapata y deslizamiento del modelo nativo.
La liberación llega a 5 bar de tubería; servicio completo, a 3,5 bar de tubería
y aproximadamente 4,83 bar de cilindro. Depósitos, emergencia y otros sistemas
siguen parciales. El oráculo Pullman anterior y sus tolerancias permanecen intactos.

La cabina original Class 47 se capturó de nuevo en OR y se contrastó con Bevy:
**0,9969 m / 0,0900°**, 106 piezas visibles y shaders completos. Las pruebas
SMS/WAV de 12 s, cabina y exterior, resolvieron 8 programas, 38 streams y 38 WAV;
las mezclas son distintas y tienen señal de audio. Se informan 21 referencias
WAV ausentes en el paquete original. No se descargan sustitutos ni se certifica
paridad del mezclador con OR.

El paquete Linux incluye `Jugar.sh --check`, menú con `Jugar.sh`, selección
CPU/GPU/Auto, `LEEME.txt`, commit y hashes en `BUILD.json`. Los recursos propios
están incluidos; descargas y datos del jugador quedan fuera. La comprobación de
dependencias y catálogo pasa desde una carpeta con espacios. La compilación de
QA requiere **glibc 2.43**, verificada en ELF; no se anuncia compatibilidad con
distribuciones anteriores. El Snap construido conserva su prueba de instalación
pendiente en #189.

[Capturas y mediciones de esta revisión](fixtures/compatibility/journey-release-2026-10-05/README.md).
[Pasos de prueba manual, sección 34](PLAYER_MANUAL_TESTS.md#34-probar-las-cinco-mejoras-del-recorrido-y-distribución).

El chequeo final completo pasa formato, Clippy sin avisos, compilación, **1553
pruebas Rust ejecutadas** (incluidas dos de geometría/movimiento con ruta nativa),
**55 Python**, tres composiciones de edificios originales y los seis oráculos
físicos. La primera pasada conserva 44 pruebas ignoradas por sus dependencias
específicas; las dos pruebas nativas indicadas se ejecutan después explícitamente.
El paquete extraído también pasa simulación y menú con RX 7600 desde una carpeta
con espacios, sin Cargo/Rust ni shaders externos al paquete.


La repetición final de Demo Model 1, con Class 47 original y frenos actualizados,
completa **3/3 paradas, 28075,79 m y 2090,25 s** simulados. La cabina sigue
visible en Linlithgow, con tubería 3,5 bar y cilindro 4,83 bar al frenar. La
partida gráfica tarda 161 s reales; P50/P95/P99 **25/25/25 ms**, máximo de
partida **28,95 ms**, **cero** cuadros mayores a 100 ms, arranque **441,10 ms**,
pico RSS **1014,48 MiB** y VRAM **809,66 MiB**. Quedan cero recursos o shaders
pendientes y se evaluaron 678 cabezas nativas sin errores.

El horario original de esta actividad se conserva: el conductor al 75 % llega
**105,15 s tarde a Haymarket y 108,30 s a Linlithgow**. No se presenta este viaje
como puntual ni como el oráculo físico completo de la Class 47. La espera final
mantiene los 600 s originales; práctica permite reducirla. La simulación sin
ventana repite exactamente tiempos, paradas y distancia del visor.

La regresión visual por software mantiene el golden: 12/230400 píxeles fuera
de tolerancia (0,005 %), RMSE 0,486; es una prueba de dibujo, no una medición
de rendimiento de GPU.
Las tres siluetas WORLD individuales e instanciadas coinciden **1,0/1,0/1,0**
frente al mínimo fijo 0,98. Las capturas por software se ejecutaron por separado
del viaje medido en GPU.

## Menú: volver de Mitre a Chiltern

La selección ejecutaba la auditoría de todas las formaciones en el hilo de Bevy
y descartaba sus resultados al cambiar de ruta. Con el Chiltern original instalado
y 185 formaciones, tres vueltas desde Mitre tardaron **8340,09 / 8411,93 / 8529,46 ms**.
Con la revisión en un único trabajador y caché por paquete, las mismas vueltas
tardaron **2,90 / 2,20 / 2,15 ms**. Es el tiempo de `cycle_route` con los diagnósticos
ya disponibles; no mide cuadros del renderizador ni la carga de una partida.

Las regresiones comprueban navegación y consultas mientras el lector está
bloqueado, descarte de resultados anteriores, separación de paquetes, cancelación
al cerrar el menú, actualización de recursos y bloqueo de inicio mientras falta
el diagnóstico seleccionado. El chequeo completo pasa **1612 pruebas Rust** y
**55 Python**; el ensayo nativo de tiempos se ejecutó además explícitamente.

El menú actualizado se capturó a 1280×720 con RX 7600/Vulkan en Weston privado:
**408,91 MiB** de pico RSS, cero shaders pendientes o fallidos y salida correcta.
Se revisaron la formación elegida, sus avisos y «Revisar archivos otra vez».
Pasos para repetirlo en la [sección 39 de pruebas manuales](PLAYER_MANUAL_TESTS.md#39-cambiar-de-ruta-sin-congelar-el-menú).

```bash
OPENRAILSRS_NATIVE_ROUTE="/ruta/a/ROUTES/Chiltern" \
CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG=0 \
CARGO_PROFILE_TEST_STRIP=symbols CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 \
cargo test --locked --workspace --all-features native_menu_route_switch_timing \
  -- --ignored --nocapture --test-threads=1
```

## Inicio: cuatro opciones y preparación del viaje

El inicio ofrece Nueva partida, Continuar, Biblioteca y Ajustes. Nueva partida
se divide en Ruta y servicio, Tren y Hora y clima; resumen y Jugar son hermanos
del área desplazable. El diagnóstico completo se abre desde Ver detalles.
Biblioteca separa Instalado de Descargas del autor, y Ajustes usa cinco pestañas
con Guardar ajustes fuera del desplazamiento.

El chequeo completo pasa **1618 pruebas Rust** (45 ignoradas) y **55 Python**,
incluidos los oráculos originales, el servicio completo y Class 47. Después de
la corrección de medidas del texto también pasan las **23 pruebas de jugador**,
Clippy con `-D warnings` y la compilación de todas las funciones del workspace.
Las regresiones nuevas comprueban regreso desde el detalle sin perder la
selección, bloqueo de inicio con recursos incompletos, ubicación de las acciones,
selección desde Biblioteca, regreso de Ajustes a Pausa y conservación del viaje
cuando se actualiza el catálogo instalado.

Se revisaron **13 capturas** en RX 7600/Vulkan, siempre en Weston privado:
las diez vistas de inicio a **1280×720 / 100%**, dos vistas de Nueva partida a
**150%**, y el inicio a **1024×640 / 100%**. Los textos tienen medidas positivas,
las cuatro opciones del inicio entran completas, y Jugar, resumen y Guardar
ajustes quedan dentro del panel. Las páginas largas se desplazan dentro del
área central. Cero shaders pendientes o fallidos y salida correcta en todos los
casos; máximo RSS medido del menú: **421,07 MiB**. Este valor no mide la memoria
de una partida con el escenario cargado.

Se repitió el ensayo de regreso Mitre → Chiltern con 185 formaciones:
**2,94 / 2,58 / 2,40 ms** para actualizar la selección con diagnósticos en caché.
Los archivos de salida existentes y las tolerancias de paridad se conservaron.

El script valida también las medidas de los textos, para detectar botones que
tienen un rectángulo visible pero una etiqueta sin ancho. Permite repetir las
capturas sin abrir otra ventana en el escritorio del jugador:

```bash
python3 scripts/capture_player_menu.py \
  --route-root "/ruta/a/ROUTES/Chiltern" --out-dir tmp/player-menu-qa
python3 scripts/capture_player_menu.py \
  --route-root "/ruta/a/ROUTES/Chiltern" \
  --pages route weather-options --scale 1.5 --out-dir tmp/player-menu-qa-large
```

Prueba de cada opción y resultado esperado en la
[sección 40 de pruebas manuales](PLAYER_MANUAL_TESTS.md#40-inicio-con-cuatro-opciones-y-nueva-partida-en-tres-pasos).

## Chiltern v4: instalación externa y actividades nativas

Se descargó el repositorio original de DocMartin, revisión
`8236df20ed9f596b8cf15720c43bfbdd0c43125d`, con fecha de origen
**2026-09-09**. El ZIP ocupa **7,31 GiB** y los originales **8,51 GiB**.
La copia está en los datos del usuario, fuera del checkout; la instalación
anterior se conserva. El catálogo del juego y de la web ofrece una sola descarga
actual, porque los enlaces del autor para v2 y v3 redirigen a v4. La biblioteca
identifica cada copia por versión conocida, fecha y revisión.

La auditoría encontró **155 de 188 formaciones** con recursos completos y
tracción, y **34 archivos de actividad**. No es una certificación de sus sistemas
ni de todas las actividades. Se compilaron sin errores los **143 programas
SIGSCR usados** por esta edición. La importación conserva los identificadores
con `/`, los índices estándar de vía y las constantes `SIGFEAT_*` de OR 1.6.1.
El TDB tiene su propio límite de 64 MiB; scripts y WORLD conservan 16 MiB.
También se corrigió la inserción de alimentación eléctrica en un TOML nuevo.

El PAT ahora sigue `nextMainNode` desde `TrPathNode[0]`, en vez de recorrer la
tabla de puntos sin orden. Las regresiones comprueban índices inválidos,
ciclos y exclusión de una vía alternativa. La ubicación inicial y el destino
usan los puntos nativos de las vías curvas, con coordenadas `f64`; los cambios
se alinean con los puntos del recorrido. Esto evita elegir un andén vecino o
un desvío más corto que el trazado del autor. Los escenarios TOML calibrados y
las tolerancias de los oráculos se conservaron.

Se preparó desde el menú **RS_Football Special**, con su formación original de
**10 vehículos, 181 m, vapor y cabina 2D**. Conserva las cuatro plataformas:
Banbury General, Bicester North, Princes Risborough y High Wycombe. La
actividad **RS_Let's go to Birmingham** incluida en esta revisión usa el ítem
11358 como plataforma, pero el TDB lo define como señal: se rechaza, sin
inventar una parada alternativa.

Se revisaron cinco capturas a **1280×720**, con **RX 7600/Vulkan**, en Weston
privado y con un límite RSS de 6144 MiB: exterior de Banbury, puesto de
conducción, cabina 2D y dos vistas tras avanzar **2 km reales** con conducción
automática de prueba. El tren completó la parada inicial y se cargaron nuevos
sectores al salir de Banbury. Las instantáneas tienen cero shaders pendientes
o fallidos, cero errores de señales y cero cargas GPU/terreno pendientes.
El máximo RSS medido fue **1484,8 MiB**; VRAM del proceso, **1053,6 MiB**.
Estas cifras corresponden a esas escenas, con radio de vista de 450 m, y no
al viaje completo. En la segunda vista en movimiento hubo dos cuadros de más
de 100 ms; el mayor fue de 120,8 ms. La cabina original cargó 19 controles,
con 12 widgets representados y cinco sin representación; el vapor conserva
las limitaciones que informa la auditoría.

`check.sh` completo pasa **1626 pruebas Rust** (45 ignoradas) y **59 Python**,
además de formato, Clippy con `-D warnings`, compilación, sitio generado,
oráculos de OR 1.6.1, Class 47 y el servicio completo del piloto. Esta prueba
de v4 comprueba importación, inicio y avance; no mide todavía paridad física
o visual de su viaje completo. Las salidas que ya tenía el usuario y su
configuración de lanzamiento se restauraron tras las pruebas.

Pasos y resultado esperado en la
[sección 41 de pruebas manuales](PLAYER_MANUAL_TESTS.md#41-chiltern-v4-descarga-del-autor-y-copias-separadas).

## Pullman: contraste del salón y grúas de Banbury — 2026-10-06

Se reprodujeron las capturas del usuario con **Birmingham Pullman**, actividad
**RS_Football Special**, Chiltern v4 `8236df20`, Banbury General a 101 m del
punto de parada y tiempo despejado. El salón mostraba el mismo velo gris:
el relleno de los materiales HalfBright sumaba un color constante. Ahora el
relleno usa la textura original como mapa de emisión, también en la cabina
PBR cuando corresponde, y conserva los negros y el detalle de la tapicería.
Los cristales de cabina siguen excluidos del relleno.

El poste que oscilaba es **pbwatercrane1.s**, un `Pickup` de agua. Sus dos
cuadros a 30 cuadros/s se reproducían como un bucle de escenario, unas quince
veces por segundo incluso con la partida pausada. OR 1.6.1 lo controla desde
`FuelPickupItemShape.PrepareFrame` según la operación de abastecimiento.
El visor conserva su posición de reposo y solo reproduce bucles de objetos
Static con `StaticFlags.Animate`. Las señales conservan su control por aspecto.
El flag se preserva tanto en WORLD de texto como en el puente de WORLD binario.

Evidencia local, sin versionar contenido descargado:

- `tmp/pullman-banbury-20261006/before/passenger.png`: reproducción anterior.
- `tmp/pullman-banbury-20261006/after/passenger.png` y `exterior.png`: GPU Vulkan,
  Radeon RX 7600, cero pipelines pendientes/fallidos y cero errores de señales.
- En ambas vistas, las últimas treinta muestras de las transformaciones de
  las grúas fueron idénticas, durante más de diez segundos; no hay grúas en la
  consulta de animación continua. Se observaron tanto partes PBR como instancias.
- Pico RSS del visor: **1410,9 MiB**; pico VRAM del proceso: **1057,2 MiB**.
  Son mediciones de estas escenas, con radio de 450 m.
- `check.sh` completo: **1630 pruebas Rust** y **59 pruebas Python** aprobadas,
  formato, Clippy estricto, build, oráculos de aceptación y servicio de tres
  estaciones. Las 45 pruebas Rust marcadas como ignoradas requieren recursos
  o ejecuciones específicas y no se cuentan como aprobadas.

Prueba manual en la
[sección 42](PLAYER_MANUAL_TESTS.md#42-salón-del-pullman-y-grúas-de-agua-en-banbury).
El abastecimiento por operación sigue pendiente; esta corrección valida el
reposo del objeto y el contraste, sin alterar archivos originales ni oráculos.

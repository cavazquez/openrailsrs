# Validación visual del quinto lote: 40 issues

Esta guía cubre #243, #244, #245, #253, #254, #265, #273, #276, #277,
#278, #282, #283, #284, #285, #291, #296 y #297. Los modelos del laboratorio
son propios y usan colores de diagnóstico. Chiltern conserva los archivos del
autor: no se copian al repositorio ni se modifican para obtener una captura.

El [informe público de aceptación](validation/FIFTH40_VISUAL_ACCEPTANCE.json)
identifica los casos, sus métricas y los hashes exactos de binarios, shaders y
recursos propios. Los conjuntos de hashes se comparten entre casos para evitar
duplicaciones; no contiene rutas del equipo ni archivos descargados del autor.

## Preparación

Compilar el visor con `dev-tools` permite ejecutar las secuencias reproducibles.
El juego habitual no ejecuta acciones de captura. Para pruebas manuales, elegir
un laboratorio de `examples/` desde el menú o iniciar el visor con su escenario.
La biblioteca debe auditar recursos completos antes de iniciar Chiltern.

```sh
cargo build --workspace --all-features --locked
python3 scripts/check_player_fifth_batch.py \
  --viewer target/debug/openrailsrs-viewer3d \
  --out-dir tmp/fifth40-visual
```

Las capturas usan un compositor privado, una GPU real y archivos de jugador
aislados. Cada caso registra el hash del binario, de los shaders y de los recursos
propios, acciones aplicadas, cuadros, memoria y estado. Un error de shader,
escenario incompleto, viaje que no alcanzó su posición o acción rechazada hace
fallar el caso. No se acepta un error de extremos igual a cero si no hay ninguna
pieza conectada y deformada.

## Uniones y articulación: #243, #244, #245 y #253

Abrir `examples/player_lab/scenario_connections_curve.toml`. La ruta propia
dibuja una curva de 180 m de radio mediante segmentos cortos. Con `2` y el mouse,
acercarse a la unión entre los dos coches; avanzar despacio y mirar el fuelle,
manguera, cable y acoplador desde ambos lados.

Los extremos deben seguir sus coches mientras cambia el ángulo de la curva.
La malla conserva su textura y detalle original. Sólo se deforman matrices
explícitamente declaradas; no se inventa un fuelle para una formación que carece
de esa geometría. Una unión que excede el alcance declarado se oculta y deja un
diagnóstico, en lugar de estirarse indefinidamente.

En el laboratorio recto, aplicar el freno de mano del segundo coche desde `F9`,
desacoplar después del primero y guardar. Al restaurar, las piezas que cruzaban
esa separación deben seguir desconectadas. Mangueras y cables también respetan
sus conexiones neumáticas y del bus: no basta con que dos coches estén cerca.
La prueba de LOD cambia a una malla con otro número de vértices y comprueba que
ambas mallas originales siguen intactas.

Repetir con `examples/player_lab/scenario_jacobs_curve.toml`. Los dos cuerpos
comparten el apoyo declarado y el bogie sigue ese apoyo. La articulación ideal
conserva el momento longitudinal del grupo y no permite cortar esa unión como
un acoplador convencional. Esto no certifica fuerzas laterales, rueda/pestaña
ni todos los vehículos articulados de la comunidad; sus modelos requieren sus
propias declaraciones y ensayos.

El ensayo físico `articulated_train` conduce, frena, restaura y retrocede en ese
itinerario. Comprueba velocidades y desplazamientos relativos compartidos con
tolerancia de 1e-10, además de la prohibición de cortar el apoyo declarado. Se
complementa con pruebas de proyección de restricciones y conservación de momento;
la captura en curva comprueba que el bogie y las conexiones se representan.

## Puertas, pasajeros y recorridos de andén: #254 y #284

Los casos `boarding`, `alighting`, `boarding-restore`, `boarding-rebase` y
`boarding-close` crean una parada propia con lado izquierdo declarado. El
embarque usa sólo puertas
completamente abiertas de ese lado. Las personas esperan fuera del gálibo del
coche; al bajar se alejan de la puerta, y al subir entran por ese acceso. Las
personas ya embarcadas dejan de representarse fuera.

En una partida manual con puertas declaradas, detenerse en el punto de parada,
abrir el lado habilitado y observar el andén con `2`. Pausar con `P` durante el
embarque, guardar y restaurar. El reloj, las fracciones de puertas y el progreso
no deben avanzar mientras está pausado. Bajar y elevar la cámara o desplazarse
lejos y volver no debe cambiar el lado ni la posición de la fila.

Pedir el cierre mientras una persona cruza la puerta deja terminar únicamente
esa transferencia, de hasta 1,5 segundos, antes de comenzar el movimiento de la
hoja. No exige completar toda la carga de pasajeros. La orden pendiente se
conserva al guardar y restaurar; volver a abrir la cancela. Si todos están
esperando todavía, el cierre comienza inmediatamente.

La captura mixta final con el binario 17 registra cuatro bajados y doce subidos,
con 22 figuras visibles y ningún corredor rechazado. Los grupos se intercalan
proporcionalmente: no salen todos los pasajeros de bajada al inicio mientras el
contador del servicio apenas ha avanzado. El caso automático comprueba esa
relación, además de inspeccionar el andén desde el lado habilitado.

Se representan hasta 64 figuras por escena. La cantidad visible agrupa
pasajeros cuando la demanda es mayor; el contador del servicio conserva las
cantidades originales. Antes de aceptar una ruta entre espera y puerta se
comprueba el corredor completo contra objetos opacos. Los cuerpos y cabezas de
las personas se excluyen de esa lista para evitar que se bloqueen a sí mismas.
No hay navegación inventada a través de estaciones sin accesos declarados.

## Espejos: #265

En `examples/player_lab/scenario.toml`, entrar en cabina con `1`. Hay dos espejos
propios; deben mostrar el costado y la parte posterior correspondientes, con
inversión horizontal de espejo y sin copiar HUD o menús. Volver a `2` los oculta.

Repetir con lluvia, mover el tren y pausar. El contenido del espejo conserva
niebla, exposición y luz de la cámara principal. Su imagen tiene 256 × 160
píxeles y se actualiza como máximo diez veces por segundo; esto es deliberado.
Bajo presión de memoria gráfica se desactivan. Cambiar repetidamente de cabina
a exterior no debe provocar un fallo de cascadas ni dejar cámaras adicionales
activas. La activación ocurre antes de que Bevy prepare las cascadas de sombras.

## Horizonte y sombras: #273 y #276

En Chiltern, comparar Northolt Park, South Ruislip y Gerrards Cross desde `1` y
`2`, con la misma fecha, hora y clima. Alejar y elevar la cámara y cambiar la
distancia de visión desde `F10`; después avanzar y volver a la cabina.

El terreno lejano usa las alturas y huecos originales, con el mismo datum
vertical que el terreno detallado. Cuando llega el sector cercano se oculta su
versión gruesa. Hay hasta 64 sectores lejanos, o 25 bajo presión gráfica; no se
mantiene una segunda copia ilimitada del mapa. El color lejano procede de una
muestra de la textura del suelo del autor.

El cielo y la cámara no son superficies que proyecten sombra. Las sombras del
tren y de objetos instanciados deben continuar al cambiar de cámara o de sector.
Comparar contacto desactivado/alto con el mismo encuadre. La prueba nativa incluye
una vista baja junto a la vía y otra del horizonte, con los ajustes de iluminación
registrados. El caso histórico `bridge-low` no muestra un puente sobre la cámara;
no se cuenta como prueba de sombras bajo un puente.
Esto valida continuidad y composición en Bevy; no exige igualdad de píxeles con
el renderizador de Open Rails.

## Cristales y mojado: #277 y #278

Desde cabina y exterior, mirar a través de varias ventanas, coches y vegetación.
En `F10` alternar «Mejorar cristales superpuestos». La opción ordena fragmentos
transparentes sin convertir un material con recorte alpha en vidrio. Las ramas
con recorte, textos y paneles opacos conservan su profundidad original.

Repetir con lluvia y de noche con faros. Los materiales compatibles adquieren
una película reflectante, con brillo limitado, manteniendo las texturas del
autor. Al volver a seco recuperan sus parámetros de origen; no se cargan imágenes
nuevas por cada transición.

El almacenamiento de fragmentos de transparencia se limita a 64 MiB. Se reduce
según resolución y se desactiva bajo presión de memoria. Los shaders nativos de
cabina y escenario participan en la misma resolución OIT que los materiales de
Bevy; omitir ese paso haría opacos los cristales en un pipeline sin blending.

## Cruces y tránsito: #282 y #283

En una ruta con `LevelCr`, `CarSpawner`, TDB y RDB originales, observar un cruce
con `2`. Antes de que la formación llegue, la carretera debe quedar protegida;
las barreras usan los parámetros y animación originales. Deben permanecer
cerradas hasta que salga la cola completa, también si pasa otro servicio o queda
un corte estacionado sobre el cruce. Guardar durante apertura y restaurar debe
conservar su fracción de apertura. Pausar no mueve ni barreras ni autos.

Los autos siguen la geometría RDB, incluidos varios vectores conectados. La
primera fila se detiene antes de la barrera y las siguientes conservan separación.
Al abrir deben continuar. Cada spawner utiliza un grupo acotado; dos autos que
llegan al final juntos no pueden reaparecer en el mismo punto de entrada.

Los enlaces TDB y puntos RDB se resuelven una vez por cruce cargado, también
cuando falta un enlace. La ocupación sigue actualizándose cada cuadro. Al
descargar sectores se libera la caché; una unión que amplía el grafo permite
reintentar los enlaces, y restaurar una partida anterior al sector recupera su
definición sin repetir búsquedas en cada cuadro.

La prueba propia comprueba una carretera en ángulo, una cola que espera y luego
se libera, pausa y múltiples cambios de origen. La prueba del RDB original se
activa con `CHILTERN_ROUTE` y comprueba los extremos 753/754 publicados por el
autor. No se aceptan geometrías desconectadas como si fueran una recta válida.

El modelo conserva protección conservadora por ocupación y aproximación. Las
reglas particulares de Open Rails para trenes estáticos y aproximación muy lenta
no equivalen a todas las normativas regionales; no se certifican cruces de
cualquier contenido ni se inventan enlaces ausentes del TDB/RDB.

La regresión gráfica nativa usa Park Royal en el sector `-6083:14926`
(UID 12473/12474), con las barreras WORLD y los spawners RDB del autor. Los
ítems de carretera 813/814 pertenecen a los vectores de sus spawners 811/812
y 107/108. Conserva todo el grafo TDB original y elige un itinerario continuo
por `e1994`; no cambia sus perfiles ni copia texturas o modelos.
Una toma deja la formación sobre el cruce y otra conduce hasta despejarlo con
toda su cola. El informe incluye estado de barreras, puntos de espera y poses de
hasta 128 autos, para comprobar que la imagen corresponde a objetos reales.
La toma cerrada del 13 de octubre mostró 38 autos RDB y dos filas esperando;
los primeros quedaron a 14 m del punto de barrera, con errores de 0,037 m y
0,0007 m respecto de su margen de 10 m más medio largo. El grafo completo resolvió
todos los cruces cargados. El recorrido de 380 m liberó ambas barreras tras
despejar toda la formación.
La captura final con el binario 17, a 380,91 m, muestra las barreras abiertas y
el tramo despejado desde una cámara fija; conserva los mismos 38 autos y 27
puntos de espera resueltos. El encuadre se restauró en el estado de la cámara,
además de su transformación, para que el sistema de vuelo no lo sobrescribiera.
La regresión de simulación también comprueba una aproximación en retroceso,
incluido el extremo trasero que encabeza el movimiento, y su restauración.

```sh
python3 scripts/check_native_world_fifth_batch.py \
  --viewer target/debug/openrailsrs-viewer3d \
  --route-root "$CHILTERN_ROUTE" \
  --out-dir tmp/fifth40-native-world \
  --cases crossing-closed crossing-released
```

## Instalaciones de carga: #285

Abrir `examples/player_lab/scenario_refill.toml` para una prueba propia rápida,
o usar un `Pickup` original compatible con agua, carbón o combustible diésel y un
vehículo con `IntakePoint` del mismo tipo. Detenerse con regulador cerrado y
alinear la toma; abrir el panel de tracción con `B` y pulsar «Abastecer en la
toma más cercana». «Cancelar abastecimiento» devuelve la pieza a su posición.
La instalación mueve su pieza animada mientras abre y retorna al terminar o
cancelar. La reserva de la instalación es finita y el combustible transferido no
restablece daños, temperatura ni consumo anterior.

En el laboratorio propio, el mástil está junto al coche y el brazo tiene volumen
para observarlo desde ambos lados. No está escondido dentro de la carrocería.
La captura final del binario 17 muestra ese conjunto durante una apertura del
25 %, pausada y restaurada sin cambiar el reloj ni su estado. La instalación
declara 45,36 kg de capacidad y 0,454 kg/s de suministro; todavía no entrega
combustible durante esa apertura. Las pruebas de simulación verifican por
separado suministro efectivo, agotamiento, cancelación y límites del depósito.

Guardar mientras está conectado y restaurar debe conservar operación, cantidad
transferida y apertura. Pausar detiene simultáneamente suministro y animación.
La animación usa todo el rango original para `ANIMATED_PARTS` y el rango corto
para otros modelos; una velocidad de animación cero sigue siendo instantánea.
No se afirma soporte de grúas, terminales o cargas incompatibles sin su modelo
operativo correspondiente.

## Cámara y nieve: #291, #296 y #297

En exterior, acercarse a un edificio o instalación opaca con movimiento y zoom.
La cámara debe detenerse antes de atravesar su volumen y deslizar sobre su pared,
con el límite del terreno activo. Si un sector nuevo encierra inicialmente el
ojo, sale por un lateral o por arriba. El caso `camera-collision` cruza un muro
propio y verifica que el ojo queda del lado de origen.

El ensayo llega primero al costado del muro y después intenta cruzarlo. El primer
desplazamiento también está sujeto a colisión: no se teletransporta el ojo detrás
de una pared para preparar la prueba. El informe registra las correcciones y los
volúmenes cercanos; una escena sin objetos no puede aprobar por accidente.

Elegir nieve y mirar un techo, fachada, vidrio y vía. La capa tiene espesor
irregular sólo sobre normales suficientemente orientadas hacia arriba. El máximo
es 8 cm en terreno y 5 cm en superficies marcadas. Fachadas, cabina y cristales
mantienen su material; la vía conserva una geometría utilizable.
Los modelos de prueba declaran normales por cara. Asignar una normal vertical a
una fachada daría un ensayo engañoso: el shader la trataría como superficie
superior, aunque el contenido original estuviera bien.

En `scenario_jacobs_curve.toml`, avanzar y observar las huellas detrás de las
ruedas declaradas. Se derivan de sus transformaciones físicas, no de un camino
pintado delante del tren. Fuera de esa franja queda cobertura. Pausar no produce
marcas nuevas; teletransportar o restaurar no dibuja una línea entre posiciones
lejanas. La posición absoluta conserva las huellas durante cambios de origen.

El registro guarda hasta 256 segmentos en cada uno de 25 sectores cercanos y
envía a GPU hasta 64 segmentos próximos, menos en calidad reducida. El shader
omite el recorrido de segmentos fuera de su caja conjunta. Un cuadro inmóvil o
pausado no vuelve a subir todos los materiales: sólo se actualizan los que
cambian de segmentos, de origen o aparecen al cargar un sector. Un modelo que carece
de ruedas declaradas no fabrica huellas: necesita un modelo con ejes compatibles.

La captura propia final con el binario 17 avanzó 35,70 m en curva y conservó
278 segmentos en dos sectores; envió 64 segmentos cercanos al shader. Se
inspeccionaron las fachadas de color y los techos nevados con las normales
correctas por cara, sin errores de compilación de shaders. Registró 425 MiB de
RAM y 100 actualizaciones de materiales durante el caso completo.

Con clima seco y sin historial de nieve, el sistema omite consultas de ruedas y
materiales. `snow_tracks.gpu_changes` y `material_uploads` permiten comprobar que
el avance normal no causa subidas de uniformes vacíos. El perfil opcional de
sistemas separa este coste del de simulación y carga de sectores.

`check_native_world_fifth_batch.py --cases roofs-snow` toma una vista fija de
South Ruislip con nieve, sombras y 512 partículas Hanabi. En la validación del
13 de octubre se inspeccionó el PNG: techos y terreno nevados, ventanas y fachadas
conservadas, vía utilizable y ninguna compilación de shader fallida. La captura
usó 1.445 MiB de RAM y aproximadamente 1.281 MiB de VRAM del proceso en RX 7600.
Estas cifras describen esa escena; no son un presupuesto para toda ruta o GPU.

## Evidencia y referencia

Las capturas nativas cubren las tres estaciones desde cabina/exterior, contacto
apagado/alto, horizonte elevado y una vista baja junto a la vía. Las propias cubren conexión efectiva,
separación y restauración, conducción en curva, Jacobs, pasajeros, pausa y rebase,
mojado/seco, nieve y colisión. Los resultados de una ejecución deben conservarse
junto con sus PNG, hashes y logs; no se versionan las instalaciones del autor.

Después de corregir la lectura de matrices anónimas válidas en modelos `.s`,
se repitieron trece vistas originales de Chiltern con el binario de fase 26:
las tres estaciones desde cabina y exterior, techos nevados, el cruce de Park Royal
cerrado y liberado, contacto apagado/alto, horizonte y vista baja. Los trece PNG
se inspeccionaron y terminaron sin shaders pendientes o fallidos en la RX 7600.
El [recibo de la repetición](validation/FIFTH40_CHILTERN_PHASE26_REFRESH.json)
conserva los hashes de binario, fuente, imágenes y diagnósticos, el intento excluido
por falta de espacio y las políticas de caché que llegaron realmente al visor.
La carpeta de repetición del cruce conserva su nombre `no-cache`, pero allí sólo
se desactivó la caché de texturas: las de fuente y geometría seguían habilitadas.
Las cuatro vistas adicionales sí desactivaron esas tres cachés explícitamente.

Las capturas históricas mantienen sus propios binarios y condiciones. La nueva
serie comprueba la composición de esos modelos originales tras el cambio de
lector; no certifica cambios posteriores de fuente, estabilidad de memoria durante
dos horas ni el paquete Snap final. Las dos imágenes de contacto son visualmente
similares: se comprobaron sus ajustes y sombras coherentes, sin atribuirles una
mejora cuantitativa que las imágenes no demuestran.

Una captura posterior con fase 27 sí muestra el puente de celosía original de
Park Royal (`WORLD` UID 11863, `SRV_150ftRSTTTrussBridge.s`) y su apoyo desde un
ángulo lateral bajo. La vegetación original tapa parte de su extremo izquierdo.
El [recibo separado de fase 27](validation/FIFTH40_PHASE27_BRIDGE_AND_COUNTRY.json)
registra ese encuadre y sus hashes: no sustituye a `bridge-low` ni se describe
como una cámara situada debajo del puente.

El primer paso de «Nueva partida» se comprobó aparte en ventanas de 800×600 y
640×480. La lista inicial de fase 27 ocultaba opciones y se conserva como
`DISCOVERYFAIL`. En fase 28, dos columnas de cuatro filas muestran los ocho
países sin desplazar la lista. El [recibo del menú](validation/FIFTH40_COUNTRY_PHASE28.json)
registra los rectángulos y etiquetas reales de la UI, además de los dos PNG
inspeccionados. A 640×480 se recorta parcialmente la nota explicativa posterior;
las ocho opciones y el pie fijo quedan completamente visibles.

Para comprobarlo a mano, abrir «Nueva partida» y redimensionar a ambos tamaños.
El primer paso debe ser «1 · País», con las ocho opciones visibles y ningún país
ya elegido. Elegir un país debe abrir sus rutas y servicios; si no tiene rutas
instaladas, el menú debe explicarlo. Volver a «País» y elegir «Todos los países»
debe recuperar la lista completa. Esta elección no limita los trenes que se
pueden elegir después. Las capturas de tamaños validan la composición; los clics
y la selección se verifican por separado.

Una prueba pesada de nieve y cámara ABA en la GPU integrada recibió `DeviceLost`
de RADV y falló; no se cuenta como aceptación. Las capturas livianas de pasajeros,
pared y nieve en curva pasaron en esa GPU, y el WORLD nevado se comprobó aparte en
la RX 7600. Las pruebas de memoria y fluidez deben ejecutarse con una sola carga
gráfica por dispositivo; un perfil concurrente no constituye una comparación
aislada de rendimiento.

La corrección posterior de vida útil de partículas mantiene tres imágenes
globales: mapa de refugio CPU, su versión codificada para Hanabi y textura de
pluma. `SceneCleanup` conserva sus IDs explícitamente; las demás imágenes del
escenario siguen retirándose. Esto evita añadir una nueva clave de material a
la caché de Hanabi por cada recarga. La textura de pluma conserva una copia CPU
de 16 KiB para recuperarse tras una pérdida del dispositivo.

El test sin GPU
`particle_textures::tests::scene_transfers_keep_particle_texture_ids_and_refresh_shelter_bytes`
ejercita doce recargas, datos nuevos del refugio, los mismos IDs y descriptor,
la pluma intacta y la retirada de una imagen transitoria. Bevy 0.19.1 ya usa
`RenderQueue.write_texture` y conserva textura y vista cuando el descriptor no
cambia; se mantiene ese mecanismo de actualización.

Este test y los PNG de aceptación no prueban por sí solos estabilidad de GTT.
Los bind groups antiguos pueden mantener ocupados bloques del allocator mayores
que sus texturas. La atribución requiere comparar bytes asignados y reservados
del allocator, además de los recursos activos y una ejecución real de dos horas
con el binario que incorpora el cambio. Esa aceptación prolongada está pendiente.

Otra retención confirmada se encuentra en Bevy 0.19.1: el mapa
`BinUnpackingBuffers.view_phase_buffers` elimina vistas retiradas, pero
`BinUnpackingBindGroups` sólo inserta o reemplaza entradas. Cada grupo viejo
mantiene referencias a cuatro buffers: metadatos, instancias agrupadas, trabajo
de preprocesamiento e índices de parámetros indirectos. Aunque desaparezcan
los buffers del mapa de fuentes, esas referencias pueden anclar bloques de
memoria del allocator.

`render_retirement` usa esas API públicas para conservar únicamente las claves
presentes en el mapa de fuentes. Se ejecuta en `PrepareBindGroups`, después de
`prepare_preprocess_bind_groups`: la escritura y retirada de buffers del cuadro
ya terminó, los grupos activos ya están preparados y la fase `Render` todavía
no despachó. La corrección se aplica también al visor normal; conserva vistas,
cascadas y fases activas sin reiniciar el historial de instancias ni modificar
dependencias. Si el renderer no usa preprocesamiento GPU, no crea sus recursos.

La fuente consultada es `bevy_pbr/src/render/gpu_preprocess.rs` (Bevy 0.19.1):
`BinUnpackingBindGroups`, `create_bin_unpacking_bind_groups` y
`create_bin_unpacking_bind_group`. El retiro de fuentes está en
`bevy_render/src/batching/gpu_preprocessing.rs::write_binned_instance_buffers`.
El test
`render_retirement::tests::retiring_views_releases_buffer_owners_and_preserves_live_phases_and_subviews`
ejercita doce recargas con generaciones de cámara, fases y cascadas diferentes.
Verifica que las referencias de buffers retirados se liberan y las activas
siguen vivas. El test `renderer_without_gpu_preprocessing_keeps_retirement_optional`
cubre recursos ausentes y un cuadro vacío sin GPU.

`render_resources.bin_unpacking_lifetime` registra entradas activas y fuente,
claves obsoletas antes/después de la poda y retiros acumulados. Para aceptación
GPU hay que ver retiros efectivos al cambiar de escenario, cero claves obsoletas
después de la poda y memoria asignada/reservada durante el ensayo prolongado.
Estos tests no prueban que desaparezca toda reserva alta: los buffers activos
y el allocator también mantienen capacidades para reutilización.

La referencia sigue fijada en Open Rails 1.6.1. Se consultaron las reglas de
`Simulation/LevelCrossing.cs`, los rangos de animación en
`Viewer3D/Shapes.cs` (`LevelCrossingShape`, `FuelPickupItemShape`),
`Viewer3D/RoadCars.cs` y las posiciones de acopladores/mangueras de
`Viewer3D/RollingStock/MSTSWagonViewer.cs`. Las extensiones propias —OIT, nieve,
figuras agrupadas y articulación ideal— tienen aceptación específica de Bevy;
una prueba equivalente no certifica recursos o sistemas no cubiertos.

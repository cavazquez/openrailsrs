# Visibilidad, biblioteca y entradas externas

Lote seleccionado el 16/10/2026: #385, #386, #388 y #415–#431.
Son once issues de funcionamiento y nueve de infraestructura/pruebas de entradas
externas; estos últimos forman una misma línea de testing dividida por formato.
Primero se escribe toda la implementación; luego se ejecutan las pruebas del
lote. Los resultados y los cierres se registran después de verificarlos.
Los issues de memoria y la migración de Bevy quedan fuera de esta entrega.

## Escenario: #385, #386 y #388

Las formaciones conservan su física. Las instancias WORLD grandes pueden
compactarse por vista en un compute shader, con mandos indirectos. Los grupos
pequeños, los dispositivos sin soporte, el viento, la nieve y los shaders aún
no preparados usan el buffer original. Las sombras siempre usan ese buffer;
una instancia fuera de cámara puede seguir proyectando una sombra visible.
Hay como máximo cuatro vistas con buffers compactados por grupo.
La activación automática exige un trabajo estimado de al menos un millón de
índices/vértices por grupo y cuadro; la primera escena pequeña mostró unos
0,1 ms de sobrecoste al forzarlo. El límite de salidas compactadas es de 64 MiB;
si no hay presupuesto se conserva el dibujo original. `OPENRAILSRS_WORLD_GPU_CULLING=1`
fuerza la evaluación de grupos desde 16 instancias para comparar; `0` la desactiva.

La oclusión utiliza triángulos opacos reales de superficies admitidas. Exige
que una caja completa esté cubierta y detrás de un triángulo, con márgenes.
Los casos inciertos, próximos al plano de recorte, materiales con recorte alpha,
caras descartadas, desplazamientos y sombras se mantienen en el dibujo normal.
La selección se acota a 48 fuentes y 1.024 triángulos; por vista se prueban hasta
64 triángulos proyectados. No es una certificación de todos los materiales.

Las texturas originales con mipmaps pueden presentar primero una cola de
menor resolución y después refinarse por tamaño visible y antigüedad. Se
conservan los bytes originales, alpha, sRGB y compresión. El presupuesto de
refinamiento es de 8 MiB por cuadro; se retienen temporalmente hasta 32 fuentes
y 64 MiB. Las texturas mayores o con disposiciones no admitidas conservan la
carga normal. El presupuesto no limita todas las cargas de Bevy ni la RAM del
proceso. Los originales no se reemplazan y no se generan mipmaps artificiales.
Al cambiar la vista GPU de una imagen se actualizan sólo los materiales que la
referencian. Esto incluye la textura de detalle del terreno y evita que quede
retenido un nivel de menor resolución después del refinamiento.

Para comparar el mismo binario con cada función y el conjunto:

```bash
python3 scripts/check_visibility_refinement.py \
  --binary target/debug/visibility_oracle --out tmp/visibilidad-01
```

La escena propia incluye objetos ocultos, instancias fuera de vista, cambio de
cámara, plano cercano y traslado del origen. Mide cuadros completos, no tiempos
GPU aislados. El resultado se limita al equipo, backend y escena registrados.
No sustituye las pruebas de todas las rutas o de AMD, Intel y NVIDIA.

Para comprobarlo a mano, iniciar un servicio, alternar cabina y exterior,
rodear edificios, acercarse a túneles y mover rápidamente la cámara. No deben
desaparecer edificios o trenes visibles. Las sombras deben conservarse al dejar
fuera de vista su objeto. Las texturas cercanas deben recuperar detalle.
Para una comparación se pueden desactivar funciones al iniciar el proceso:
`OPENRAILSRS_WORLD_GPU_CULLING=0`, `OPENRAILSRS_WORLD_OCCLUSION=0` y
`OPENRAILSRS_PROGRESSIVE_MIPS=0`. No cambian los archivos del escenario.

## Biblioteca: #415–#419

- **#415:** buscar un nombre con tildes compuestas o combinadas y sin tildes.
  Los nombres mostrados y los archivos originales deben conservarse. Las
  distinciones de escritura japonesa se mantienen.
- **#416:** enfocar el buscador y usar Inicio, Fin, RePág y AvPág. El cursor y
  la página deben seguir las coincidencias; Enter confirma un elemento.
- **#417:** buscar un nombre inexistente, activar filtros restrictivos y elegir
  un país sin rutas. Se debe explicar la causa y ofrecer limpiar o abrir
  Biblioteca sin iniciar otra ruta silenciosamente.
- **#418:** filtrar por trocha. Se exige información compatible de todos los
  coches; datos ausentes y trochas mixtas tienen advertencia. Quitando el filtro
  se pueden seleccionar para consultar su diagnóstico.
- **#419:** alternar orden por nombre y por favoritos. Buscar y filtrar no debe
  cambiar la identidad seleccionada; una auditoría que cambie las coincidencias
  tampoco debe confirmar otro tren por conservar sólo una posición numérica.
  A 800×600 y con escala 1,5 se debe ver al menos una formación completa al
  entrar. Las herramientas de edición siguen después de la elección principal;
  el diagnóstico de gálibo acompaña a la vista previa.

## Cola de descargas: #420–#422

Las escrituras persistentes se confirman antes de publicar cambios en memoria.
Si falla guardar la cola, no se consume un identificador ni se publica una
descarga nueva. Al reiniciar se descartan identidades ambiguas, se conserva la
cancelación y se exige retomar los trabajos pendientes. Una descarga reiniciada
no hereda los bytes de un intento que no utiliza reanudación HTTP.
La cancelación activa tampoco se envía al instalador si falla guardar su
solicitud. Los identificadores fuera del límite admitido producen un diagnóstico
antes de publicar un trabajo nuevo.

El progreso diferencia total desconocido de porcentaje. Un tamaño cero o menor
que los bytes recibidos no certifica finalización. La instalación sólo se marca
lista cuando termina la preparación, no cuando aparece un 100 % de transferencia.

Prueba manual: en Biblioteca, encolar dos paquetes originales, cancelar el
segundo y reiniciar el programa. La cancelación debe conservarse y el primero
debe esperar «Retomar cola». Estas pruebas descargan contenido del autor en la
carpeta del jugador; el contenido sigue fuera de Git y del paquete.

## Fuzzing: #423–#431

Los ejercicios de producción usan semillas propias. No necesitan mapas, GPU,
credenciales, red ni instaladores. La campaña determinista se diferencia del
fuzzing guiado por cobertura; pasar una no se presenta como haber ejecutado la
otra. Usar una carpeta nueva conserva los intentos anteriores.

```bash
cargo run --locked -p openrailsrs-fuzz -- \
  --runs 256 --seed 1612026 --out tmp/fuzz-01 --export-corpus fuzz/corpus
python3 scripts/fuzz_content.py --runs 256 --out tmp/instalador-01
```

- **#423:** infraestructura, reproducción, presupuestos y ejecución CI.
- **#424:** STF, lexer, comentarios, cadenas, Unicode y anidamiento.
- **#425:** contenedores SIMISA, expansión limitada y codificaciones.
- **#426:** texturas ACE/DDS, dimensiones, formatos y cadenas de mipmaps.
- **#427:** modelos binarios, bloques, offsets e índices.
- **#428:** terreno RAW y coordenadas extremas de tiles.
- **#429:** compilación y evaluación acotadas de SIGSCR.
- **#430:** framing multijugador, fragmentación, límites y mensajes inválidos.
- **#431:** URLs y ZIP sintéticos, metadatos, enlaces y escapes de directorio.

Ante un fallo se conservan `last-input.bin` y `last-case.json`. Para repetirlo:

```bash
cargo run --locked -p openrailsrs-fuzz -- --target terrain \
  --replay tmp/fuzz-01/last-input.bin --out tmp/reproduccion-01
python3 scripts/fuzz_content.py --replay tmp/instalador-01/last-input.bin \
  --out tmp/reproduccion-instalador-01
```

Para libFuzzer se requiere nightly y cargo-fuzz. Exportar primero el corpus
anterior. Un ejemplo acotado y su minimización:

```bash
cargo +nightly fuzz run stf -- \
  -max_total_time=30 -timeout=5 -rss_limit_mb=1024 -max_len=65536
cargo +nightly fuzz tmin stf fuzz/artifacts/stf/ARCHIVO_DEL_FALLO
```

La workflow «Bounded input fuzzing» ejecuta mutaciones deterministas en los PR
pertinentes. Su ejecución manual añade campañas libFuzzer de 30 segundos por
objetivo Rust. Los artefactos identifican el caso; no certifican ausencia de
vulnerabilidades. `check.sh` incluye el humo determinista y las referencias
ferroviarias mantienen sus límites previos.

## Resultados del lote

La [ejecución conjunta](fixtures/visibility-inputs-20261016/execution.json)
terminó con `check.sh` y Clippy correctos: 2.368 pruebas Rust aprobadas, ninguna
fallida y 49 ignoradas por sus condiciones habituales. Se conservaron las
referencias y tolerancias de Open Rails 1.6.1 y las seis trazas locales del
jugador. El servicio Northolt Park → South Ruislip → West Ruislip completó sus
tres paradas y 7.054 m en la simulación automatizada.

- **Gráficos:** 40 comparaciones de imágenes de la escena propia, sin diferencia
  de píxeles, combinando dos densidades, cuatro posiciones de cámara y cinco
  configuraciones contra la referencia. En la vista con oclusión comprobada,
  los envíos opacos pasaron de 47 a 10 y los 131 de sombras se conservaron.
  La [escena densa](fixtures/visibility-inputs-20261016/graphics-dense.json)
  registró una mediana de 2,33 ms frente a 1,88 ms en automático; el percentil 95
  pasó de 8,27 a 5,84 ms. Son cuadros completos de ese ensayo en una RX 7600,
  no tiempos GPU aislados. Forzar compute en grupos pequeños añadió coste;
  por eso la selección automática mantiene el dibujo normal en esos grupos.
  La oclusión por sí sola tampoco mejoró todos los percentiles.
- **Contenido original:** se compararon [cabina y exterior de Chiltern](fixtures/visibility-inputs-20261016/chiltern-views.json)
  con las mismas condiciones y se recorrieron [504 m en ambas configuraciones](fixtures/visibility-inputs-20261016/chiltern-driving.json).
  En ese tramo corto, las dos mantuvieron un percentil 95 de conducción de
  8 ms y ningún cuadro de conducción por encima de 100 ms. La carga inicial
  se registra por separado. Esto no demuestra una mejora de velocidad en
  todas las rutas ni igualdad artística completa con Open Rails.
- **Menú:** [ocho capturas verificadas](fixtures/visibility-inputs-20261016/menus.json)
  de rutas, trenes, biblioteca y descargas, a 800×600 y a 1280×720 con escala
  1,5. La elección principal y el pie de lanzamiento permanecieron accesibles.
- **Entradas externas:** [11.776 mutaciones deterministas Rust](fixtures/visibility-inputs-20261016/stable-mutations.json),
  [5.120 del instalador sin red](fixtures/visibility-inputs-20261016/installer-mutations.json)
  y [siete campañas libFuzzer con ASAN](fixtures/visibility-inputs-20261016/libfuzzer.json),
  de 30 segundos por objetivo. Se encontró y corrigió un pánico real al leer
  canales ACE incompletos. Se conserva el [caso reducido a 184 bytes](fixtures/visibility-inputs-20261016/ace-minimized.ace),
  junto con el original de 423 bytes y una regresión que exige un error de
  lectura en ambos. Estas campañas acotadas no certifican ausencia de fallos.

Las comparaciones gráficas usan un mismo binario en todas sus variantes.
Después se añadieron dos protecciones de cola y se corrigió una comprobación
de orden en un test, conservando sin cambios los archivos de renderizado y
la distribución del menú. Se volvió a abrir el visor completo desde
[cabina y exterior con la compilación final](fixtures/visibility-inputs-20261016/final-runtime.json).
El manifiesto registra los hashes de ambas compilaciones y de sus fuentes.

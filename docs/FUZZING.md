# Campañas de fuzzing

Los siete objetivos Rust ejercitan lectores reales con entradas sintéticas:
STF, SIMISA, ACE/DDS, modelos, terreno, SIGSCR y multijugador. No cargan rutas,
GPU, credenciales ni contenido descargado. El instalador tiene además un
ejercicio determinista de URLs y ZIP en `scripts/fuzz_content.py`; ese ejercicio
no es una campaña de cobertura libFuzzer.

## Resultado del 16 de octubre de 2026

Los siete objetivos completaron 1800 segundos cada uno con AddressSanitizer:
**411.670.546 ejecuciones y ningún hallazgo registrado**. El corpus conservó
54.207 entradas repartidas entre los siete objetivos. Las 14 pruebas del
ejecutor y de los límites del instalador también pasaron.

Se acreditaron 42 tramos completos de 300 segundos, sin objetivos simultáneos.
Tres tramos interrumpidos quedaron registrados y no aportan tiempo al total.
STF combina dos tramos previos y cuatro finales: se comprobaron las mismas
fuentes Rust y el mismo ejecutable; entre esos intentos sólo cambió el ejecutor
para comprimir logs y permitir procesos acotados. Los ensayos de preparación
tampoco cuentan dentro de la media hora.

El [informe de ejecución](fixtures/fuzz-30min-20261016/execution.json) registra
tiempos, semillas, hashes, versiones, inventarios y resultados por tramo. Los
logs completos e inventarios detallados permanecen en
`tmp/fuzz-30min-20261016/`; el corpus sigue en `fuzz/corpus/`, fuera de Git.
Se verificaron los hashes comprimidos y sin comprimir de los 42 logs, las
fuentes y los ejecutables. No quedaron procesos de la campaña abiertos.
Este resultado corresponde a lectores y protocolos con entradas sintéticas;
no valida gráficos, conducción ni un recorrido completo.

## Media hora por objetivo, uno por vez

El ejecutor requiere un entorno POSIX, como Linux, y Rust nightly con `rust-src`
y `cargo-fuzz`. Exportar las semillas propias una vez; el directorio de salida
del exportador debe ser nuevo:

```bash
cargo run --locked -p openrailsrs-fuzz -- \
  --runs 1 --out tmp/fuzz-seeds-01 --export-corpus fuzz/corpus
python3 scripts/run_fuzz_campaign.py \
  --seconds-per-target 1800 --out tmp/fuzz-30min-01
```

La compilación con AddressSanitizer ocurre antes del presupuesto. Después se
ejecuta un único objetivo y un único trabajador: sin `fork` ni trabajos de
fuzzing paralelos. Los siete presupuestos suman 3 horas y media, más compilación,
carga del corpus y cierres de los procesos. Se pueden elegir objetivos concretos
con `--targets textures shapes` o hacer una ronda corta con
`--seconds-per-target 30`.

Por defecto cada entrada tiene un límite de 5 segundos y cada proceso de
1024 MiB de RSS; son presupuestos del ensayo, no requisitos del juego.
La carpeta de compilación es `tmp/fuzz-build`; `--target-dir` permite reutilizar
otra compilación cargo-fuzz. Se verifica que las fuentes no cambien durante la
compilación ni durante la campaña y se registran los hashes de los ejecutables.

## Conservar descubrimientos y retomar

- `fuzz/corpus/<objetivo>/`: entradas que libFuzzer conserva por su cobertura.
  La siguiente campaña vuelve a usarlas. No se vacía ni reemplaza el corpus.
- `tmp/fuzz-30min-01/progress.json`: progreso, semillas, comandos, hashes,
  inventarios del corpus, cobertura, ejecuciones, tiempos y memoria informada.
- `events.jsonl`: inicio y resultado de cada tramo.
- `<objetivo>/attempt-NNN/fuzz.log.gz` y `artifacts/`: log y entradas originales
  de cada intento, sin sobrescribir intentos anteriores.
- `fuzz/findings/<objetivo>/<tipo>-<sha256>/`: entrada, ocurrencias y comandos
  exactos para reproducir y, para crashes/leaks, intentar minimizar el caso.
  Se separan crash, timeout, límite de memoria, leak y entradas lentas. Un hash
  repetido agrega una ocurrencia; varios hashes no implican varios errores.

El progreso se guarda cada diez segundos. Los presupuestos se acreditan en
tramos completos de cinco minutos y se exige el resumen real de libFuzzer:
salir con código cero antes de tiempo no cuenta como media hora completada.
Ctrl+C o SIGTERM cierra el proceso hijo y sus descendientes. Para continuar:

```bash
python3 scripts/run_fuzz_campaign.py \
  --seconds-per-target 1800 --out tmp/fuzz-30min-01 --resume
```

Hay que conservar las mismas opciones, fuentes y ejecutables. Se repite el
tramo interrumpido, no los que ya finalizaron. Si se modificó el código para
corregir un fallo, elegir una salida nueva: reutiliza el corpus y conserva el
resultado de la versión anterior. Un bloqueo impide dos ejecutores usando ese
mismo corpus simultáneamente.

Si el entorno interrumpe trabajos prolongados, añadir `--max-slices 1`: el
proceso termina limpiamente después de un tramo, con estado `checkpoint`.
Repetir el mismo comando con `--resume` continúa el siguiente. Este límite
por proceso puede cambiar al retomar; los treinta minutos por objetivo se
mantienen y sólo el estado `complete` acredita la campaña terminada.

Los logs se comprimen mientras se generan, conservando todos los bytes y su
hash sin comprimir. Algunos lectores emiten un aviso por cada entrada inválida;
esos avisos repetidos no se consideran hallazgos. Se pueden leer con
`gzip -dc RUTA/fuzz.log.gz`.

El corpus y los hallazgos quedan fuera de Git. Un caso pequeño, propio y
reproducible que expone un error confirmado debe incorporarse después a las
pruebas de regresión del lector correspondiente, junto con su corrección.

## Reproducir, minimizar y compactar

Cada `finding.json` guarda un `replay_command` con el ejecutable y la entrada
exactos. El comando `minimize_command`, cuando corresponde, escribe
`minimized.bin` sin borrar el original. Reproducir el reducido y comprobar que
conserva la misma causa antes de usarlo como regresión.

Para reducir el corpus por cobertura sin eliminar el original:

```bash
mkdir -p fuzz/corpus-compacto/textures
tmp/fuzz-build/x86_64-unknown-linux-gnu/release/textures \
  -merge=1 fuzz/corpus-compacto/textures fuzz/corpus/textures
```

La compactación depende del ejecutable y sus opciones. Comparar cobertura con
la misma versión; un número de aristas de otro binario no es un porcentaje
comparable. No guardar cada mutación: el corpus conserva casos útiles y los
artefactos conservan fallos y entradas lentas.

## GitHub Actions

La ejecución manual de **Bounded input fuzzing** permite elegir 30, 300 o 1800
segundos por objetivo. Reutiliza el corpus sintético de campañas previas de la
misma rama y publica corpus, hallazgos, logs y progreso como artefactos durante
30 días, incluso si falla el ensayo. Los PR conservan las pruebas cortas y las
regresiones del ejecutor; no arrancan automáticamente una campaña de 3h30.

Fuentes: [corpus, límites y opciones de libFuzzer](https://llvm.org/docs/LibFuzzer.html),
[guía de cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html).
Completar una campaña acotada no demuestra ausencia de errores.

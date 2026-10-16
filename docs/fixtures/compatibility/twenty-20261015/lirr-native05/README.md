# Brooklyn–Hempstead: referencia nativa completa, aceptación Rust pendiente

Captura de **Open Rails 1.6.1**, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`, ejecutando sus DLL originales. No contiene la ruta, archivos ENG/WAG/CON/PAT, un grafo importado, DLL, binarios ni prefijo Wine. Los originales sólo se identifican por sus hashes en `source-inputs.json`.

La actividad original es `01Brooklyn-Hempstead.act`, formación `SFexpress-500`: siete vehículos, 150 t y 73,21 m. La referencia completa contiene **33.703 muestras a 20 Hz**, 1685,10 s y 31.361,73 m. Las dos tareas de estación originales terminaron con `IsCompleted=true`. `coverage.json` certifica esa cobertura nativa; **no certifica paridad de Rust**.

## Archivos y mediciones

- `reference.csv.gz` conserva sin pérdida el CSV de 14.706.950 bytes; su hash descomprimido es `86c44d3042dbcd7f7ed398706b7cacfab4e8c441c845ef4b5aaa8c07cc0e9976`.
- `protocol.json` conserva íntegros los límites declarados antes de ejecutar: RMS de velocidad 0,75 m/s, error máximo de odómetro 45 m, RMS de tubería/cilindro 5 psi cada uno y RMS de fuerza motriz/freno 5000 N cada uno. No se ajustaron después del resultado.
- `measurement-declaration.json` se escribió **antes de la captura 05**. Corrige dos canales del capturador: las fuerzas motriz y gravitatoria se expresan en el marco del tren, y los mandos comparados son los valores reales del controlador.
- `initial.json` incluye los campos numéricos de frenos, depósitos y controlador inmediatamente después de la inicialización original, antes del primer tick. Son observaciones del original; no se usan para imponer un estado posterior a Rust.
- `ActivityTrajectory.cs`, `HeadlessCapture.cs`, `capture-script.py` y `oracle_wine.py` son las fuentes exactas utilizadas. Las copias de scripts son evidencia; para ejecutar desde el repositorio se usan los archivos de `scripts/`.
- `capture-execution.json` y `wine-isolation/` registran la ejecución aislada, los hashes de las DLL y la limpieza de sus procesos. Las rutas absolutas del equipo se reemplazaron por marcadores; se conservan por separado los hashes de los recibos privados originales.
- `reference-pin-at-capture.toml` es el catálogo histórico exacto de esa ejecución. Actualizar posteriormente otro ensayo del catálogo no cambia las DLL, fuentes ni datos de esta captura.

OR `Train.UpdatePhysics` invierte `MotiveForceN + GravityForceN` por `Flipped XOR rearCab` antes de calcular el movimiento del tren. El powercar trasero de esta formación está invertido. El CSV conserva **cada fuerza local, su signo y su fuerza orientada**, además de las sumas; sumar directamente los valores locales cancelaba dos motores que empujaban en el mismo sentido ferroviario. `measurement-coherence.json` comprueba que las sumas orientadas coinciden exactamente con esos canales por coche.

`throttle` y `brake` contienen `CurrentValue` después de los controladores originales. `requested_throttle` y `requested_brake` conservan las demandas del conductor. En esta formación, demanda de regulador 0,5 produce 0,4; demandas de freno 0,45/0,69 producen 0,36/0,552 porque el último notch de emergencia está en 0,8. El estado inicial original tiene mando 0,7, tubería 64 psi y cilindro 65 psi. Son valores del contenido y código originales, no calibraciones para coincidir con Rust.

## Diagnósticos conservados

La captura 04 y la 05 tienen idénticos tiempos, velocidades, odómetros, presiones, fuerzas de freno/fricción, límites y puertas, muestra por muestra. El conductor y la física originales no cambiaron. La 04 conserva la medición anterior de fuerza local y mandos demandados en `historical/native04.csv.gz`; debe leerse con esa limitación.

`diagnostics/replay02.csv.gz` conserva un replay completo pero fuera de tolerancia, anterior a la corrección de los canales. Su comparación histórica queda en `replay02-comparison.json`; sus mandos y suma motriz no tienen el contrato corregido de la 05.

`diagnostics/replay03.csv.gz` usa los mandos reales de la 05 con los mismos binarios. Se truncó al alcanzar antes el extremo físico de la ruta: 1609,261 s y 31.379,468 m. El último paso es parcial; no se cuenta como muestra regular de 20 Hz ni se publica un RMS de aceptación sobre ese prefijo. El error y los hashes de entrada/salida están en su recibo. No se amplió el recorrido, se trasladó el tren ni se cambió una tolerancia para evitar el fallo.

El siguiente replay completo, con la implementación neumática corregida, debe registrarse como **otro intento**. Los anteriores se conservan. El issue #220 permanece abierto mientras falte esa aceptación.

## Reproducción

Se necesitan los originales identificados por el protocolo. No se descargan ni se redistribuyen desde esta carpeta. Desde la raíz del repositorio, con los ejecutables recién compilados, descomprimí la referencia en una carpeta temporal propia:

```bash
python3 - <<'PY'
import gzip
from pathlib import Path
source = Path('docs/fixtures/compatibility/twenty-20261015/lirr-native05/reference.csv.gz')
target = Path('tmp/lirr-reference-replay/native.csv')
target.parent.mkdir(parents=True, exist_ok=False)
target.write_bytes(gzip.decompress(source.read_bytes()))
PY
```

Después importá el ACT y CON originales mediante `target/debug/openrailsrs import-msts` y ejecutá `target/debug/examples/activity_trajectory escenario-importado/scenario.toml tmp/lirr-reference-replay/native.csv tmp/lirr-reference-replay/rust.csv`. No uses otro itinerario, una formación alternativa ni un CSV corto como sustituto. La comparación debe exigir igual cantidad de muestras, 20 Hz, todos los controles y los seis límites de `protocol.json`; un error de importación o fin anticipado no es aprobación.

Para volver a capturar el original, `scripts/capture_lirr_full_service.py --native-only` acepta `--activity`, `--consist`, `--out-dir` y un `--wine-prefix` privado marcado. Ese modo comprueba DLL originales, hashes antes/después y cobertura completa, pero **no ejecuta ni aprueba Rust**. Wine permanece sin acceso al escritorio mediante `OracleWine`.

El manifiesto identifica las compresiones, exportaciones de recibos y hashes originales. No se debe reescribir esta referencia para acomodar una implementación que difiere.

## Contexto histórico identificado después

Esta captura heredó `Simulator.Paused=true` del cargador headless y no ejecutó el inicializador climático del visor. OR omite el cálculo ambiental al estar pausado y limita el coeficiente base inicial a 0,5. La comparación anterior contra un Rust con adhesión 1 no tenía las mismas condiciones de partida. Estos archivos se conservan íntegros como evidencia histórica; no se consideran aceptación de una partida normal. La nueva referencia `../lirr-native07` declara el estado despausado, el preset Clear original y adhesión 100/variación 0 antes de ejecutarse, manteniendo todos los límites numéricos.

`diagnostics/replay04.csv.gz` conserva el siguiente replay completo: 33.703 muestras y mandos exactos. Tubería RMS 0,000416 psi y cilindro RMS 0,000229 psi cumplen; velocidad 3,88144 m/s, odómetro 1463,649 m, fuerza motriz 14315,18 N y fuerza de freno 7958,46 N siguen fuera de sus límites. El resultado bruto permanece en `replay04-comparison.json`; no prueba paridad jugable con ese contexto pausado.

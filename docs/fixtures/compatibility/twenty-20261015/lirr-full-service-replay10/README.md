# Brooklyn–Hempstead: comparación completa aprobada

El replay 10, ejecutado con el build 11, cumple los **seis límites declarados**
contra las DLL originales de Open Rails 1.6.1. Se compararon **32.583 muestras a
20 Hz**, 1629,10 s y 31.361,20 m de referencia, con los siete vehículos de
`SFexpress-500` y las dos tareas de estación originales. La referencia es la
captura despausada 07; sus archivos históricos permanecen intactos en
[`../lirr-native07`](../lirr-native07/README.md).

Resultados y límites originales:

- Velocidad RMS: **0,0398531564 m/s**, límite 0,75 m/s.
- Error máximo de odómetro: **11,486328125 m**, límite 45 m.
- Fuerza motriz RMS: **3114,406783 N**, límite 5000 N.
- Fuerza de freno RMS: **72,907216 N**, límite 5000 N.
- Tubería RMS: **0,000418428 psi**, límite 5 psi.
- Cilindro RMS: **0,000115632 psi**, límite 5 psi.

`comparison.json` conserva los números completos, los seis resultados y la
cobertura. Los mandos reales de regulador, freno y puertas coinciden en todas
las filas. La referencia usa el ACT/CON originales, señales, plataformas y
embarque. Rust recorre la misma vía física e inicializa sus propios estados a
partir del contenido. No recibe velocidades, RPM, fuerzas, presiones ni
posiciones observadas. La evaluación de objetivos y puntuación ACT está
desactivada expresamente durante este replay físico para evitar un corte de
la medición; **esta aprobación no certifica esa evaluación ni todo el tráfico**.

## Qué cambió

La tubería y los cilindros plain usan el controlador, los volúmenes y la
precarga originales. La fuerza de las zapatas conserva su dependencia de la
velocidad. Se corrigieron la orientación de los powercars, el radio del motor,
la fase de publicación MU, el integrador adaptativo Pacha, el cálculo legacy de
Davis y la proyección gravitatoria `sin(inclinación)`.

El build 11 añadió la aritmética Single del gobernador con su historial dRPM,
el avance por velocidad final y el odómetro Single para formaciones nativas
escalares/rígidas, y la masa fija del motor original cuando no tiene animación
continua de carga. Los motores explícitos del modelo propio conservan sus
reservas variables; el solver elástico Rust conserva su integrador. No se
afirma aquí equivalencia completa de las interpolaciones FreightAnim.

También se incorporó la resistencia de túnel calculada desde TRK/TSECTION,
posición y gálibo `Size`. La
[captura diagnóstica 08](../lirr-native08-diagnostics/README.md) confirmó en las
DLL una fuerza antes ausente de unos 304 N a los 400 s y 239 N a los 500 s.
Sus 164 canales anteriores reproducen exactamente la captura 07. Estas fuerzas
sirvieron para localizar la omisión; no se inyectan en la física Rust ni se
ajustó una constante para imitar la trayectoria. Varias correcciones conviven
en este build; la diferencia total no se atribuye a una sola.

## Archivos y procedencia

`rust.csv.gz` conserva sin pérdidas los 3.799.961 bytes del CSV medido; su SHA
sin comprimir es
`50514006ecc104e5666c3593acc959aba5089d7e6cb11a26fa8494fd6d8c4b1c`.
La referencia canónica es `../lirr-native07/reference.csv.gz`, SHA original
sin comprimir
`21212399f6289ff44b6d108d6312670008e6503abee0054df2d4dd150d6e2be2`.
El gzip no redondea, reduce filas ni elimina la muestra inicial.

`receipt.json` conserva la ejecución y sus entradas antes/después;
`build11.json` liga el resultado a los ejecutables exactos:

- CLI: `d38504e5c694b7378e49fbe7fbaf7e2acb748727007d23a7d968a00cb41eee59`.
- Replay: `5197e72567b99b0e72eaf41a4a56348471719e58c52485822ba1a478eae70b38`.

`runtime-source-inputs.json` identifica las 752 entradas del build congelado;
`acceptance-fixture-inputs.json`, sus 27 fixtures nuevos. `configuration.json`
explica el contexto y las únicas entradas reproducidas.
`replay-script.py` es una copia literal del arnés que ejecutó la medición,
incluida su descripción histórica; la orden efectiva con `--native-dir`
explícito está en el recibo. Su ubicación de ejecución original era
`tmp/twenty-20261015/`.

El manifiesto liga los archivos publicados con los SHA privados originales.
Sólo se reemplazaron prefijos del equipo en JSON y logs. No se redistribuyen
ENG/WAG/CON/PAT/TDB originales, grafo importado, DLL, ejecutables ni prefijo
Wine. Los originales se identifican por hash y se obtienen de su autor.

## Intentos anteriores y reproducción

`history.json` conserva compactos los fallos numéricos de los replays
05, 06, 08 y 09, con sus binarios y contexto. El replay 07 se marca **inválido**:
se inició antes de terminar el build 09 y no es aceptación, sea cual sea su
resultado. Los intentos con el simulador pausado siguen separados en
[`../lirr-native05`](../lirr-native05/README.md). Ningún fallo fue convertido
en aprobación y ningún límite se amplió.

Para repetir la física, se necesitan los originales cuyos hashes figuran en
el recibo, una compilación coherente y una carpeta de salida nueva. El
capturador `scripts/capture_lirr_full_service.py --native-only` produce una
referencia nueva con su propio recibo privado. Se puede copiar
`replay-script.py` a `tmp/UN_ENSAYO/replay.py` y ejecutarlo con `--native-dir`,
`--binary`, `--replay-binary` y `--out-dir` explícitos. El arnés verifica los
hashes, importa el ACT/CON, reproduce sólo los mandos y aplica el protocolo.
No construye binarios ni ejecuta Wine por sí mismo.

Esta carpeta acredita el subconjunto físico descrito. No reemplaza la puerta
global del proyecto, una prueba visual ni una partida ACT completa con
puntuación, y no extiende el resultado a todos los trenes, rutas o climas.

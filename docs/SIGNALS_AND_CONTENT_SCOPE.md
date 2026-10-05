# Señales, formaciones y scripts: ampliación verificable

Referencia: Open Rails **1.6.1**, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`.
La compatibilidad sigue siendo por función y contenido, con límites explícitos.

## Señalización

El corredor extendido contiene **66 cabezas**, con **15 tipos de programa**:
44 NORMAL, 21 DISTANCE y un INFO (contrapeso). No son 66 programas distintos.

- SIGSCR verifica funciones, argumentos, selectores y variables en todas las
  ramas al compilar, incluidas las ramas que no se ejecutan al empezar.
- Los locales `float` se inicializan a cero por actualización como en OR.
  La lista de locales se prepara al compilar y se reutiliza.
- Una orden del despachador a Alto o Precaución restringe el aspecto antes de
  que el programa de la señal anterior consulte `NEXT_SIG_LR`.
- El contrapeso INFO conserva su aspecto visual y no crea autoridad de parada
  ni entra en la tabla normal del TCS o en el límite de búsqueda distante.
- El coordinador concede en exclusiva el próximo bloque normal al aproximarse
  a su entrada. Usa intervalos físicos canónicos: `eNN` y `eNN_r` comparten vía.
  Comprueba formaciones completas, coches estacionados, posiciones de desvíos
  y reservas existentes. Retiene una concesión antes de resolver peticiones
  nuevas por identificador de servicio. Se libera al avanzar; la ocupación
  sigue protegiendo la cola. La concesión propia se guarda y valida al cargar.

Esto no reproduce el despachador general de OR, sus bloqueos de agujas,
reversas/itinerarios variables, enlaces completos ni resolución de deadlocks.
Las reservas se habilitan en sesiones con programas nativos; los escenarios
anteriores mantienen sus reglas declarativas. Una red en vía única puede
necesitar un planificador de cruces para evitar esperas sin salida.

## Compatibilidad del material

`audit-consists --json` entrega ahora `report.compatibility` por vehículo:
motor, tipo de freno, scripts declarados, sonidos, trocha y déficit de peralte.
La auditoría comprueba también dependencias SMS/WAV locales y del SOUND global,
además de modelos, texturas y cabinas. Caché compartida de vehículos y geometría.

Los avisos distinguen recursos obligatorios ausentes, sonidos opcionales,
tracción eléctrica/vapor parcial y scripts C# que requieren selección del host
o que todavía no se ejecutan (frenos/alimentación). El menú muestra los avisos;
los modelos/cabinas declarados ausentes impiden iniciar como antes. Un WAV
faltante o una función parcial se informa sin invalidar la formación completa.

```bash
target/debug/openrailsrs audit-consists /ruta/TRAINS/CONSISTS --json
```

No ejecuta automáticamente scripts encontrados ni certifica subsistemas porque
todos los archivos existan. Tampoco valida todos los binarios ni los posibles
comandos del lenguaje SMS; el motor de audio conserva su propio diagnóstico.

## C# y peralte

El [host opcional](TCS_CSHARP_HOST.md) recibe aspectos nativos, señales y postes
por índice, y máxima del tren separada del límite de vía. Sigue requiriendo
selección explícita de un script compatible, SDK .NET y API acotada. El guardado
del estado interno del script y los demás hosts C# aún requieren trabajo.

El [documento de peralte](https://openrails.org/files/superelevation_v1.pdf)
explica la velocidad de equilibrio y el déficit admitido. Se conserva la
fórmula `v = sqrt((peralte + déficit) * g * radio / trocha)` del código fijado.
`ORTSTrackGauge` admite metros y pies+pulgadas;
`ORTSUnbalancedSuperElevation` conserva el déficit explícito válido.
Se usa el código 1.6.1 para los valores por defecto: pasajero/carga 3 pulgadas,
locomotora/ténder 6 pulgadas, categoría desconocida 0,01 pulgadas. El PDF antiguo
describe otro valor por defecto para carga; no se aplica a esta referencia.
`Carriage` se interpreta como pasajero, como en OR. El importador conserva los
campos originales de trocha y déficit, incluso en los vehículos reducidos del
servicio extendido; los valores omitidos mantienen los defaults de la referencia.

F8 → **Locomotora** muestra radio y roll del tramo TDB/TSection validado, peralte
geométrico, trocha y velocidad de confort más restrictiva de los coches
acoplados, evaluada en la curva de la cabeza. El exceso de confort se informa.
Se mantiene el roll escrito en la vía para tren/cabina; no se genera aquí un
peralte nuevo ni una transición automática en mallas originales. Tampoco es
un modelo de suspensión, descarrilamiento o penalización de conducción. Cuando
TSection no da trocha, el diagnóstico utiliza 1,435 m. Faltan perfiles por bogie
y la reproducción de los estándares/runoff de `SuperElevation.cs`.

## Comprobaciones

```bash
cargo test -p openrailsrs-sim -p openrailsrs-train -p openrailsrs-track -p openrailsrs-formats
OPENRAILSRS_DOTNET=/ruta/dotnet bash scripts/check_tcs_host.sh
python3 scripts/capture_superelevation_or.py --source-root ../openrails \
  --dotnet /ruta/dotnet --out-dir tmp/superelevation-recapture
python3 scripts/run_oracles.py --verify-only --source-root ../openrails
```

La captura compila la expresión **original** extraída por `git show` del commit
fijado, conserva aritmética C# `float` y escribe diez casos en una carpeta nueva.
No reemplaza referencias. `oracles/superelevation-or-1.6.1.json` tiene su hash
fijado y tolerancia de **0,00002 m/s**; el test Rust comprueba esos resultados.
Es un oráculo aislado de la fórmula, no una ejecución completa del simulador OR.

Las regresiones adicionales cubren ramas SIGSCR no ejecutadas, la propagación
de Alto, un bloqueo entre trenes opuestos, liberación/guardado, reservas inválidas,
recursos opcionales incompletos y el servicio entero de seis paradas con tráfico.
La aceptación C# verifica la transferencia real SIGSCR → JSONL → script → freno
físico. [Pruebas manuales](PLAYER_MANUAL_TESTS.md#29-señales-compatibilidad-c-y-peralte).

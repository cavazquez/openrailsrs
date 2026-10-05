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
  nuevas por antigüedad de espera e identificador. Se libera al avanzar; la
  ocupación sigue protegiendo la cola. Las concesiones propias se guardan y
  validan al cargar.

El coordinador concede **bloque y agujas de forma atómica**, comparte la posición
entre jugador y tráfico y rechaza órdenes manuales sobre agujas reservadas.
Mantiene bloqueos mientras la formación completa o una sección estacionada
ocupa su gálibo de 5 m. F8 → Despachador muestra dueños, espera y cambios de ruta.

Después de cinco segundos esperando, busca una alternativa hacia delante que
conserve las estaciones pendientes y el destino, evitando ocupación, reservas
ajenas y agujas bloqueadas. No retrocede sobre el prefijo ocupado. Detecta ciclos
de espera; si no hay alternativa mantiene la restricción y lo informa. No
reproduce las reversas automáticas, todos los enlaces, las órdenes de horarios
ni el despachador completo de OR. Sin señales usa autoridad móvil hacia delante
y ocupación física, sin acumular una nueva reserva retenida por cada quantum.
Al cargar se rechazan concesiones superpuestas y posiciones compartidas
contradictorias antes de modificar la partida.

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
selección explícita de un script compatible, SDK .NET y API acotada. Save/Restore
conserva estado, mensajes y eventos cuando el tipo sobrescribe ambos hooks. Se
vincula a hash/tipo y prepara un host nuevo antes de aplicar el guardado. Los
hosts C# de frenos y alimentación y otros miembros del API siguen pendientes.

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
El **peralte automático** agrega perfiles de cant/runoff al TDB, con los
estándares métricos/imperiales o `ORTSSuperElevation` del TRK. El mismo perfil
deforma las mallas originales de vía y sitúa tren/cámara, manteniendo UV,
normales y transiciones de LOD. La elevación conserva la altura del carril
interior; el contacto entre la vía deformada y la pose se verifica por debajo de
1 mm en una prueba geométrica a grandes coordenadas.

Se conserva el roll escrito. Se excluyen agujas, vías múltiples, carreteras,
colocaciones WORLD incompatibles y las tablas antiguas `ORTSTrackSuperElevation`
sin un oráculo propio. Se usa la velocidad de diseño del TRK para las curvas;
falta validar velocidades locales por categoría/dirección, perfiles de bogies,
suspensión y descarrilamiento. F10 permite desactivar generación para la próxima
partida; `OPENRAILSRS_SUPERELEVATION=0` permite reproducir la vista anterior.
Cuando TSection no da trocha, el diagnóstico utiliza 1,435 m.

## Descarga de contenido

El menú ofrece el catálogo curado de Open Rails y prepara las actividades de los
paquetes compatibles. Descarga en segundo plano, cancelación, instalaciones
independientes, procedencia y auditoría; no reemplaza Chiltern ni ejecuta
instaladores o scripts del paquete. El catálogo también tiene enlaces de autor
y contenido comercial que requieren obtenerse en su origen. Las dependencias
MSTS ausentes no siempre están disponibles libremente. [Guía](OFFICIAL_CONTENT.md).

## Comprobaciones

```bash
cargo test -p openrailsrs-sim -p openrailsrs-train -p openrailsrs-track -p openrailsrs-formats
OPENRAILSRS_DOTNET=/ruta/dotnet bash scripts/check_tcs_host.sh
python3 scripts/capture_superelevation_or.py --source-root ../openrails \
  --dotnet /ruta/dotnet --out-dir tmp/superelevation-recapture
python3 scripts/capture_cant_profiles_or.py --source-root ../openrails \
  --dotnet /ruta/dotnet --out-dir tmp/cant-profiles-recapture
python3 scripts/test_official_content.py
python3 scripts/run_oracles.py --verify-only --source-root ../openrails
```

La captura compila la expresión **original** extraída por `git show` del commit
fijado, conserva aritmética C# `float` y escribe diez casos en una carpeta nueva.
No reemplaza referencias. `oracles/superelevation-or-1.6.1.json` tiene su hash
fijado y tolerancia de **0,00002 m/s**; el test Rust comprueba esos resultados.
Es un oráculo aislado de la fórmula, no una ejecución completa del simulador OR.
El segundo oráculo conserva diez perfiles generados por `MarkSections`, los
constructores de estándares y conversiones originales, con tolerancias de
**0,00001 m de cant** y **0,000001 rad de roll**. Los hashes y el commit se fijan
en `oracles/openrails-reference.toml`; no se reemplaza la referencia histórica.

Las regresiones adicionales cubren ramas SIGSCR no ejecutadas, la propagación
de Alto, un bloqueo entre trenes opuestos, liberación/guardado, reservas inválidas,
recursos opcionales incompletos y el servicio entero de seis paradas con tráfico.
La aceptación C# verifica la transferencia real SIGSCR → JSONL → script → freno
físico. [Pruebas manuales](PLAYER_MANUAL_TESTS.md#30-peralte-despachador-guardado-c-y-contenido-oficial).

# Host opcional C# para TCS (issue #164)

La decisión es **B: proceso .NET separado**, con **A: `BasicEtcsTcs` Rust por defecto**.
El núcleo y la partida habitual siguen funcionando sin .NET. El host compila un
script elegido explícitamente con Roslyn y conecta sus límites, mensajes y órdenes
de freno a `LiveDriveSession`. El visor consume únicamente `session.etcs_status()`;
esa lectura devuelve el último estado, sin ejecutar código ni hacer IPC.

## Decisión y alcance

El proceso separado permite usar Roslyn actual en Linux y cerrar un host bloqueado
sin cargar CLR dentro de Bevy. Agrega un intercambio local por quantum y un runtime
opcional. C (CLR incrustado) complicaría la inicialización, el diagnóstico y la
recuperación; D (prototipo del contrato en Python) no ejecutaría los scripts C#.
A mantiene el camino ligero para equipos y contenido que no usan scripts.

Referencia del API: Open Rails **1.6.1**, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`, archivo
`Source/Orts.Simulation/Common/Scripting/TrainControlSystem.cs`.
`tools/or-tcs-host/TrainControlSystem.cs` declara el subconjunto implementado:
`Initialize`, `Update`, `HandleEvent`; reloj, velocidad, límite de ruta, hasta 32
señales normales y 32 cambios de límite por delante, próxima señal distante,
límite/intervención, freno de servicio y emergencia. `HostMessage`,
`HostDeltaTimeS` y `HostNextStopDistanceM` son extensiones identificadas del host.

**No es el API completo de OR.** Todavía faltan funciones de señales genéricas,
límites propios de SIGCFG, curvas ETCS originales, pantógrafos/alimentación,
API completo de `ETCSStatus` y persistencia `Save/Restore` del script. Un miembro
ausente produce error de compilación; no se reemplaza con una respuesta vacía.
`TrainSpeedLimitMpS` recibe el límite efectivo de la sesión; `CurrentPostSpeedLimitMpS`
recibe el límite de vía en la cabeza. `TrainMaxSpeedMpS` usa el mínimo de las
velocidades máximas declaradas por las locomotoras, separado del límite de ruta.
`NextSignalAspect(index)` conserva los ocho aspectos nativos; señales `INFO`
(contrapesos) no entran en la tabla normal. Se traduce expresamente SIGASP al
enum `Aspect` de OR, cuyos valores y orden son distintos. Los cuatro `TCSEvent`
expuestos también conservan los valores numéricos originales.
Se pueden ejecutar scripts compatibles con ese contrato, incluido el fixture;
no se promete ejecutar cualquier script de un paquete OR.

## IPC JSONL v1

Entrada/salida estándar del proceso, un objeto UTF-8 por línea, máximo **64 KiB**.
Cada respuesta debe repetir `version: 1` y el `seq` exacto de su solicitud.
La primera solicitud tiene `kind: "initialize"`; las siguientes, `"tick"`.
`context` contiene `time_s`, `dt_s`, `speed_mps`, `speed_limit_mps`,
`next_signal_distance_m`, `next_signal_stop` y `next_stop_distance_m`.
Las distancias ausentes son `null`. El tick usa el quantum de física de la sesión:
normalmente **0,05 s / 20 Hz**; escenarios con quantum menor lo conservan.

El contrato extendido agrega `train_max_speed_mps`, `current_post_speed_limit_mps`,
`signals: [{distance_m, aspect}]`, `distance_signal` y
`speed_posts: [{distance_m, speed_limit_mps}]`. Las tablas tienen como máximo
32 entradas, distancias finitas no negativas y orden ascendente. Índices 0–31:
una señal ausente devuelve `Aspect.None` / `float.MaxValue`; un poste ausente
devuelve −1 m/s / `float.MaxValue`. Índices fuera de rango producen error del
host. Se admiten los campos antiguos de JSONL v1 para el fixture anterior; una
distancia ausente ya no se transforma en una señal clara ficticia.

`events` entrega hasta 64 entradas por tick: `acknowledge` con `message` y `menu`
con `action`. ACK llama AlerterPressed/Released; menú llama
GenericTCSButtonPressed/Released. El DMI entrega esos eventos a la sesión; las
acciones de menú se identifican mediante el nombre Rust de `MenuAction`, y la
entrada numérica con `data:valor`. El ejemplo de aceptación envía `restrict`.

`status` devuelve `allowed_mps`, `next_limit_mps`, `intervention_mps`,
`emergency_brake`, `full_brake` y hasta 32 `messages` con texto, posibilidad de
reconocimiento y estado reconocido. Rust valida números finitos entre 0 y 200 m/s
y textos de hasta 1024 bytes. Los scripts retienen su estado entre ticks.
El host retiene límites/frenos hasta que el script los cambia explícitamente.

Inicialización/compilación: plazo de 10 s. Cada tick: 250 ms configurables en
`ScriptHostConfig`, incluyendo la escritura al proceso si deja de leer stdin.
Salida prematura, JSON inválido, secuencia errónea, excepción
o plazo vencido cierran el host, muestran `TCS host failure` en el estado ETCS y
mantienen intervención: regulador cero y freno completo en el siguiente quantum.
No hay cambio silencioso al TCS Rust. El fallo inicial impide abrir la sesión.
El proceso se termina y recoge al cerrar la sesión. Es aislamiento de fallos,
no un sandbox de permisos: se selecciona un script local de confianza.

## Comprobación Linux

Necesita el SDK [.NET 10](https://dotnet.microsoft.com/en-us/download/dotnet/10.0),
Roslyn incluido en el SDK, Rust y Python. No usa paquetes NuGet externos.

```bash
bash scripts/check_tcs_host.sh
# SDK instalado en otra carpeta:
OPENRAILSRS_DOTNET=/ruta/dotnet bash scripts/check_tcs_host.sh
```

Compila los fixtures `MinimalTcs.cs` y `NativeLookaheadTcs.cs`, verifica el protocolo real
y lo consume desde el simulador Rust. Prueba ACK, menú, cambio a 18 km/h y freno
físico por exceso de velocidad. Comprueba los ocho aspectos, distancias e índices,
postes, velocidad máxima, enumeraciones nativas y entradas inválidas; una señal
original de Chiltern ordenada a Alto llega al script y aplica freno físico.
Los tests Rust cubren muerte del proceso, timeout
de respuesta y de escritura,
respuesta enorme y secuencia errónea, comprobando intervención sin fallback.

## Prueba en una partida

```bash
dotnet build tools/or-tcs-host -o tmp/or-tcs-host
OPENRAILSRS_TCS_HOST_DLL="$PWD/tmp/or-tcs-host/OrTcsHost.dll" \
OPENRAILSRS_TCS_SCRIPT="$PWD/docs/fixtures/tcs/MinimalTcs.cs" \
OPENRAILSRS_TCS_TYPE=MinimalTcs ./scripts/run_chiltern_service.sh
```

La selección se hace al abrir la sesión; no se cargan automáticamente `.cs` del
contenido. En una cabina con DMI ETCS, aparecerá `C# TCS listo: confirmar` y la
intervención de freno. Reconocer el mensaje libera esa intervención; el exceso
de velocidad vuelve a frenar. El Pullman de Chiltern no incluye un DMI ETCS:
para el fixture allí usá la prueba headless, o una locomotora con pantalla ETCS.
Con host C# activo, cargar un guardado se rechaza explícitamente porque el estado
del script no se serializa. Los guardados del TCS Rust mantienen su comportamiento.

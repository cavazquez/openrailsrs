# Neumática original con controlador de tren

El soporte se activa por `Air_single_pipe`, equipo `Triple_valve` sin válvulas
adicionales no soportadas y un `Brake_Train` MSTS reconocido en la locomotora
principal. No depende del nombre del tren, del archivo ni de una bandera global
de emergencia avanzada. El equipo de una formación encabezada por EP conserva
su ruta anterior. Un depósito principal para alimentar la tubería no implica
que exista un mando de freno independiente.

La inicialización sigue `Train.UnconditionalInitializeBrakes` y
`AirSinglePipe.Initialize` de OR 1.6.1: actualiza el controlador durante 1000 s,
limita el EQ inicial a servicio completo y deriva aire de cilindro, depósito
auxiliar y tubería con los volúmenes authored. Por ejemplo, máximo de 90 PSI,
reducción de servicio de 26 PSI, relación 2,5 y mando inicial 0,7 producen
64 PSI de tubería/auxiliar, 90 PSI de control/emergencia y 65 PSI de cilindro.
Estos estados se calculan a partir de parámetros; no se copian de una traza.

Durante la conducción se usa el valor real del mando. `SetPercent` en OR puede
reescalar la demanda de interfaz alrededor de la muesca de emergencia. Con las
muescas 0/0,2/0,7/0,8, el valor real 0,552 corresponde a una fracción suave
0,704 y objetivo de EQ de 69,92 PSI. La posición de retención conserva el EQ al
reducir demanda con la configuración nativa `GraduatedRelease=false`. Liberar
el mando recarga a 90 PSI a través del depósito principal finito y la tubería.
El compresor mecánico usa RPM reales / RPM máximas, además de su histéresis.

Las ecuaciones proceden de los archivos fijados de OR 1.6.1:

- `Train.cs`, `UnconditionalInitializeBrakes` y `PropagateBrakePressure`.
- `MSTSBrakeController.cs`, `UpdatePressure`, `IncreasePressure` y `DecreasePressure`.
- `MSTSNotchController.cs`, selección de muesca y `GetNotchFraction`.
- `AirSinglePipe.cs`, inicialización, propagación entre volúmenes y actualización de válvulas.
- `MSTSLocomotive.cs`, depósito principal y compresor mecánico.

La API `BrakeSystem::native_start_handle()` informa si el builder ya preparó
el estado authored. `reset_native_plain_brakes(handle, immediate_release)`
repite esa inicialización formal; devuelve `false` si el equipo o el mando no
son válidos. La física evita el mapeo genérico presión/fuerza para estos mandos.
El estado guardado conserva la posición previa y la reducción mínima del
controlador, los depósitos y las presiones. La firma histórica normaliza sólo
candidatos plain inactivos y metadatos opcionales ausentes.

El fixture propio `plain_train_control.eng` y cinco regresiones de parser /
subsistema cubren inicialización, mando real, retención, depósito finito,
ausencia de freno independiente, guardado, exclusión en formación EP y equipos
distintos de compresor/depósito bajo un mismo mando. Las cinco pasaron en la
validación dirigida del 16 de octubre de 2026. El mismo comando
(`cargo test --locked -p openrailsrs-formats -p
openrailsrs-sim --all-features --lib --no-fail-fast -- --test-threads=1`) pasó
275 pruebas de formatos y 140 de simulación, incluida la regresión del máximo
interior de eficiencia del convertidor. Evidencia local:
[log dirigido](../tmp/twenty-20261015/subsystem-tests01.log), SHA-256
`4f7b4d8794253297afb0ba79691da0a4b56004f70830a827ca3a10544232be00`, y
[recibo de ejecución](../tmp/twenty-20261015/subsystem-tests01-result.json).
El recibo también conserva por separado el fallo del ensayo de descarrilamiento
desde parado `fromrest-tests08`; ese fallo no se cuenta como aprobación.

El ensayo de liberación mantiene el umbral de 89,9 PSI y usa 180 s de
asentamiento: sólo llenar auxiliares y tuberías requiere 11,8236 PSI·m³, es
decir, al menos 56,30 s con el caudal máximo de entrada antes de los
estrangulamientos nativos. Las ecuaciones no se cambiaron para ampliar ese
horizonte. Esta ejecución dirigida se conserva separada de la puerta global
del workspace.

El replay LIRR 10, medido después del build 11, aprobó los seis límites del
protocolo original contra la captura despausada 07: 32.583 muestras a 20 Hz,
1629,10 s y 31.361,20 m de referencia. El RMS de tubería fue
**0,000418428 psi** y el de cilindro **0,000115632 psi**, ambos frente al límite
previo de 5 psi. La fuerza de freno dio **72,907216 N RMS**, frente a 5000 N.
También aprobaron velocidad, odómetro y fuerza motriz. Los números completos,
los binarios y sus 752 entradas de compilación están en la
[evidencia del recorrido](fixtures/compatibility/twenty-20261015/lirr-full-service-replay10/README.md)
y su [comparación](fixtures/compatibility/twenty-20261015/lirr-full-service-replay10/comparison.json).

Ese resultado corresponde a los **siete vehículos originales de
SFexpress-500 y las dos plataformas de Brooklyn–Hempstead**. El conductor de
Open Rails cumple las tareas originales; Rust reproduce sus mandos reales y
recorre la vía con estados físicos propios. La puntuación y los objetivos ACT
están desactivados expresamente durante la comparación física para no cortar
la medición. No se certifican por este resultado ni el tráfico completo ni
todos los sistemas de todos los vehículos. La captura 05 pausada y los
replays fallidos siguen preservados con su contexto; el replay 07 prematuro
se marca inválido en el historial. No se ampliaron tolerancias.

Quedan fuera de este subconjunto los controladores scripted arbitrarios,
válvulas auxiliares rechazadas por el parser, sobrecarga y sincronización DP
fuera de la unidad principal. Esta implementación usa la configuración nativa
de liberación no graduada; no afirma equivalencia para todas las opciones de OR.

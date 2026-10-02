# Referencia nativa del servicio Chiltern local

Captura del motor original Open Rails **1.6.1**, sin recompilar ni modificar
sus DLL. Usa la misma formación Birmingham Pullman de ocho coches, recorrido
Northolt Park → South Ruislip → West Ruislip y horario del servicio Bevy.
Las tres tareas de estación del propio Open Rails terminaron correctamente.

`capture/trace.csv` contiene 15 003 muestras a 20 Hz, desde t=0 hasta 750,1 s,
con semilla 0 y 7054,788 m recorridos. La cabeza comienza en el extremo de
salida del andén de Northolt, con error inferior a 0,1 m. El PAT nativo ancla
la **cola**: se genera su punto inicial retrocediendo la longitud de formación
medida por el `Traveller` original y conservando el vector de aproximación 94.
`content/` contiene únicamente los tres archivos ACT/SRV/PAT generados.

`manifest.json` registra hashes del cliente C#, DLL, ENG/WAG, TDB, PAT original,
TRK, catálogos tsection, SIGCFG/SIGSCR, escenario y archivos capturados.
`HeadlessCapture.cs` conserva el cliente exacto de esta captura. Las DLL y
el Content completo se resuelven desde la instalación local y no se duplican
en Git. La captura emplea una copia privada de Wine, sin compartir registros
ni enlaces a Documents/Desktop del usuario, y rechaza colisiones de nombres.

## Reproducción

Desde la raíz del repositorio, con el Content y la instalación originales:

```bash
python3 scripts/capture_chiltern_service_or.py --out-dir tmp/or-service-new
python3 scripts/verify_chiltern_service_capture.py tmp/or-service-new
python3 scripts/capture_chiltern_service_or.py --out-dir tmp/or-replay-new \
  --replay examples/baselines/chiltern_local/driver.csv
python3 scripts/verify_chiltern_service_capture.py \
  --replay tmp/or-replay-new
```

Los argumentos `--route-root`, `--installation-root`, `--wine-source-prefix`
y `--wine-prefix` permiten elegir otras ubicaciones. Cada salida debe ser nueva.
El compilador .NET Framework se obtiene de la instalación privada existente.

`reproducibility.json` registra una segunda ejecución con las órdenes
exportadas: velocidad, odómetro, vector/posición, mando nativo, presiones,
límite efectivo y masa resultaron exactamente iguales en las 15 003 muestras.
La traza también se repitió byte por byte en dos capturas de servicio.

## Columnas y contratos

Tiempo, posición, odómetro, velocidad y presión tienen sus unidades en el
nombre de columna: segundos, metros, m/s y PSI. Se registra además el aspecto
y distancia de próxima señal, límite efectivo y sumas directas de los campos
de fuerza de los coches. Estas sumas conservan las convenciones locales de
Open Rails; no se reinterpretan como aceleración del tren.

`brake` es demanda EP de servicio en [0,1]. Se aplica en el rango nativo del
regulador de freno [0,05;0,45], con `native_brake_handle` y `cylinder_psi`
registrados por separado. El cilindro alcanza 45 PSI. `driver.csv` contiene
estas órdenes, nunca la presión medida; traslada la orden registrada en
t+dt al comienzo de su intervalo t y conserva el horizonte terminal completo.
La muestra inicial de la traza precede a la primera orden del cliente.

`door_state` es el estado real del motor original; `doors_command_open`
registra la orden. Se exige parada dentro de 10 m a ≤0,1 m/s, embarque con
puertas abiertas, horario cumplido, puertas cerradas y las tres tareas nativas
aprobadas. `outcome.json` distingue el instante en que el cliente habilita la
salida del `ActDepart` nativo, registrado cuando el tren comienza a moverse.
Los pequeños movimientos de acopladores al iniciar la parada se conservan.

La simulación original adapta su adhesión a la actualización a 20 Hz; los
ajustes físicos efectivos están registrados en `initial.json`. `capture.log`
conserva advertencias del Content sobre formas ausentes y scripts por defecto.
Esta captura verifica física y servicio; el HUD, la cabina, audio y exterior
requieren una comparación visual adicional.

## Objetivo físico completo todavía pendiente

```bash
python3 scripts/run_oracles.py --suite service --out-dir tmp/service-parity
```

Este comando **devuelve FAIL**: el replay Bevy presenta RMS de velocidad
7,5153 m/s, pico de 16,6450 m/s y error máximo de odómetro de 3029,15 m.
Los objetivos siguen siendo 0,75 m/s, 2 m/s y 45 m, con cobertura temporal
completa. El regulador coincide; el freno coincide salvo la muestra inicial.
El escenario de replay elimina intervenciones automáticas de paradas y
señales del runner antiguo para aislar las órdenes físicas originales.

La evidencia señala tres trabajos concretos: límites de velocidad por
posición, pendientes por sección/vehículo y especificación nativa del freno.
El poste de 35 mph del vector 96 actúa cerca de su final; el importador actual
lo aplica a todo el vector. La física Bevy usa pendiente cero en este mapa.
La formación original tiene siete frenos EP y uno `AirSinglePipe`; el perfil
convertido identifica el sistema por clase de vehículo y conserva fuerzas
de zapata como fuerzas nominales. Open Rails aplica presión de referencia y
fricción Karwatzki de la zapata. No se ajustaron umbrales ni la traza original
para ocultar estas diferencias.

El servicio Bevy con conducción por realimentación completa las tres paradas;
esa aprobación funcional y los cuatro oráculos breves de `chiltern.toml`
se mantienen separados del nuevo objetivo físico completo.

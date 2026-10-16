# Siete sistemas físicos: implementación y aceptación pendiente

La implementación se registró antes de ejecutar la fase conjunta. Ahora existen
capturas originales nuevas hasta `tmp/twenty-20261015/physical-native-05`, con
manifiesto y recibos de aislamiento. Las cinco tablas originales ya pasaron
la comparación Rust. El primer replay cerrado de vapor completó 1600 pasos,
pero falló sus límites previos; el diagnóstico y los puertos pendientes se
conservan en `docs/STEAM_THERMODYNAMICS_AUDIT.md`. Las tres comparaciones
CPU (tablas, válvulas y propagación de tres coches) pasaron con los límites
previos en `physics-native-tests03`. La aceptación manual sigue en validación. Este documento no da por cerrados
#221, #223, #224, #234, #324, #325 ni #329.

Los archivos de `examples/physics_lab` son contenido propio acotado. No contienen
modelos, sonidos ni vehículos originales copiados. Los equipos nuevos son
opcionales: un perfil sin esos campos conserva su camino de física anterior.

## Qué entra en el tick

- **#221:** `ORTSRSAdvancedEmergency (1)` habilita válvulas y reservorios de un
  sistema de aire de un tubo. Las presiones de la tubería se intercambian según
  volumen entre coches; servicio y emergencia usan fuentes distintas. La
  válvula local observa el gradiente, descarga el depósito de emergencia, abre
  su escape durante el tiempo declarado y vuelve a cargar al liberar. Tras
  cerrar el escape temporizado, el rearme acelerado original devuelve aire
  del cilindro/auxiliar a la tubería cuando ésta supera20psi. El freno
  independiente usa el circuito finito de la locomotora y su tubería3. Su mando
  no abre la tubería1. Se rechazan equipos de dos tubos, reservorios de suministro
  y otras válvulas que este subconjunto no implementa. Los perfiles anteriores
  no activan el modelo nuevo por llevar sólo la etiqueta `air_single_pipe`.
- **#223:** cinco tablas de saturación y su interpolación Single proceden de
  `SteamTable.cs`, OR1.6.1, commit
  `d16e670da333d26d2edfc97d5631a19dadf49ce5`. La caldera nueva conserva masa y
  energía interna; inyección, calor de fuego, pérdidas, consumo y seguridad
  pasan por el mismo depósito. Retirar vapor de un auxiliar también modifica
  ese balance. El volumen finito y el ciclo de cilindros son un modelo propio;
  compartir tablas no demuestra equivalencia del ciclo original completo.
  `startup="or_hot"` inicia a máxima presión menos5psi y `startup="or_cold"`
  a66% de la máxima, con aritmética Single como el original. Masa y energía se
  inicializan a esa misma presión. La opción ausente conserva el arranque
  previo a presión de trabajo; las calderas legacy no cambian. En STF se
  declara `ORTSRSBoilerStartup ( or_hot )` dentro del opt-in de termodinámica.
- **#224:** cada locomotora conserva su caldera, fuego, ténder y controles.
  `local_regulator=None` sigue el mando compatible; `Some(0..1)` permite su
  regulación local. Ambos respetan aislamiento operativo, puertas, inversor,
  protección, emergencia y corte físico. Cerrar el regulador no apaga el fuego
  ni inventa agua. El compound propio separa HP, receptor y LP, y ofrece una
  válvula de arranque. El receptor y sus balances no se presentan como el
  modelo Vauclain nativo de OR.
- **#234:** un vehículo y un tramo deben declarar el mismo `system`. La entrada
  exige velocidad de engrane y tiempo de acoplamiento; incompatibilidad o una
  entrada demasiado rápida registran un fallo hasta retirar la solicitud.
  El contacto positivo sustituye la fuerza rueda/carril del mismo motor,
  limitada por fuerza/potencia. Su freno y el de sobrevelocidad actúan como
  frenos mecánicos y no se recortan por adherencia rueda/carril. Fuera del tramo
  vuelve la tracción convencional. No hay un solver equivalente en OR1.6.1.
- **#324:** las consecuencias son optativas por material. Pueden activarse por
  el indicador nativo de trepado, exceso sostenido de Nadal o descarga de rueda,
  con velocidad mínima, tiempo y límites declarados. El estado registra causa,
  posición, momento, velocidad y fuerzas; corta tracción, activa emergencia y
  aplica arrastre fuera de vía. No simula colisiones con el terreno. Desactivar
  consecuencias conserva el evento, que también permanece al guardar.
- **#325:** irregularidad espacial determinista por tramo, semilla, longitud
  de onda y amplitud; dos carriles y transición suave. Suspensión, topes,
  contacto unilateral y carga normal evolucionan físicamente. La carga afecta
  adherencia, deslizamiento y riesgo; la reacción sobre el perfil introduce
  fuerza longitudinal. Un apoyo compartido se integra una vez y distribuye
  carga a ambos cuerpos. La representación vertical usa este estado. No se
  usa la vibración de cámara de OR como oráculo de fuerzas.
- **#329:** curvas de convertidor declaradas gobiernan relación de par,
  capacidad, pérdidas, inercia del motor, cambios e histéresis de bloqueo.
  La carga de bomba cambia RPM y consumo; las pérdidas producen calor y pueden
  disparar protección. La etiqueta nativa `Hydraulic` por sí sola no aporta
  esas curvas y no activa esta extensión. No se atribuye al original un solver
  que no tiene.

Los estados de equipo permanecen ligados a índices/identidades del catálogo al
cortar, recoger y reordenar secciones. La cremallera conserva también estados de
vehículos estacionados, y sólo actúa en los vehículos de la sección activa.
La marcha atrás llama al integrador compartido `physics::step_backwards` sobre
la vía real: conserva posición global, regiones de contacto y pendiente al
cruzar aristas. La orientación del material y el sentido del movimiento son
entradas diferentes para el muestreo de irregularidades.
Los campos de equipo configurados deben estar completos y ser válidos antes
de restaurar una partida. Los contenidos originales permanecen de sólo lectura.

## Contenido y comprobaciones escritas

`scenario_hydraulic.toml` combina convertidor y válvulas neumáticas;
`scenario_double_steam.toml` combina calderas simple y compound;
`scenario_rack.toml`, `scenario_vertical.toml` y `scenario_derailment.toml`
permiten explorar cada equipo. La vía propia tiene3km y pendiente; el caso de
descarrilamiento usa la curva propia del laboratorio de bogies.
Para conducir hasta esa curva desde parado, elegir en el menú **Descarrilamiento:
aproximación y exceso en curva** (`scenario_derailment_drive.toml`). Este ejercicio
separado añade una recta de aproximación de 2 km antes de la misma curva; conserva
los umbrales y el ejemplo compacto anterior. No establece una velocidad inicial.
Usa `consists/curve_risk.con`, con la locomotora propia `risk_loco.eng`
(2 MW, 80 t, máxima de 160 km/h) y un coche propio de 30 t. Son definiciones
físicas de laboratorio, sin modelos, sonidos ni texturas originales incorporados.
La formación anterior, de menor potencia y velocidad máxima, podía completar
la curva sin alcanzar un incidente; eso no justificaba reducir sus umbrales.

`sim/tests/physical_extensions.rs` escribe conducción con aceleración real,
balance de masa/energía y recursos, circuitos independientes, transición de
convertidor, freno positivo en pendiente, semilla reproducible, apoyo compartido,
evento persistente, guardado/restauración y rechazo de equipos manipulados.
También integra esfuerzo e impulsos, consumo de vapor/carbón y calor por
identidad en las dos locomotoras, comprueba la suma de fuerzas y ausencia de
tracción en el vagón. Verifica regulador común cerrado con regulación local, neutro, puertas,
retención, aislamiento y emergencia. Son ensayos del modelo propio, sin salida
OR inventada. La raíz mantiene además `twenty_joint_physics` para la integración
con patios y autoridad multijugador.

Al iniciar la fase conjunta, ejecutar primero los targets dirigidos; no iniciar
otra compilación en paralelo con el gate de la raíz. El perfil compartido es:

```sh
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_DEV_STRIP=symbols
export CARGO_PROFILE_DEV_INCREMENTAL=false
export CARGO_PROFILE_TEST_OPT_LEVEL=1 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_TEST_STRIP=symbols
export CARGO_PROFILE_TEST_INCREMENTAL=false
cargo test -p openrailsrs-sim --test physical_extensions
cargo test -p openrailsrs-sim --test twenty_joint_physics
```

La fase de implementación no ejecutó estos comandos. Un fallo exige conservar
su salida e identificar código, configuración o recurso faltante.

## Nuevas referencias reales preparadas

`oracles/physical-systems-seven-protocol.json` declara límites **antes** de
capturar. Conserva los hashes de DLL y fuente original fijados. El capturador
`scripts/capture_physical_systems_reference.py` y
`tools/openrails-reference/PhysicalSystemsReference.cs` están preparados para:

1. Consultar las cinco tablas originales en157 presiones, incluidos puntos
   próximos a cambios de intervalo.
2. Llamar `AirSinglePipe.Parse/Initialize/Update` en el perfil numérico RF
   BluePullman original y dos variantes explícitas admitidas por OR. Cada caso
   recorre servicio gradual, liberación, freno independiente, emergencia y
   rearme; la tubería1/3 es una entrada prescrita de subsistema.
3. Llamar `PropagateBrakePressure` y `Update` originales con tres coches y
   sus volúmenes. Se prescribe sólo la presión de cabeza una vez por tick.
   Las presiones restantes, cilindros y depósitos evolucionan. Este ensayo
   separa propagación de la válvula de alimentación/compresor de una locomotora.
4. Con ENG y PAT originales explícitos, ejecutar `Simulator.SetExplore/Start`
   y1600 llamadas reales a `Simulator.Update(0.05)` para fuego al ralentí,
   carga, inyección, deriva y frenada. Sólo prescribe controles; registra
   parámetros estáticos inicializados y producción, presión, calor, consumo,
   inyección, ténder, fuerzas y movimiento. La raíz selecciona ese contenido.

Las propiedades admiten0..300psi; el límite de seguridad del perfil Rust debe
quedar dentro de ese dominio. Tolerancias previas: tablas según protocolo;
presiones neumáticas0.005psi, fuerza50N y estado de válvula exacto. El diagnóstico
SteamDrive usa0.1bar,0.1kg/s de producción/vapor,0.05kg/s de inyección/combustión,
1kg de masas,100N,0.1m/s y2m, más los restantes límites declarados. No se amplían
después de ver errores.

La captura05 contiene157 consultas,6420 pasos de válvulas,6420 pasos de redes
de tres coches y1601 muestras de SteamDrive original. La captura04 se conserva
intacta:05 agrega campos originales de inventario, temperatura, densidad y
potencia sin cambiar ningún campo anterior de la trayectoria. Los intentos
previos quedan registrados, incluidos errores de arnés y la red que carecía de
contexto de locomotora para habilitar la válvula de emergencia original.

El capturador sólo admite directorios nuevos y usa `scripts/oracle_wine.py`, prefijo privado marcado,
`Graphics=null`, `ShowCrashDialog=0`, ausencia de displays/audio heredados y
recibos de aislamiento y cierre. No invocar Wine/csc/helper fuera del wrapper.
La carpeta de captura archiva el helper, script, protocolo, entradas, hashes y
los resultados originales, con estado `captured_not_compared`.

La copia promovida `docs/fixtures/compatibility/twenty-20261015/physical-native05`
conserva exactamente el manifiesto y sus once archivos. No contiene runtime,
DLL, ENG/PAT ni recursos originales. El estado histórico `captured_not_compared`
del manifiesto queda intacto; `execution.json` separado identifica el binario,
el log y los tres pases de comparación CPU posteriores. Los trece archivos
tienen pines nuevos; los anteriores no cambian.

Los tres ensayos usan esa copia por defecto, sin Wine ni contenido instalado,
y fallan si falta evidencia. `OPENRAILSRS_PHYSICAL_SYSTEMS_CAPTURE` permite
seleccionar explícitamente otra captura completa. Ya no llevan `ignore`.

```sh
cargo test -p openrailsrs-sim --test physics_seven_native
cargo run -p openrailsrs-sim --example steam_native_compare -- \
  --capture-dir /ruta/captura-nueva --out-dir /ruta/rust-steam-attempt01
```

`steam_native_compare` carga ENG/PAT/vía originales sin modificarlos y conduce
`LiveDriveSession`. Aplica un overlay en memoria: parámetros estáticos
inicializados del vehículo y eficiencias propias registradas antes. Cada
simulador inicializa su estado; se preservan también los errores iniciales.
No introduce presión, velocidad, fuerza, calor ni caudal observados en el tick
Rust. Emite CSV, métricas por fase, diferencias, configuración efectiva, hash de
ejecutable/fuentes y recibo del árbol de trabajo real. Sale con error si excede
límites o no completa el horizonte. El reporte siempre mantiene
`full_native_steam_cycle_parity_claim=false`; un fallo de ciclo queda abierto
para resolverlo, aunque las tablas y la conservación propia pasen.

La ejecución previa del recipiente propio, `steam-compare02`, completó1600 pasos,
conservó las seis trazas humanas y volvió a fallar los límites previos. Sus
resultados compactos están en
`docs/fixtures/compatibility/twenty-20261015/steam-diagnostic02`; no contienen
diff del árbol ni contenido original. Los14 ensayos propios de
`physical_extensions` pasaron, incluido el reparto de fuerza, consumo y calor
por locomotora. Esos pases no certifican el compuesto original, que no se
capturó. Después se incorporó un [perfil nativo separado](NATIVE_STEAM161.md):
sus tablas, los subsistemas caliente/frío y la comparación cerrada de80s pasan
los límites previos. Esta aceptación corresponde al perfil tanque/simple/manual
documentado, además de los balances del recipiente propio; no acredita todos
los sistemas de vapor originales.

El inventario de agua se toma de `CombinedTenderWaterVolumeUKG`, multiplicado
por el `WaterLBpUKG` original y la conversión de libras a kg. El campo visible
`CurrentLocoTenderWaterVolumeUKG` se inicializa después y no representa el
inventario físico en la fila0. `production_kg_s` compara capacidad térmica
`Qdot/h_steam` con `EvaporationLBpS`; `net_evaporation_kg_s` registra por separado
el cambio de masa líquida más inyección observada. Ambas conservan el límite
previo de producción0.1kg/s. La capacidad térmica del modelo propio no se
reemplaza con el caudal original para forzar equivalencia.

## Primera ejecución gráfica de conducción

El visor normal build04 ejecutó tres casos aprobados desde reposo: hidráulica,
frenado por cremallera y salida de cremallera. Las tres imágenes se revisaron
después de pausar y restaurar el `SessionSnapshot` por JSON. En **B → Tracción**
se leen RPM de motor/bomba/turbina, ratios, par, potencias y pérdidas, calor,
temperatura y consumo del estado real. El freno de piñón produjo trabajo físico
y la salida mostró `Disengaged` sin fuerza transmitida por cremallera. Los
auxiliares inferiores de algunos paneles quedan en el área desplazable; no se
ensayó el desplazamiento con rueda.

El cuarto caso, irregularidad activada, **falló la aceptación gráfica**. El
solver produjo cargas variables y desplazamientos verticales, y sus indicadores
se leen en el panel; el tracker de carrocería permaneció vacío y registró
desplazamiento renderizado cero con vibración cosmética apagada. La lectura del
código confirmó un retorno temprano que omitía la suspensión física vertical.
Ese fallo se conserva; motivó la reparación y la repetición documentadas más
abajo. La hipótesis inicial de ausencia de raíz del modelo no se confirmó.

La ejecución se detuvo allí: irregularidad desactivada y descarrilamiento no
se capturaron. No se presentan seis pases. El [recibo y las cuatro imágenes](fixtures/compatibility/twenty-20261015/physical-driving-gpu01/README.md)
conservan hashes, controles, valores, resultado parcial y fallo original. Se
utilizó Vulkan en RX 7600 y los cuatro visores/compositores cerraron con código
0; había trabajo CPU concurrente, por lo que no se afirma rendimiento aislado.
Son definiciones y geometría de laboratorio propias, sin recursos originales
distribuidos ni un nuevo oráculo OR para estas extensiones.

## Reparación de suspensión y conducción hasta un incidente

El [segundo intento con build05](fixtures/compatibility/twenty-20261015/physical-driving-gpu02/README.md)
aprobó irregularidades activadas y desactivadas después de pausa y restauración
JSON. Con movimiento cosmético apagado, la suspensión activa registra dos
carrocerías, 4,480 mm de desplazamiento físico máximo y veinte piezas de rodaje
compensadas. Al desactivarla, el desplazamiento físico renderizado vuelve a
cero. Las PNG muestran las opciones y cargas legibles; como el panel tapa el
tren, la oscilación y poses de ruedas tienen su evidencia CPU/ECS separada.

El descarrilamiento de ese intento produjo un evento real, pero el guion falló
al no observar una velocidad positiva entre arranque y detención. Se conservó
ese resultado. El [reintento estricto](fixtures/compatibility/twenty-20261015/physical-driving-gpu03/README.md)
agregó únicamente una observación intermedia y pasó las mismas aserciones, sin
cambiar mandos, física ni umbrales. Desde reposo llegó a `WheelUnloading` a los
83,60 s simulados y 104,7 km/h; tras restaurar y desactivar consecuencias se leen
la causa, fuerza lateral, carga mínima, emergencia aplicada y esfuerzo cero.
Los visores y compositores de ambos intentos terminaron con código 0. La
restauración es un ciclo JSON dentro del proceso, no una prueba completa del
menú de guardados en disco. Se mantiene el alcance de laboratorio propio.

## Aceptación manual posterior

La raíz debe registrar equipo, escenario, controles, causa/estado y observación
de cada caso. No alcanza con una captura de pantalla sin trayectoria física.

1. Vapor doble: en Tracción seleccionar cada caldera; variar pala, tiro,
   inyectores y regulador local. Observar depósitos separados y comparar ambos
   consumos. Con compound, cambiar la válvula de arranque y revisar HP/receptor/LP.
   Abrir puertas, poner neutro y aplicar emergencia: ningún regulador local
   debe evitar esos interbloqueos. Pausar conserva el balance; guardar y cargar
   debe continuar la misma evolución.
2. Aire: aplicar independiente con servicio liberado y observar que la presión
   de tubería1 no cae por ese mando. Luego servicio, emergencia y liberación:
   ver propagación, depósito/escape local y tiempo de rearme, sin un salto de
   fuerza prescrito.
3. Hidráulica: arrancar desde parado, aumentar carga y pasar cambios. Ver RPM de
   bomba/turbina, par, pérdidas, temperatura y combustible. Soltar regulador y
   verificar enfriamiento. No confundir sonido/animación con evidencia mecánica.
4. Cremallera: engranar dentro de límite, recorrer entrada/salida y frenar en
   pendiente. Probar solicitud incompatible o entrada rápida y retirar la
   solicitud para rearmar. Comparar contacto positivo con rueda/carril mojado.
5. Irregularidad: repetir semilla y controles; comparar cargas y movimientos.
   Desactivar/reanudar la opción, invertir y comprobar apoyo compartido. La
   oscilación visual debe corresponder al estado, sin girar ruedas/bogies para
   simular una fuerza inexistente.
6. Descarrilamiento: en **Nueva partida → Ejemplos**, elegir
   **Descarrilamiento: aproximación y exceso en curva**
   (`examples/physics_lab/scenario_derailment_drive.toml`). Comenzar detenido,
   poner adelante, soltar los frenos y acelerar por la aproximación de 2 km.
   Este ejercicio selecciona la formación propia de 2 MW y 160 km/h; cambiarla
   por otra menos potente puede evitar que se alcance la condición de riesgo.
   Primero respetar el límite de la curva; repetir el ejercicio sobrepasándolo
   de forma controlada, sin imponer velocidad por herramientas de diagnóstico.
   En **B → Tracción**, ver la causa, el corte de tracción y la emergencia.
   Guardar, cargar y desactivar consecuencias; el registro del evento debe
   conservarse. El ejercicio compacto `scenario_derailment.toml` comienza junto
   a la curva y se mantiene para pruebas locales; no acredita por sí solo la
   aceleración desde parado hasta un incidente. La captura GPU del ejercicio
   nuevo pasó con pausa, restauración JSON y consecuencias desactivadas; sus
   límites están registrados arriba. Esto no acepta aún una simulación de
   choque ni caída al terreno.

No ejecutar pruebas prolongadas ni reescribir las seis trazas humanas de
Chiltern para obtener pases. Las referencias históricas permanecen inmutables.

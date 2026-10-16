# Física, patios y multijugador: lote de veinte issues

Este lote comprende #217, #218, #220, #221, #223, #224, #234, #324,
#325, #329, #338, #344, #345, #346, #348, #397 y #407–#410.
La implementación de los veinte precede a la primera ejecución de pruebas,
como pidió el usuario. Un cambio escrito no es un criterio de cierre aprobado.

La referencia sigue siendo Open Rails 1.6.1. La cremallera, las fuerzas
verticales de irregularidades y las consecuencias opcionales de
descarrilamiento son extensiones del simulador Rust. Sus ensayos no se
presentan como capturas de una función inexistente en la referencia.

## Física y controles

- **#221:** emergencia neumática, depósitos, propagación y rearme. Hay que
  distinguir el freno de servicio, la emergencia y el independiente.
- **#223:** propiedades de agua y vapor y balance de masa y energía por
  caldera. Una tabla contrastada no certifica por sí sola la dinámica completa.
- **#224:** calderas y reguladores por locomotora y un perfil compound
  explícito; no se impone ese perfil a cualquier locomotora de vapor.
- **#234:** entrada y salida de tramos de cremallera, engrane solicitado,
  incompatibilidades, velocidad máxima y frenado mecánico propio.
- **#324:** causas físicas persistentes de descarrilamiento y consecuencias
  opcionales. Desactivar el modo conserva la causa del incidente. No hay un
  solucionador de choques contra el terreno.
- **#325:** dos carriles con irregularidades deterministas, contacto unilateral
  y suspensión vertical. El cuerpo visible toma su movimiento del estado
  físico; el ajuste de movimiento cosmético de la cámara sigue separado.
- **#329:** transmisión hidráulica declarada, convertidor, pérdidas, esfuerzo,
  RPM y consumo. Los diésel sin este equipo conservan su modelo anterior.

En **B → Tracción** aparecen los controles del equipo instalado. Las calderas
independientes permiten seguir el regulador común o recibir uno local. Los
controles de cada caldera afectan esa locomotora. Las opciones de irregularidad
y descarrilamiento aparecen cuando el escenario declara sus parámetros. Allí
también se muestran presión, energía, cargas, fuerza de cremallera y la causa
registrada de un incidente. Guardar y cargar debe conservar esos estados.

## Patios y horarios

- **#217:** encuentro y transferencia física de porciones, con esperas y
  diagnóstico de incompatibilidades.
- **#218:** reserva, despacho y devolución de material físico en depósitos.
- **#338:** secuencia AI de desacople, rodeo por otra vía y reacople.
- **#344/#345:** giro y traslación de un tablero que transporta sus vehículos;
  los puertos se bloquean durante el movimiento y sólo se puede salir por la
  vía alineada.
- **#346:** vagones libres, pendientes, selección de vías, retardadores,
  ocupación y contacto entre formaciones.
- **#348:** incidentes opcionales de vagones sin control, protección y
  recuperación. No se relajan las reglas normales de desacople del jugador.

Los ejemplos propios se descubren en **Nueva partida**. En **F9 → Patio, mesas
y clasificación** se dan las órdenes y se consulta el progreso. El panel de
patios pausa una partida local; cerrarlo permite avanzar a la maniobra. Los
modelos del tablero y de su carga usan el mismo estado de simulación y las
mismas transformaciones de origen que el resto del escenario.

El camino completo para estos escenarios es `LiveDriveSession` junto con
`LiveTraffic`. Los antiguos ejecutores por lotes y la cabina de terminal
rechazan una definición de patio que no pueden ejecutar; no la ignoran.

## Multijugador

**#407** integra dos conductores y despachador con servidor autoritativo;
**#408** reparte conductor y fogonero de una misma caldera; **#409** permite
observar y seguir servicios sin autoridad de conducción; **#410** conserva el
mundo, roles y concesiones al guardar y restaurar el servidor.

El acceso está en **F10 → Multijugador** y en el menú. La sala comienza en la
interfaz local. La guía de multijugador explica cómo abrirla a otro equipo,
guardar y reconectar. Cada participante necesita el mismo contenido. Los
controles locales de conducción, formación y despacho pasan por la autoridad
de la sala; los espectadores sólo reciben una vista pública acotada.

Abrir un panel del jugador no debe detener el reloj de los demás. Al
desconectarse un conductor se aplica una parada segura, y un fogonero ausente
cede al automático si el equipo lo admite. El perfil nativo de vapor manual
detiene el tren y cierra sus reguladores cuando pierde al fogonero.
La prueba conjunta debe comprobar permisos, latencia,
reconexión y recuperación, además de abrir la ventana.

## Capturas equivalentes: #397

`examples/visual_comparison/chiltern.json` declara seis estaciones, cabina y
exterior, formación, verano despejado a las 09:55, ventana de 1280×720 y cámaras.
Las tolerancias de cámara y sol se fijan antes de capturar.

Con un visor ya compilado y la ruta original instalada:

```bash
python3 scripts/capture_equivalent_views.py \
  --route-root /ruta/al/Content/ROUTES/Chiltern \
  --out-dir tmp/comparacion-estaciones-01
```

El modo habitual reproduce las referencias nativas inmutables y genera las
vistas Bevy. `--station northolt-park`, repetible, selecciona estaciones.
`--plan-only` registra el plan sin abrir un renderizador. `--capture-native`
junto con `--activity`, `--installation-root` y `--wine-source-prefix` captura
también Open Rails en una copia privada del runtime y del prefijo Wine. El
original no se modifica. Para una ejecución nueva se exige otra carpeta.

El resultado incluye `manifest.json`, `comparison.json`, `comparison.html`,
imágenes y logs. Los hashes identifican la configuración, ejecutables,
definiciones de vehículos, bases de vías y evidencia utilizada. No constituyen
una auditoría de todos los archivos de una instalación. La galería permite
revisar geometría, transparencias, materiales y recursos del autor. Pasar las
condiciones de captura no equivale a aprobar una similitud global de píxeles.

## Recorrido completo LIRR: #220

`oracles/lirr-full-service-protocol.json` fija la actividad original
Brooklyn–Hempstead, SFexpress-500 de siete vehículos, paso de 0,05 s y umbrales
antes del ensayo. El capturador ejecuta la actividad con las DLL originales de
Open Rails 1.6.1. Rust reproduce sus mandos, sin reemplazarlos por un controlador
capaz de ocultar la diferencia física.

El protocolo `lirr-full-service-live-protocol.json` conserva esos seis límites
y declara la partida despausada, tiempo despejado y adherencia determinista
antes del primer paso. Hay que seleccionarlo explícitamente para repetir el
contexto aceptado; el protocolo histórico conserva su contexto anterior.

```bash
python3 scripts/capture_lirr_full_service.py \
  --protocol oracles/lirr-full-service-live-protocol.json \
  --activity /ruta/al/Content/ROUTES/LIRR/ACTIVITIES/01Brooklyn-Hempstead.act \
  --consist /ruta/al/Content/TRAINS/CONSISTS/SFexpress-500.con \
  --out-dir tmp/lirr-completo-01
```

El comando requiere `openrailsrs` y el ejemplo `activity_trajectory` ya
compilados. Guarda controles, velocidad, odómetro, fuerzas y presiones a 20 Hz,
los tiempos de llegada y los hashes de entradas y salidas. No se acepta una
traza truncada, una formación diferente ni perder una estación. Los umbrales
de velocidad RMS (0,75 m/s) y odómetro máximo (45 m) no se amplían después de
ver el resultado. Los intentos fallidos se conservan.

El replay físico desactiva solamente la evaluación de paradas en su copia
temporal para que un error de trayectoria no corte los datos. No certifica
todos los eventos ACT, sonidos o scripts originales. El cumplimiento de la
última tarea nativa se registra por separado: algunas tareas de terminal se
completan al volver a salir.

## Evidencia y cierre

Las anotaciones de los issues deben enlazar las pruebas efectivamente
ejecutadas y sus resultados, incluidos los fallos. Sólo se cierran los que
cumplen todos sus criterios. Las instrucciones manuales y las capturas
complementan las pruebas automáticas; no se marca una escucha o inspección
humana como realizada por haber escrito una prueba.

## Índice de evidencia y estado del 16 de octubre

Los veinte issues tienen evidencia dirigida dentro del alcance descrito aquí.
El [batch físico14](fixtures/compatibility/twenty-20261015/physical-final14/result.json)
pasó Chiltern, cuatro ensayos cortos, Class 47, las tres paradas jugables, LIRR12
y vapor10 con sus límites previos. Este índice no declara cerrados los issues:
la puerta global final de la versión integrada todavía está pendiente. Las dos
regresiones contables de
**#224**, incluidos esfuerzos menores que 1 N, pasaron en la ejecución de
librerías de 1.774 pruebas. Un binario compilado y un subconjunto aprobado no
sustituyen la verificación completa.

- **#217, #218, #338, #346 y #348:** los veinte ensayos CPU de
  `operational_yards` comprobaron encuentros, depósitos, rodeo, clasificación,
  recuperación, errores y guardados. El [manual de patios](OPERATIONAL_YARDS.md)
  indica cómo abrir cada ejercicio y qué observar desde F9. El alcance de pools
  y maniobras es el de las definiciones declaradas; no abarca todos los dialectos
  de horarios de la comunidad.
- **#221:** las [capturas originales y su ejecución Rust](fixtures/compatibility/twenty-20261015/physical-native05/execution.json)
  distinguen servicio, emergencia, independiente y propagación por tres coches.
  Los [controladores neumáticos nativos](NATIVE_PLAIN_AIR.md) tienen pruebas
  adicionales de inicialización, depósitos y rearme. La aceptación de LIRR
  tiene su propia referencia y comparación completa, enlazadas más abajo.
- **#224:** las pruebas de calderas independientes y el ensayo TCP cooperativo
  guardaron y recuperaron dos calderas, inventarios, reguladores y receptor;
  después volvieron a producir fuerza y mover la formación. El
  [perfil compound con receptor es propio](STEAM_RECEIVER_PROFILE.md), con sus
  límites explícitos. No es una validación del Vauclain original de Open Rails.
- **#234 y #329:** las [primeras capturas de conducción](fixtures/compatibility/twenty-20261015/physical-driving-gpu01/README.md)
  aprobaron hidráulica, freno positivo y salida de cremallera desde reposo. Las
  pruebas CPU comprueban transiciones, energía, consumo e incompatibilidades.
  El fallo posterior de suspensión de ese mismo intento permanece registrado.
- **#325:** la [repetición de suspensión](fixtures/compatibility/twenty-20261015/physical-driving-gpu02/README.md)
  aprobó activación y desactivación con movimiento cosmético apagado, pausa y
  restauración. Las siete pruebas ECS verificaron por separado el movimiento
  físico de carrocería y la conservación de las poses de ruedas y bogies.
- **#324:** el [reintento estricto desde parado](fixtures/compatibility/twenty-20261015/physical-driving-gpu03/README.md)
  llegó a una descarga de rueda, aplicó emergencia y conservó la causa tras
  restaurar y desactivar consecuencias. El guion anterior había fallado al
  observar sólo arranque y detención; sus resultados no se reescribieron.
- **#344 y #345:** cuatro [vistas exteriores inspeccionadas](fixtures/compatibility/twenty-20261015/operational-yards/README.md)
  muestran mesa y carga durante el movimiento y al alinearse, con pausa,
  restauración, cambio de cámara y origen flotante. El coste registrado es el
  de toda la aplicación; no se midió un coste marginal del tablero ni se
  presenta una captura interior de cabina.
- **#397:** la [comparación de seis estaciones, cabina y exterior](fixtures/compatibility/twenty-20261015/visual/comparison.html)
  conserva cámaras, iluminación, hashes y revisión de contenido. Pasaron las
  condiciones declaradas de captura; no se afirma igualdad global de píxeles
  ni aceptación de todas las transiciones de detalle.
- **#407–#410:** quince pruebas de [multijugador](MULTIPLAYER_VIEWER.md) pasaron
  sobre TCP real y el mismo plugin del visor: dos conductores, despachador,
  fogonero y espectador, permisos, latencia, reconexión y guardado completo.
  Fueron **cinco identidades simultáneas**, no ocho. Los
  [tres paneles renderizados](fixtures/compatibility/twenty-20261015/multiplayer/README.md)
  corresponden a despachador, conductor y espectador, con dos visores como
  máximo a la vez. El fogonero se probó por TCP/CPU. El desplazamiento con rueda
  del panel sigue como comprobación manual.

**#220 tiene aceptación dirigida:** la [referencia nativa LIRR en condiciones de partida](fixtures/compatibility/twenty-20261015/lirr-native07/README.md)
completa Brooklyn–Hempstead con la formación y tareas originales. El
[replay LIRR12](fixtures/compatibility/twenty-20261015/physical-final14/lirr/comparison.json)
pasó los seis límites previos con 32.583 muestras, siete vehículos y dos
plataformas: velocidad RMS **0,0360176 m/s** y odómetro máximo **12,35546875 m**.
El [pase anterior LIRR10](fixtures/compatibility/twenty-20261015/lirr-full-service-replay10/README.md)
conserva su resultado y sus recibos originales.
No se ampliaron las tolerancias. La resistencia de túnel tiene una
[captura diagnóstica adicional](fixtures/compatibility/twenty-20261015/lirr-native08-diagnostics/)
que conserva idénticas las columnas de la referencia anterior; el replay usa
la geometría y sus propios estados, sin recibir fuerzas observadas. Las
referencias de contextos anteriores y los intentos fallidos permanecen separados.

**#223 tiene aceptación dirigida:** el [perfil nativo de vapor](NATIVE_STEAM161.md)
pasó las tablas, los subsistemas caliente/frío y la
[comparación cerrada vapor10](fixtures/compatibility/twenty-20261015/physical-final14/steam/comparison.json)
de 80 segundos con una locomotora tanque simple y manual: 1.600 ticks,
error máximo de distancia de **1,982907 m** y de velocidad de **0,0478134 m/s**,
con límites de 2 m y 0,1 m/s. El [pase anterior vapor08](fixtures/compatibility/twenty-20261015/steam-cycle161-closed08/README.md)
conserva sus mediciones y hashes. El
[diagnóstico termodinámico](STEAM_THERMODYNAMICS_AUDIT.md) conserva los fallos
anteriores del recipiente propio; ese modelo mantiene sus pruebas separadas
de conservación y compound. Queda la puerta global para el cierre, sin afirmar
compatibilidad de todos los motores de vapor ni recorridos completos.
Las instrucciones de prueba y las revisiones de píxeles documentadas no se
presentan como pruebas manuales realizadas por el usuario.

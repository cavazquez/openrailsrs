# Clima y diagnóstico del visor

Los issues #190–#197 añaden perfiles visuales de clima, escenografía opcional y herramientas para medir el renderer. La física ferroviaria, las rutas originales y los oráculos fijados de Open Rails 1.6.1 mantienen sus referencias.

## Jugar

F10 → Hora y clima permite elegir **Llovizna**, **Lluvia sostenida**, **Lluvia intensa**, **Nevada leve**, **Nevada intensa**, **Después de nevar** o el ciclo de **Tormenta**. El perfil automático sigue el clima manual o los datos del clima vivo. Al cambiar de perfil, intensidad, nubosidad, niebla y viento se aproximan suavemente al nuevo estado. Pausa congela el reloj de esos efectos.

La tormenta recorre aproximación, actividad, despeje y calma en 660 segundos de simulación. Rayos, iluminación y lluvia usan el mismo estado; el sonido del trueno se retrasa según distancia/343 m/s. Los eventos y las ráfagas se repiten con la misma semilla. El clima vivo suaviza las nuevas muestras recibidas del proveedor; el ciclo determinista es una opción independiente para pruebas.

La acumulación de nieve se separa de los copos que caen. Después de nevar conserva cobertura sin precipitación. Las superficies superiores reciben cobertura irregular; fachadas verticales, cristales e interior de cabina conservan sus materiales. La máscara utiliza normales y textura originales, y se conserva al cambiar de LOD. Lluvia y viento tienen sonido ambiental; volumen, pausa, distancia e interior/exterior se coordinan con el audio existente.

F10 → Hora y clima también permite calidad **Adaptativa**, **Alta**, **Media** y **Baja**, y un presupuesto combinado de partículas CPU/GPU. GPU usa Hanabi 0.19.0; CPU e híbrido conservan su respaldo acotado. F10 → Imagen y cabina permite apagar el humo/vapor y elegir ejecución. Hanabi simula partículas visuales, no la física del tren ni el pasto persistente.

F10 → Imagen y cabina → **Escenografía** conserva **Auténtica** por defecto. **Mejorada** agrega matas instanciadas cerca de la cámara, variación leve del terreno, zonas mojadas y movimiento suave del bosque. Calidad Baja/Media/Alta limita el conjunto a 2.048/8.192/16.384 matas. Las máscaras originales de terreno y geometría dejan libres infraestructura, agua y Forest. Los sectores pertenecen a los tiles y se regeneran con las mismas semillas al regresar. La máscara ferroviaria usa una región acotada; comparte los recorridos de vía y el RDB originales, conserva límites de caminos y compila sólo los caminos cercanos. El catálogo auxiliar incluye sus definiciones y dependencias, sin duplicar todo TSection. No se escriben cambios en el contenido del autor.

## Builds de desarrollo

El build normal y el Snap no activan estas features. Compilar explícitamente:

```bash
CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_OPT_LEVEL=1 CARGO_PROFILE_DEV_DEBUG=0 \
cargo build --locked -p openrailsrs-viewer3d \
  --features dev-tools,dev-inspector,experimental-framepace \
  --bin openrailsrs-viewer3d
```

Después, ejecutar `target/debug/openrailsrs-viewer3d --menu`, o el mismo binario con `--live --route-root "$CHILTERN_ROUTE" "$CHILTERN_SERVICE"`. El lanzador de desarrollo existente compila todo el workspace con todas las features, pero las herramientas permanecen apagadas hasta activarlas.

- **F11**: overlay oficial de FPS y gráfico de tiempos por cuadro. **Ctrl+F11**: depuración de picking. **Ctrl+Shift+F11**: guardar métricas en la carpeta de datos del jugador, `diagnostics/viewer.json`. F6/F10 conservan las asignaciones personalizadas; las herramientas no toman F11/F12 si ya se asignaron a otra acción.
- **F12**: inspector de sólo lectura. Seleccionar una malla con el mouse o filtrar la lista. Muestra entidad, transformación, visibilidad, LOD, capas, mesh, material efectivo y textura. El estado importado incluye prim_state, flags, ZBias, modo Z y alpha test, nombre de shader y textura original. El panel no modifica materiales ni archivos.
- **OPENRAILSRS_DEV_TOOLS=1** activa overlay y trazas de estados al iniciar. **OPENRAILSRS_DEV_INSPECTOR=1** abre el panel. **OPENRAILSRS_DIAGNOSTICS_OUT** cambia la salida del reporte. El inspector comparte el picking y bloquea los controles de cámara/conducción mientras se usa el panel.
- **OPENRAILSRS_FRAMEPACE=off|30|60|unlimited** evalúa bevy_framepace 0.22. Por defecto `off`. **OPENRAILSRS_PRESENT_MODE=fifo|auto_no_vsync** registra VSync por separado. Un compositor puede limitar la entrega aun sin VSync solicitado.

Cada partida inicia un registro nuevo. Los reportes almacenan hasta 12.000 muestras, percentiles y una lista acotada de picos. Clasifican trabajo pendiente de shaders, assets, streaming, CPU, GPU o presentación. Es una atribución observada por el visor, no un profiler completo. El tiempo GPU es el mayor tramo instrumentado, sin sumar tramos anidados. La latencia de entrada es una sonda de cola→siguiente cuadro ECS; excluye teclado, sistema operativo y pantalla.

## Pruebas reproducibles

Se necesita el paquete original de Chiltern v4 fuera del repositorio y un escenario nativo auditado. Los archivos `--route-root` y `--scenario` se proporcionan explícitamente. Compilar el mismo binario con las tres features anteriores para toda la comparación. Cada ejecución utiliza datos de jugador y compositor aislados; las ejecuciones son secuenciales, con GPU real, 1280×720, radio 450 m y semilla 81.

```bash
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_SERVICE" --suite rain --out-dir tmp/qa/rain
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_SERVICE" --suite snow --out-dir tmp/qa/snow
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_SERVICE" --suite storm --out-dir tmp/qa/storm
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_STEAM_SERVICE" --suite vfx --target-m 500 --out-dir tmp/qa/vfx
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_SERVICE" --suite pacing --target-m 1500 \
  --timeout-s 600 --out-dir tmp/qa/pacing
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_SERVICE" --suite scenery --target-m 1500 \
  --timeout-s 600 --out-dir tmp/qa/scenery
```

Las suites lluvia/nieve cubren tres perfiles y tres calidades; tormenta captura cuatro fases; VFX compara apagado, CPU, GPU e híbrido con Hall a vapor. Pacing y escenografía recorren A→B→A con la cámara, desplazándola 7,15 km hacia un punto del recorrido situado a 8 km mientras la física permanece pausada. El punto A se alcanza primero con la formación en marcha. El viaje exige cargar/descargar tiles y volver al origen; valida los hashes de la vegetación regenerada. Los percentiles incluyen los cuadros de streaming posteriores al calentamiento inicial. La plantilla y las variables para ambas formaciones están en la evidencia enlazada al final de esta guía.

`--repeats 3` es el valor habitual. `--case` restringe la prueba. Cada carpeta conserva capturas PNG, logs, reporte individual y `comparison.json` con hashes del escenario/binario. El validador rechaza shaders fallidos, adaptador software, precipitación activa sin partículas, presupuestos excedidos, vegetación que cambia al regresar y viajes incompletos. El contador de dibujos registra envíos reales WORLD/pasto; excluye Standard PBR, UI y Hanabi, por lo que no representa todos los draw calls del cuadro.

CPU del proceso, actividad global del dispositivo usado por el visor y sus límites de alcance figuran en los reportes. El muestreador identifica los clientes DRM, elimina descriptores duplicados y separa cada GPU; selecciona la que registra mayor VRAM del proceso y conserva los valores por dispositivo. No promedia una GPU integrada inactiva con la GPU que renderiza. Linux/RADV expone VRAM y GTT por proceso mediante DRM fdinfo; `null` significa que la plataforma no permite medirlos. No sumar memoria global del dispositivo a RSS como si perteneciera toda al visor.

Resultados medidos y decisiones: [evidencia de los issues 190–197](fixtures/compatibility/weather-renderer-2026-10-07/README.md). Comprobación manual: [sección 49](PLAYER_MANUAL_TESTS.md#49-clima-perfiles-escenografía-y-herramientas-de-desarrollo).

### Patios densos y continuidad de la formación

Las escenas rurales de radio 450 m no cubren el coste de Paddington. Para probar
ese patio, usá un escenario por Paddington Suburban y un radio de 2 km. Conservá
cámara, clima, niebla y ejecutable dentro de cada comparación:

```bash
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_PADDINGTON_SERVICE" --suite rain --case downpour-high \
  --view-radius-m 2000 --fog-quality volumetric64 --camera-yaw -1 \
  --camera-pitch 0.65 --camera-distance 210 --formation-cars 8 \
  --ready-frames 480 --repeats 1 --out-dir tmp/qa/paddington-rain
python3 scripts/benchmark_viewer.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$CHILTERN_PADDINGTON_SERVICE" --suite snow --case heavy_snow-high \
  --view-radius-m 2000 --fog-quality volumetric64 --camera-yaw -1 \
  --camera-pitch 0.65 --camera-distance 210 --formation-cars 8 \
  --ready-frames 480 --repeats 1 --out-dir tmp/qa/paddington-snow
```

`train_formation` registra las transformaciones ECS de las raíces de los coches,
sus desplazamientos en el itinerario y la separación entre centros. La opción
`--formation-cars` exige todos los coches y rechaza diferencias mayores a 2 m
entre separación espacial y separación sobre el recorrido; la tolerancia admite
el acortamiento de la cuerda en curvas. Esta comprobación detecta coches separados
o superpuestos; no certifica la física de los acopladores.

La selección inicial de detalle se aplica directamente. Las transiciones de
partes visibles siguen durando 0,35 s y usan un conjunto finito de rangos, porque
Bevy conserva sus índices durante la vida del renderizador. El validador rechaza
el agotamiento de esa tabla aunque se informe como advertencia. Los benchmarks
no reutilizan resultados si cambia la configuración de cámara, radio o niebla.
[Evidencia de Paddington](fixtures/compatibility/paddington-2026-10-07/README.md).
[Comprobación manual](PLAYER_MANUAL_TESTS.md#50-chiltern-v4-formación-y-clima-en-paddington).

APIs de referencia: [Bevy dev tools](https://docs.rs/bevy/0.19.1/bevy/dev_tools/index.html), [inspector de sólo lectura](https://docs.rs/bevy-inspector-egui/0.37.0/bevy_inspector_egui/reflect_inspector/struct.InspectorUi.html), [framepace 0.22](https://github.com/aevyrie/bevy_framepace/blob/main/Cargo.toml). Las versiones y features efectivas quedan fijadas por Cargo.lock.

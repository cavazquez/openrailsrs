# Cuarta entrega: evidencias de aceptación

La referencia física sigue siendo Open Rails 1.6.1, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`. Se mantienen Bevy 0.19.1 y Hanabi 0.19.0. Las extensiones se comprueban con balances de masa y energía, estados, trayectorias y guardado; no se presentan como paridad de sistemas que OR no declara.

`check.sh` completo pasó con los recursos originales de Chiltern y datos del jugador aislados. Incluyó formato, Clippy sin advertencias, pruebas Rust/Python, empaquetado, build, geometría nativa y los oráculos anteriores. El informe registra 1.949 pruebas Rust aprobadas entre la ejecución habitual y la adicional de geometría nativa. Las pruebas ignoradas que necesitan otros recursos o intervención manual conservan esa condición.

Los seis ensayos anteriores mantienen sus tolerancias. RMS de velocidad: Birmingham 0,4176 m/s; aceleración máxima 0,4394; frenada y marcha por inercia 0,4366; medio regulador 0,6020; servicio Chiltern 0,2081; Class 47 0,2382. Los errores máximos de posición disponibles son 18,46 m, 39,61 m y 25,79 m respectivamente. El servicio local terminó sus tres paradas.

## Equipos y operación

- **#226, #327, #328:** `equipment_physics` y `equipment_acceptance` comprueban dos motores, gobernadores nativos diferentes, combustible sin doble descuento, aislamiento, energía térmica y fallo recuperable sólo del motor sin refrigeración. El ejemplo corto tiene capacidades propias explícitas; no se atribuyen a locomotoras comerciales.
- **#227, #228:** `voltage_filter_oracle`, `vehicle_equipment_oracle` e `independent_pantographs` conservan filtro, retardos, contacto por posición, inversión durante la subida y guardado. Hay 728 muestras originales de tensión y 480 pasos de pantógrafo. La captura muestra dos colectores animados con mandos independientes.
- **#229:** `train_power_bus` y `equipment_acceptance` verifican proveedor real, consumo, cable, desacople, guardado y reacople. El retroceso de práctica conserva también el bus de tráfico todavía sin salida.
- **#230:** conducción diésel fuera de la catenaria, selección exclusiva de fuente, reserva mínima, marcha por inercia al agotarse y recarga desde la red. El guardado reproduce energía y velocidad.
- **#317, #318:** 56 casos originales de curva, mando y límites del freno eléctrico; aceptación de receptividad, rechazo y reparto a reóstato/neumático sin duplicar fuerza ni energía.
- **#319, #320, #321:** calor conservado, descenso prolongado, frenadas repetidas y recuperación; presión y rueda con protección de bloqueo activada/desactivada; corte y recuperación gradual de tracción. La parada del ensayo resbaladizo mide 95,312 m con protección y 96,217 m sin ella. El modelo de ruedas representa un grupo por vehículo.
- **#330, #331:** 402 muestras originales de seis tablas de inyectores, calderas y controles separados, transferencia de agua/vapor/calor y trabajo de compresores con fugas y batería finita. Se conserva el alcance de la caldera existente.
- **#332:** protocolos compatibles/incompatibles, aislamiento, retardos, instrumentos y guardado. La rampa continua mantiene un segundo de transporte en vez de reiniciar el retardo con cada cambio de mando.
- **#334:** el servicio de mercancías de 85 t termina tres paradas, respeta 45 km/h y se cruza con tráfico programado; carga y límite sobreviven al guardado y a la reconstrucción del itinerario.

## Vía, cabina y entorno

- **#232, #233:** lectura de tablas antiguas, categoría/sentido y límites locales; 18 casos originales de peralte. Se conservan cant escrito, rama MainRoute y exclusiones de agujas/caminos. La geometría nativa comprueba contacto rueda–riel y continuidad en ambos sentidos.
- **#249, #299:** 640 muestras originales de inclinación activa y pruebas de viento por orientación/área. Bogies y caja conservan sus planos; el viento no añade una frenada longitudinal. La trayectoria y el estado se conservan al guardar.
- **#257, #259, #260:** puertas independientes por lado/vehículo, andén, enclavamiento y guardado; cambio de puesto válido sin trasladar coches, con instrumentos del vehículo seleccionado. Capturas de los dos lados y de cambio repetido de cabina.
- **#272:** límites de circulación por volúmenes declarados, sin cruzar huecos entre habitaciones; captura de la cámara detenida en el límite del coche. El material sin esos volúmenes conserva sus vistas de asiento.
- **#235:** jugador y tráfico, incluido el que espera su salida, comparten congelamiento/deshielo y recuperan el mismo estado después del guardado.
- **#281, #286:** luminarias originales cercanas y triángulos opacos de protección meteorológica; la prueba del techo inclinado conserva un tragaluz tanto en CPU como en la máscara de Hanabi. Se inspeccionaron cabina y exterior a las 23:00 en Northolt Park, South Ruislip y Gerrards Cross.
- **#300, #302, #303:** bancos periódicos con fase guardada, campo común de nubes/sombras, gotas acotadas al vidrio y barrido propio del limpiaparabrisas. Las capturas incluyen vidrio nativo con y sin barrido. Niebla, nubes y luminarias completan una ida y vuelta de cámara de ocho kilómetros con física pausada y origen flotante.

Las 19 capturas finales usan GPU AMD Radeon RX 7600, Vulkan/RADV, 1280×720 y datos de jugador aislados. Todas terminaron con cero pipelines fallidos o pendientes y escenario completo. Binario: `fcc8bee54cd6310f5b723f50e9cc0caa8576c6bc0db14da2ef2efcb254dd2053`. Los laboratorios de colores son recursos propios para verificar equipos; las vistas de Chiltern usan el contenido instalado del autor.

## Reproducción

La [guía manual](PLAYER_30_4_MANUAL.md) identifica los treinta ejemplos, mandos y resultados esperados. Los [equipos y sus límites](VEHICLE_SYSTEMS.md) separan importación original y perfiles propios. Los informes crudos se generan en `tmp/`, sin versionar contenido descargado ni preferencias del jugador.

```bash
python3 scripts/check_player_fourth_batch.py \
  --viewer target/debug/openrailsrs-viewer3d \
  --out-dir tmp/fourth-graphics --route-root /ruta/original/ROUTES/Chiltern
```

La comparación de rendimiento pasó el [contrato existente](PERFORMANCE_CONTRACT.md): misma ruta/formación, cámara, 1280×720, semilla 42, lluvia, 2.048 partículas, 450 m de visión y calidad automática desactivada. Cada binario cargó sus shaders compatibles. P50: 27 → 27 ms; P95: 38 → 42 ms; P99: 53 → 53 ms; cuadros de más de 100 ms: 1 → 0. Pico de RAM de toda la captura: 1.326 → 1.277 MiB; VRAM del proceso: 1.358 → 1.088 MiB. La tolerancia del 15% y sus mínimos permanecen iguales. El aumento de P95 es de cuatro milisegundos y queda dentro del presupuesto; una medición en este equipo no garantiza esos FPS en todos los equipos.

La revisión de seguridad encontró una expansión sin límite durante el escaneo de puertas. Se corrigió con presupuesto de 64 MiB descomprimidos y una regresión de archivo comprimido excesivo. Los manifiestos de recarga se escriben en la caché del jugador, permitiendo rutas de sólo lectura. Los cuatro issues anteriores de audio siguen fuera de este lote y abiertos para la escucha humana.

La regresión visual de CI pasó contra el golden original: 1,610% de píxeles fuera de tolerancia, máximo permitido 2%, tolerancia de canal 16/255. El golden no se modificó.

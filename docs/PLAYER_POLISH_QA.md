# Verificación de las cinco mejoras de partida

Referencia: Open Rails 1.6.1, fijada por el proyecto. Verificación local del
4 de octubre de 2026, Rust 1.97.1 y Bevy 0.19.1. Las instrucciones para el jugador
están en [PLAYER_MANUAL_TESTS.md](PLAYER_MANUAL_TESTS.md), secciones 15–20.

## Implementación

- Cabinas: perfiles por CVF para altura/distancia del asiento 3D y alcance del
  limpiaparabrisas, con persistencia y restauración. Instrumentos de carga
  separados de RPM; ventana CVF y disposición de escobillas por cabina.
- Efectos: emisores ENG originales para escape diésel y vapor, partículas en
  coordenadas del mundo, sin sombras, limitadas a 512. Mojado gradual de terreno
  y materiales opacos PBR, con actualizaciones acotadas y sin duplicar texturas.
- Servicio: Northolt Park, South Ruislip, West Ruislip, Denham, Denham Golf Course
  y Gerrards Cross, 15,32 km, con dos servicios de tráfico. PAT/TDB y frenos de
  los ocho vehículos quedan identificados por SHA-256 en la procedencia.
- Detalle y memoria: transición de 0,35 s para mallas WORLD rígidas, máximo 64
  mallas salientes; telemetría de cuadros/RAM y comprobación de cobertura de los
  sectores en el viaje real. Las piezas animadas conservan sus enlaces.
- Física del recorrido: límites por posición/sentido, aplicación de aumentos
  después de la cola, pendientes por sección y parámetros EP/aire por vehículo.
  El tren inicia frenado en el andén para sostener la pendiente.

El embarque y la espera de horario se muestran por separado. La práctica rápida
reduce el embarque a cinco segundos y omite la espera de horario. La salida
anticipada con puertas cerradas permite continuar y queda registrada; las
puertas abiertas mantienen el corte de tracción actual. El horario del servicio
normal y los oráculos congelados permanecen intactos.

## Comprobaciones automatizadas

`OPENRAILSRS_NATIVE_ROUTE="$CHILTERN_ROUTE" CARGO_BUILD_JOBS=2 ./check.sh` pasó:
formato, Clippy con advertencias como errores, 1416 pruebas Rust, 11 regresiones
Python, geometría/movimiento nativos, composición de edificios, cuatro oráculos
físicos y el servicio corto completo. La lectura de un único emisor STF tiene una
regresión dedicada; las mallas de partículas vacías mantienen buffers válidos.

El generador extendido produjo nuevamente todos sus archivos con los mismos
bytes. Las seis plataformas coinciden con su TDB dentro de 0,002 m en las
coordenadas de renderizado de esta prueba, con tolerancia de 3 m. El servicio
extendido sin renderer terminó 6/6 en 1997,05 s y 15318,77 m; el mayor error de
detención fue 3,94 m y todas las velocidades de llegada fueron menores a 0,1 m/s.

## Viaje gráfico con GPU

Se ejecutó un único visor a la vez, 1280×720, distancia de escenario 2000 m,
conducción automática y tiempo ×16. Renderer: **AMD Radeon RX 7600 (RADV NAVI33),
Vulkan, GPU dedicada**; Mesa 26.0.8. La presentación utilizó Weston 14.0.2 en un
compositor privado. Xvfb no ofrece DRI3 para esta presentación AMD y su intento
fallido no se considera un resultado válido.

```bash
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/radeon_icd.json WGPU_BACKEND=vulkan \
python3 scripts/check_viewer_streaming.py \
  --route-root "$CHILTERN_ROUTE" \
  --scenario examples/chiltern_extended/scenario.toml \
  --checkpoint terminal --headless-wayland --require-hardware \
  --timeout-s 900 --out-dir tmp/five-gpu-journey-final
```

- Servicio completado: 6/6, 15318,81 m y 1996,45 s de simulación. Mayor error de
  detención: 3,94 m. Ambos servicios AI llegaron con sus paradas registradas.
- Captura en 177,0 s reales. Pico de RSS externo: **3007 MiB**, por debajo del
  límite de 6144 MiB. Seis sectores GPU, 6519 entidades y cero shapes cercanos
  pendientes al capturar.
- Histograma de toda la sesión: 6002 cuadros; P50 **25 ms**, P95 **34 ms**, P99
  **48 ms**. Incluye 4457 cuadros con carga activa. Hubo tres cuadros de más de
  100 ms; el mayor fue 3917 ms. Este resultado no afirma ausencia de tirones.
- Pipelines: cero pendientes y cero fallidos; log sin errores Bevy ni panic.
  Seis emisores originales, 77 partículas al capturar y cero transiciones LOD
  pendientes. No se usó lavapipe para estas mediciones de rendimiento.

PNG, JSON y log están en `tmp/five-gpu-journey-final/`. Se conservan como salidas
locales de QA y no se agregan al repositorio.

## Prueba de controles y cabina

Una sesión Xvfb/lavapipe separada verificó el menú extendido, lluvia y embarque
normal con los dos contadores visibles. En F10 se activó práctica y se guardaron
altura +5 cm, asiento hacia atrás +5 cm y barrido 105 % para `PULLMAN_GR.cvf`;
el archivo de ajustes contiene esos valores y la preferencia de práctica. Tras
reiniciar el servicio, Q/V completaron el embarque rápido y activaron el barrido.
F7 registró Northolt Park, 20 pasajeros y las seis estaciones, con práctica,
cinco segundos de pasajeros y espera de horario cero. Se inspeccionó el vidrio
despejado sin gotas sobre el tablero. La sesión terminó sin errores de renderer,
con pico de 3009 MiB. Sus capturas están en `tmp/five-gui-controls/`; estas pruebas
de interfaz no se usan para informar rendimiento de GPU.

`check_rolling_stock.py --formation KingLE.con --headless-wayland
--require-hardware` verificó también exterior y cabina 2D originales de la King:
ambas vistas cargaron sin geometría de sustitución ni errores, con catorce
emisores originales de escape/vapor detectados. Pico de RSS: 1422 MiB en exterior y 1415 MiB en
cabina. Estas capturas detenidas certifican recursos; la intensidad de las purgas
y del silbato requiere la prueba de movimiento/bocina de la sección 17 del manual.
La revisión final agregó el caso de amperímetros con escala bipolar: cero en
ralentí y carga positiva en tracción, verificado por pruebas de los controles CVF.

## Límites de esta entrega

La composición geométrica y la partida completa están comprobadas. La paridad
física de la captura histórica completa sigue fuera de tolerancia: RMS 7,5749
m/s y diferencia máxima de odómetro 2717,27 m, sin cambiar la referencia ni sus
umbrales. Las señales siguen reglas de ocupación de tres aspectos; no se afirma
compatibilidad completa SIGSCR. Los parámetros nativos mejoran el modelo de
cilindros, pero no reproducen todas las válvulas/depósitos del original. El
amperímetro eléctrico sigue siendo una estimación y el acabado de los objetos
con material de instancias permanece original.

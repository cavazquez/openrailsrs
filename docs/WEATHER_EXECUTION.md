# Nieve, cálculo de efectos y memoria

`F10` permite elegir **Auto, GPU, CPU o Mixto** para la precipitación. El modo
predeterminado usa **bevy_hanabi 0.19.0**: inicializa semillas persistentes en
GPU y calcula viento, caída y oscilación sin subir posiciones cada cuadro. CPU actualiza
una malla combinada limitada. Mixto reparte ambas cargas. En Auto, presión de VRAM
o cuadros lentos sostenidos reducen el presupuesto; la recuperación usa histéresis.

El campo es estable en el mundo: mover el tren o la cámara no arrastra los copos.
Hay variación de tamaño, rotación, forma y caída; precipitación iluminada y
cobertura irregular sobre terreno y trenes. Los edificios, incluidos sus techos,
conservan sus materiales y no reciben cobertura de nieve. Un mapa de altura conservador
reduce partículas bajo terreno/techos; no reemplaza una colisión exacta por edificio.
Vidrio: manchas irregulares, acumulación, velocidad del tren y barrido del limpiaparabrisas.
Nubes: ruido direccional 3D, sin la proyección plana que producía bandas verticales.

## Elegir cálculo y renderizado

El humo y vapor tienen un ajuste separado: **F10 → Imagen y cabina → Humo y vapor**.
Hanabi 0.19.0 ejecuta su movimiento en GPU; CPU usa la malla combinada existente
y Mixto reparte los nacimientos 75/25. Auto conserva esa alternativa cuando
no hay cómputo GPU utilizable. Todos toman los mismos emisores originales,
motor/caldera, viento y reloj ferroviario. El presupuesto total es de hasta
512 partículas; al cambiar de modo se limpia la estela anterior.

La lluvia y nieve ahora usan Hanabi en GPU, con el mismo campo analítico y las
siluetas irregulares del respaldo CPU. Un cambio de intensidad modifica la cuota
visible; las capacidades se agrupan en potencias de dos para acotar la caché. Su control
**Cálculo del clima** y `OPENRAILSRS_WEATHER_EXECUTION` no cambian el humo.
Para pruebas del escape se usa `OPENRAILSRS_TRAIN_EFFECT_EXECUTION=auto|gpu|cpu|hybrid`.
Los dos ajustes eligen dónde calcular partículas; el dibujo sigue usando
el adaptador seleccionado al iniciar.

El renderizador se selecciona al iniciar, mediante `--renderer auto|gpu|cpu`,
`OPENRAILSRS_RENDERER` o la preferencia de F10. Cambiarlo desde F10 requiere reiniciar.
CPU solicita un adaptador software de wgpu/Mesa; no existe migración del dispositivo
durante una partida. GPU explícito rechaza un adaptador software.

```bash
OPENRAILSRS_RENDERER=gpu OPENRAILSRS_WEATHER_EXECUTION=gpu ./scripts/run_chiltern_service.sh
OPENRAILSRS_RENDERER=gpu OPENRAILSRS_WEATHER_EXECUTION=cpu ./scripts/run_chiltern_service.sh
OPENRAILSRS_RENDERER=gpu OPENRAILSRS_WEATHER_EXECUTION=hybrid ./scripts/run_chiltern_service.sh
# Linux con lavapipe instalado: renderizado completo por software
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
OPENRAILSRS_RENDERER=cpu OPENRAILSRS_WEATHER_EXECUTION=cpu ./scripts/run_chiltern_service.sh
```

La física, señales, carga de archivos y lógica de la partida usan CPU en todos los
modos. El modo software permite ejecutar sin GPU utilizable y resulta mucho más
lento; los modos de efectos no prometen ejecutar toda la aplicación en GPU.

## Niebla y faros

**Clima → Niebla** reduce la visibilidad a **120 m**; el volumen local atenúa
objetos cercanos y dispersa los faros, también al mirar desde la cabina.
**F10 → Imagen y cabina → Modelo de niebla → Automática** activa 32 pasos
volumétricos con niebla, lluvia, nieve o tormenta en un adaptador hardware.
Distancia mantiene la alternativa económica; 64 pasos mejora el muestreo a
costa de tiempo GPU. Apagar Niebla elimina ambas capas.

El volumen local mide 400 × 45 × 400 m y se desvanece en los bordes. En niebla
densa se evita aplicar dos veces la misma extinción, mientras la niebla por
distancia completa el fondo. Hanabi atenúa e ilumina sus partículas según
ese ambiente; la precipitación recibe luz de los faros del jugador.

Se corrige en el asset embebido de **Bevy 0.19.1** la distancia usada para
atenuar luces puntuales en el volumen: distancia luz–muestra, en lugar del
radio de toda la caja. También se aplica a esas luces la fase normalizada
Henyey–Greenstein que ya usa la rama direccional: sin ella el haz sobreexpone
la vista desde la cabina. La modificación se limita a esa versión y no cambia
la rama del sol. Ver [fallo upstream #25282](https://github.com/bevyengine/bevy/issues/25282)
y [funciones de fase en PBRT](https://pbr-book.org/4ed/Volume_Scattering/Phase_Functions).
La luz ambiental usa el color actual del cielo y la transmisión acumulada del
volumen; la bruma diurna pierde contraste sin oscurecer el fondo a negro.
El reporte indica si se aplicó; un cambio de shader que impida reconocerla
se informa en el log. No es una simulación de humedad ni una certificación
de equivalencia atmosférica con Open Rails.

## Medición reproducible

```bash
python3 scripts/benchmark_weather_execution.py --route-root "$CHILTERN_ROUTE"
python3 scripts/check_train_effects.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$PERFIL_DE_FORMACION_ORIGINAL" --mode gpu --mode cpu --mode hybrid
python3 scripts/check_train_atmosphere.py --route-root "$CHILTERN_ROUTE" \
  --scenario "$PERFIL_DE_FORMACION_ORIGINAL"
python3 scripts/check_viewer_streaming.py --route-root "$CHILTERN_ROUTE" \
  --scenario examples/chiltern_extended/scenario.toml --checkpoint terminal \
  --headless-wayland --require-hardware --weather snow --weather-execution auto --renderer gpu
```

La matriz usa la misma escena congelada, cámara, hora, resolución **1280×720** y
**2048 partículas** en cuatro ejecuciones secuenciales. Comprueba los hashes,
metadatos, adaptador y actualizaciones de malla antes de aceptar una comparación.
El compositor limita la cadencia; no se deduce una aceleración de throughput a
partir del tiempo por cuadro de una escena limitada a 40 fps.

`F8 → Diagnóstico` separa RSS, VRAM del proceso, GTT y memoria global del dispositivo.
Linux usa `/proc/self/fdinfo`; si no hay contador disponible muestra ausencia,
no cero. La VRAM global incluye otras aplicaciones. **No sumar RSS y VRAM:**
reservas/memoria compartida pueden solaparse. P50/P95/P99 de partida excluyen la
carga inicial e incluyen los tirones de streaming; la carga inicial tiene su propia métrica.

Resultados locales del 4 de octubre de 2026, RX 7600 / RADV:

- Escena fija final: GPU/CPU/Mixto de efectos, RSS 1446–1487 MiB y VRAM del
  proceso 785–809 MiB, sin volver a subir las posiciones de la malla GPU en cada
  cuadro. P50/P95/P99 de partida 25/25/25 ms, con cadencia limitada por el compositor.
- Software completo: RSS 2535 MiB, P50/P95/P99 de partida 202/235/261 ms;
  sin contador de VRAM dedicada disponible.
- Viaje de seis estaciones con nieve Auto: **6/6 paradas**, pico RSS 2695 MiB,
  pico VRAM del proceso 3191 MiB, 8192 copos GPU y una actualización de semillas.
  P50/P95/P99 25/58/72 ms, ocho cuadros de más de 100 ms y máximo 145 ms.
  Esa ejecución coincidió con compilaciones locales; sirve como regresión de
  memoria/cobertura, no como ensayo aislado de rendimiento máximo.

La matriz final, sus hashes, adaptadores, presupuestos y medidas están versionados
en `docs/fixtures/weather/execution-2026-10-04.json`. El test compara 2048 partículas
en todos los modos; la calidad adaptativa de una partida habitual puede reducirlas.

La reducción de RAM no implica que todo se cargue en GPU. Streaming, cachés
limitadas, instancias y materiales compartidos evitan retener toda la ruta en RAM.

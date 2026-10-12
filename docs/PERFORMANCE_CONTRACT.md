# Contrato de regresión de rendimiento

Una comparación válida conserva equipo/driver, resolución, ruta, formación, cámara, semilla y calidad. El binario es la variable de la prueba. `benchmark_player_contract.py` ejecuta los dos visores secuencialmente en un compositor Weston privado, sin tocar la sesión gráfica o las preferencias del jugador.

```bash
python3 scripts/benchmark_player_contract.py \
  --baseline /ruta/al/visor-anterior \
  --current target/debug/openrailsrs-viewer3d \
  --scenario examples/player_lab/scenario.toml \
  --route-root examples/player_lab/route \
  --out-dir tmp/performance-contract --weather rain --target-m 100
```

Ambos binarios deben tener las funciones de captura y usar los mismos recursos. Para una referencia con recursos separados, agregá `--baseline-resources /ruta/a/sus/assets`. El escenario, la formación y los archivos de sus vehículos deben permanecer iguales. Para contenido original, elegí un escenario preparado con referencias absolutas y su raíz original.

El directorio conserva PNG, informes crudos, contratos JSON e informe de comparación. Registra CPU, kernel, arquitectura, adaptador/driver, 1280×720, hashes de ruta/formación/binario, cámara con FOV efectivo y presupuestos. La versión del binario acompaña su SHA256. Rechaza menos de 300 cuadros de partida y condiciones diferentes. La selección de GPU debe ser de hardware; no presenta lavapipe como resultado de una GPU física.

Se comparan P50, P95, P99, cuadros de más de 100 ms, RAM actual/pico y VRAM si el driver la expone. La tolerancia normal permite 15% con mínimos de 2 ms, 96 MiB y tres tirones para absorber ruido pequeño. Carga inicial y sus cuadros se informan aparte. Un dato ausente permanece ausente.

```bash
python3 scripts/check_perf_contract.py \
  --baseline tmp/performance-contract/baseline-contract.json \
  --current tmp/performance-contract/current-contract.json \
  --out tmp/performance-contract/comparison-repeated.json
```

La herramienta devuelve código 1 si supera un presupuesto y rechaza contratos incompatibles. Antes de atribuir un cambio al código, repetí el mismo ensayo con las aplicaciones de fondo cerradas y sin otro compilador usando CPU/RAM. Para automatizarlo, usá un ejecutor con equipo/driver fijos; un runner público variable sirve para pruebas de código, no para certificar FPS de hardware. `check.sh` incluye las pruebas del comparador; las capturas requieren un ejecutor gráfico.

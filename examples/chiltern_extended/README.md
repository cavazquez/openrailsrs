# Chiltern extendido: seis estaciones

Recorrido jugable de 15,32 km, desde Northolt Park hasta Gerrards Cross. Usa el
Pullman original, las plataformas TDB y el PAT local hasta West Ruislip; continúa
por conexiones nativas hacia Denham, Denham Golf Course y Gerrards Cross. Incluye
los dos servicios de tráfico del escenario corto, pendientes por sección y
límites de velocidad por posición/sentido. El horario es propio del escenario.

```bash
./scripts/run_chiltern_service.sh --direct
./scripts/run_chiltern_service.sh --autodrive --cab

target/debug/openrailsrs play-service examples/chiltern_extended/scenario.toml \
  --out-dir tmp/extended-service

python3 scripts/check_viewer_streaming.py \
  --route-root "$CHILTERN_ROUTE" \
  --scenario examples/chiltern_extended/scenario.toml \
  --checkpoint terminal --timeout-s 900 --out-dir tmp/extended-streaming
```

La comprobación de streaming obtiene el destino del escenario, no usa la
posición final del servicio corto. `--software` permite comprobar carga con
Vulkan lavapipe; `--require-hardware` exige una GPU real para medir rendimiento.
En AMD, Xvfb puede carecer de DRI3 y rechazar la presentación Vulkan. Con Weston
instalado, agregá `--headless-wayland --require-hardware`: crea un compositor
privado, lo cierra al terminar y no utiliza el escritorio del jugador.

F10 → Práctica rápida en estaciones reduce el embarque a 5 s y omite la espera de
horario. El modo normal distingue pasajeros y hora de salida. F7 indica el modo y
registra las salidas anticipadas. Pruebas manuales completas en
[`PLAYER_MANUAL_TESTS.md`](../../docs/PLAYER_MANUAL_TESTS.md#15-embarque-horario-y-práctica-rápida).

`provenance.json` identifica la versión de referencia, los hashes PAT/TDB, los
seis extremos de plataforma y los ocho ENG/WAG originales con sus parámetros
de freno. Los pequeños archivos físicos de este servicio agregan esos tokens a
las bases convertidas, conservando intactos los fixtures históricos del replay.
El exterior, las cabinas y los emisores se resuelven desde el Content original.
Se mantiene el máximo del corredor de 80 km/h;
las señales usan ocupación de tres aspectos y no el SIGSCR completo.

Regeneración: importar la TDB con `openrailsrs import-msts RUTA --out-dir tmp/imported` y pasar su
`track.toml` a `scripts/prepare_chiltern_extended.py --route-root ...
--imported-track ...`. El escenario no copia el Content original al repositorio.

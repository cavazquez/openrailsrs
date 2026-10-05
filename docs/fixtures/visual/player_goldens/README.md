# Goldens de la partida (issue #170)

Siete vistas **1280×720**, Northolt Park, Pullman original, 09:55, despejado,
FOV vertical 45°, radio de escenario 450 m, tren y reloj detenidos. Poses,
máscaras, tolerancias, hashes de PNG y cámara están en `manifest.json`.
Frente/arriba/izquierda/derecha usan el mismo puesto 3D; también cabina 2D,
chase y orbit. La órbita de esta prueba enfoca el origen del primer vehículo,
como el adaptador de referencia OR; el encuadre de la cámara jugable se conserva.

`acceptance.json` conserva la ejecución completa del 4 de octubre de 2026:
siete vistas y dos comparaciones nativas aceptadas, cuatro fallos detectados,
con hashes de captura y manifiesto, regiones, métricas y adaptador usado.

## Un comando

```bash
python3 scripts/check_visual_goldens.py --route-root "$CHILTERN_ROUTE" --prove-faults
```

Usa el visor ya compilado, Vulkan hardware y un compositor Weston privado,
una ventana a la vez, límite de RAM de 6 GiB y timeout. No hereda variables
`OPENRAILSRS_*`; no reutiliza PNG de ejecuciones anteriores. Exige shaders listos,
WORLD cargado y ausencia de transiciones LOD pendientes.

Instalar las dependencias del análisis de píxeles con
`python3 -m pip install -r scripts/requirements-visual.txt` (NumPy/Pillow).

El informe `tmp/player-goldens/report.json` incluye RGB por región, píxeles
fuera del umbral, F1 simétrico de bordes y la máscara/silueta de pintura azul
renderizada del tren. Compara realmente PNG: no basta contar entidades,
marcadores, banderas o AABB del ECS. Máscaras de ventana, panel, instrumentos,
techo y formación permiten localizar el fallo. La caja de formación restringe
la búsqueda; los píxeles azules requieren coincidencia espacial independiente
para que el paisaje no diluya un tren ausente.

## Referencia nativa y tolerancias

Referencia fija OR **1.6.1**, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`.
Capturas nativas de cabina/exterior y sus matrices están en
`../or_reference/chiltern_station_views/`; no se modifican ni reiluminan.
Entre ejecuciones Bevy: MAE ≤5 niveles RGB, ≤3% de píxeles con diferencia >24,
F1 de bordes ≥0,85 (radio de 2 píxeles); silueta propia del tren IoU ≥0,90.
Frente a OR: panel/instrumentos MAE ≤25, ≤10% de píxeles con diferencia >64,
F1 ≥0,75. La formación exige IoU de pintura ≥0,55, IoU de extensión ≥0,85 y sus límites RGB documentados en el
manifest. Se conservan diferencias de iluminación, modelos ausentes en el
contenido OR y encuadre del HUD. El escenario fuera de esas regiones no se
presenta como paridad aceptada.

No se corrige exposición, registra/traslada imágenes ni aflojan umbrales durante
la comprobación. La tolerancia de color no puede compensar un fallo estructural.
Las regiones y umbrales forman parte de la revisión del fixture.

## Pruebas que deben fallar

`--prove-faults` ejecuta fallos reales sobre el renderer:

- Reflejar UV de las mallas de cabina: texto y controles asimétricos invertidos.
- Cambiar la dirección frontal por una mirada lateral con el mismo puesto.
- Insertar un plano negro grande a 0,5 m delante de la cámara.
- Ocultar la geometría exterior del tren manteniendo escenario y metadatos.

Cada comparación debe fallar; el informe exige `detected: true` en los cuatro
casos. La inyección solo opera cuando hay captura explícita habilitada.
Los tests Python usan además los PNG reales para verificar la sensibilidad
sin necesitar assets privados en CI.

## Actualizar una referencia

`--record` es una operación explícita que escribe candidatos versionados.
Revisar las siete capturas, las máscaras y el informe nativo antes de aceptar.
Nunca ejecutar `--record` en CI ni convertir automáticamente un fallo en PASS.
`--view` permite una reparación enfocada; la aceptación completa usa las siete.
Las regresiones #165–#169 quedan cubiertas por las regiones de cabina, UV,
oclusiones, aislamiento exterior y dirección; siguen existiendo sus tests Rust.

- [#165](https://github.com/cavazquez/openrailsrs/issues/165): panel/instrumentos de cab-front y fallo mirror.
- [#166](https://github.com/cavazquez/openrailsrs/issues/166): techo/cab_frame de cab-up, cab-left y cab-right.
- [#167](https://github.com/cavazquez/openrailsrs/issues/167): windshield/panel y fallo occluder.
- [#168](https://github.com/cavazquez/openrailsrs/issues/168): formación en chase/orbit y fallo train_missing.
- [#169](https://github.com/cavazquez/openrailsrs/issues/169): ventana frontal en cab-front/cab-2d y fallo forward.

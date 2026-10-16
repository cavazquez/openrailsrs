# Ensayo aislado de fuerzas de túnel OR 1.6.1

`reference.json` selecciona salidas numéricas genuinas de la captura
`../lirr-native08-diagnostics/`. El extractor no calcula fuerzas de referencia.
Los hashes del CSV crudo, su proyección idéntica a 07, el manifiesto, los
helpers y las fuentes originales constan en `provenance.json`. No requiere
contenido ferroviario instalado para ejecutar la prueba Rust.

El límite absoluto de 0,01 N se declaró en `protocol.json` antes de la primera
comparación Rust. Se conservan los seis límites del recorrido completo. Los
259 registros cubren siete coches, entrada/salida y cambio de número de vías,
máximos y los instantes 400, 500 y 600 s. Hay 140 llamadas con geometría activa
y 119 registros inactivos que comprueban la fase capturada y su fuerza cero.
Estos últimos no sustituyen las pruebas del guard de pertenencia al túnel.

Cada fuerza de la fila N usa entradas de la fila N−1: velocidad local absoluta,
masa y longitud de formación y `CarTunnelData`. La masa y las dimensiones de
los coches SF son estáticas en esta captura. El gálibo se obtiene con el
producto `Single(width * height)`, sin alimentar el campo observado
`TrainCrossSectionAreaM2`. La longitud del túnel para la fórmula es la suma
`Single(Ahead + Behind)`, que conserva la resta nativa de la longitud del coche.

El TRK original declara `SpeedLimit (223.52)` m/s y no sobrescribe geometrías
de túnel. Por ello se usan los defaults documentados de `TrainCar.Initialize`:
70 m² / 32 m para una vía y 100 m² / 37,5 m para dos. No se seleccionan esos
parámetros a partir de una fuerza observada.

La prueba `native_tunnel_oracle.rs` valida procedencia y límite, recompone las
entradas anteriores e invoca `native_tunnel::force_for_geometry`. Esta prueba
aislada prescribe sus fronteras de subsistema. Una aceptación del recorrido
cerrado debe derivarlas del mapa y del estado Rust, sin inyectar filas de la
referencia. El manifiesto de extracción conserva `rust_comparison: not_run`;
cualquier ejecución posterior necesita su propio resultado y recibo.

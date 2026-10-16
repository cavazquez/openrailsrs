# Diagnóstico original LIRR 08: túneles

Captura de Open Rails 1.6.1, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`, mediante las DLL originales y
`ActivityTrajectoryDiagnostics.cs`. Conserva el ACT, formación, mandos, pausa,
clima, frecuencia y seis límites del ensayo 07. El helper nuevo añade solamente
lecturas de campos; los helpers y recibos anteriores conservan sus bytes.

Hay 32.583 filas y 306 columnas. Las 164 columnas anteriores coinciden como
cadenas en las 32.583 filas: 5.343.612 valores idénticos. La proyección CSV con
los finales de línea originales recupera el SHA256 de 07
`21212399f6289ff44b6d108d6312670008e6503abee0054df2d4dd150d6e2be2`.
`telemetry-integrity.json` conserva esa comprobación por columna.

`native/trace.csv.gz` es una compresión sin pérdida con fecha gzip cero. Su
SHA256 comprimido figura en `publication.json`; al descomprimir se recuperan
los 77.143.773 bytes y el SHA256 original
`362ecf741d06ae02e5bfb6ea67af88559d0f98adf631342d42e070c91f4eb13a`.
El manifiesto capturado mantiene el nombre y hash del CSV crudo. Los demás
archivos publicados conservan literalmente sus bytes, incluidos los recibos,
las rutas del equipo y el snapshot de pines usado antes de ejecutar.

Las nuevas columnas identifican por coche fuerzas de curva, viento y túnel,
masa, gálibo, geometría nullable del túnel y sus parámetros de una o dos vías.
A 400 s, con curva y viento cero, la suma original de fuerzas de túnel es
303,5520324707031 N; a 500 s es 238,7202033996582 N. Son observaciones de las DLL,
no fuerzas prescritas a la simulación Rust.

`TrainCar.UpdateTunnelForce` se ejecuta antes de `Train.ProcessTunnels`. La
fuerza publicada en la fila N usa geometría, velocidad y longitud de formación
de N−1. El ensayo aislado de `../tunnel-or161/` registra esa frontera y calcula
el área del coche a partir de sus dimensiones estáticas. El recorrido cerrado
debe obtener por sí mismo posición, pertenencia al túnel y movimiento.

La captura pasó el control de entradas antes/después y el cierre de OracleWine
privado sin procesos propios restantes ni controladores de escritorio/audio.
No incluye DLL, prefijo, originales ENG/WAG/CON/ACT/PAT ni archivos de ruta.
Estos recibos certifican la captura original; los resultados Rust se publican
por separado y no se deducen de la coincidencia entre 07 y 08.

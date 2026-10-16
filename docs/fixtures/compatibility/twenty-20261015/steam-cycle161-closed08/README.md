# Medición cerrada de vapor 08

El ejecutable Rust de build11 completó 1.600 ticks a 20 Hz contra
`../physical-native05/native-steam/steam-drive.json`, con las fases originales
de fuego en reposo, carga, inyección, deriva y frenado. Cumple todos los límites
previos: presión, masa, calor, producción, consumo, inyección, inventarios,
esfuerzo, velocidad, distancia y estado de inyectores. Los máximos completos
figuran en `publication.json` y por fase en `comparison.json`.

El error máximo de velocidad es 0,0504398623 m/s, frente a 0,1 m/s; el de
distancia, 1,8623010547 m, frente a 2 m. No hay discrepancias de encendido de
inyectores. Ninguna velocidad, fuerza, presión, masa, calor o caudal observado
de la trayectoria alimentó el estado Rust.

El binario medido tiene SHA256
`922c65c5fb31dbd280773b36412aef72b58af87dd98caaa1b50ca8443577fb77`.
`build-source-snapshot.json` identifica los 752 archivos de build11, SHA256
`683608559ede78f65565b7543988a7c7cac4c726ce53281986142cab934db937`.
Su inventario de 27 archivos de aceptación compilados tiene SHA256
`39bdbb77fa0836682696a4ef0f5904db2ac8877a7c4073eac095c6e0b1c119cf`.
`receipt.json` conserva los 728 hashes del inventario específico de la
medición; son inventarios explícitos diferentes. Todos sus hashes comunes
coinciden. HEAD identifica la ancestría del árbol sucio, no un commit que
contenga los cambios medidos.

Recibo, comparación, log, traza Rust, configuración, mandos y snapshots se
copiaron literalmente. `publication.json` añade sus hashes y alcance sin
reescribir los recibos. No se incluyen ENG/PAT/CON, DLL, ruta importada, diff
del árbol ni contenido de las seis trazas humanas. La publicación histórica
`../steam-cycle161-closed06/` conserva íntegros sus archivos y hashes.

El alcance es tanque 3F, carbón, fogonero manual, máquina simple, vacío sin
depósito equilibrador y `SimpleControlPhysics`. Se conserva el contexto
original `Simulator.Paused=true`; paused03 aporta sólo la posición geográfica
estática post-Start. Es una trayectoria cerrada de 80 s, sin aceptación de
ruta completa, ciclo universal, ténder externo, fogonero automático o Compound
nativo. El recipiente propio sigue siendo una extensión distinta. El pase de
esta medición no declara aprobado el gate completo del workspace.

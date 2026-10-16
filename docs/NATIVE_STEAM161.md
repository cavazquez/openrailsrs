# Ciclo de vapor simple OR 1.6.1

`ORTSRSNativeSteam161 ( 1 )` selecciona por ENG un puerto acotado de
`MSTSSteamLocomotive`, Open Rails 1.6.1, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`. El 3F identificado pasó el ensayo
cerrado de 80 s y los subsistemas hot/cold con límites registrados antes de
capturar. El [catálogo de ejecución](../oracles/steam-cycle-161-execution.json)
enlaza capturas, mediciones, binarios, fuentes y regresiones de CI.

El alcance jugable es una locomotora tanque, carbón, fogonero manual, una
máquina simple de transmisión directa, inyectores, vacío sin depósito
equilibrador y `SimpleControlPhysics`. El loader rechaza el ténder externo
requerido, cuya asociación física aún no se implementó para este modo.
Fogonero automático, compuesto nativo y compatibilidad universal con cualquier
ENG de vapor quedan fuera. El recipiente conservativo y el receptor HP/LP
propios siguen siendo extensiones separadas. No hay selección por nombre de
locomotora ni cambios en el ENG original.

## Estado y secuencia

Se preservan unidades imperiales, `Single`, tablas y orden del original.
El guardado conserva masa/calor/presión, fuego y depósitos, filtros de 150/240 s,
entradas previas, sobrecalentador, inyectores, eyectores, válvulas y diagrama
del cilindro. Restaurar exige hardware idéntico y espejos coherentes; eliminar
el estado nativo no permite degradarlo al ciclo legacy.

`or_hot` inicia a presión máxima menos 5 psi; `or_cold`, al 66% del máximo.
Las tablas determinan masa y calor iniciales. La inferencia legacy
`SuperHeater ( 1.23 )` ocurre después de escoger eficiencia y caída inicial
saturadas; la contrapresión usa la selección posterior sobrecalentada.
La masa del 3F permanece en 50 t: su ENG no declara animación de carga continua.

La propagación del vacío lee fase de mando y caudales del tick anterior;
el wrapper inicia en Release. Después se publica el mando actual para el
eyector. Siguen ténder, fuego, caldera, auxiliares, sobrecalentador, cilindros,
esfuerzo, nivel, inyección y mandos de fuego. El guardado conserva esa fase;
las entradas no finitas se rechazan antes de mutar.

`BoilerHeatBTU` es el inventario térmico definido por OR, distinto de la energía
interna del recipiente propio. `EvaporationLBpS` es capacidad de producción,
no masa nueva añadida a la caldera. Los canales originales de producción,
consumo e inyección se comparan por separado.

## Evidencia medida

El [protocolo previo](../oracles/steam-cycle-161-protocol.json), SHA256
`c62fc972d0799c3fa7d32db8e6bcd19fde852b986b8ebe652bbb96de0e7e73cb`,
permanece intacto. Los helpers ejecutan DLL originales; las tablas Rust son
implementación, no el oráculo independiente.

- [Hot02](fixtures/compatibility/twenty-20261015/steam-cycle161-hot02/publication.json)
  y [cold05](fixtures/compatibility/twenty-20261015/steam-cycle161-cold05/publication.json):
  1.601 filas por caso y 310 consultas DLL de tablas aprobadas. Velocidades de
  0/8/16/12/4 m/s y tubo externo se fijaron antes de capturar. Son fronteras de
  subsistema; no prueban un viaje ni propagación del freno por una formación.
- [Cerrado06](fixtures/compatibility/twenty-20261015/steam-cycle161-closed06/publication.json):
  1.600 ticks a 20 Hz, 80 s, con fuego/carga/inyección/deriva/frenado contra la
  [captura original05](fixtures/compatibility/twenty-20261015/physical-native05/manifest.json).
  Ambos modelos inicializan desde hardware y mandos. Ninguna velocidad,
  presión, masa, calor, fuerza o caudal observado alimenta la evolución Rust.
  Su binario SHA256 es
  `5fc13442f463a20641d6b63e5da177ace6393b22ab4a8f677004215347209a56`.
- [Cerrado08](fixtures/compatibility/twenty-20261015/steam-cycle161-closed08/publication.json):
  repetición aprobada tras build11, con los mismos 1.600 ticks, referencia,
  mandos y límites. El binario es
  `922c65c5fb31dbd280773b36412aef72b58af87dd98caaa1b50ca8443577fb77`.
  Se publican el recibo de medición y los snapshots de 752 fuentes y 27
  archivos de aceptación compilados. El histórico06 permanece intacto.
- [CI y dirigida11](fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/execution/receipt.json):
  59 pruebas aprobadas, cero fallos/ignoradas. Incluye 9 del ciclo, 3 de
  integración/guardado, 3 nuevas de CI y 14 físicas propias. CI compara 3.202
  filas y 310 tablas con JSON numérico derivado y perfil STF, sin Wine,
  ENG/PAT instalados ni dependencias nuevas.

Los máximos de la última medición08 son: presión 0,000034743 bar; masa 0,022361 kg;
calor 33.234,26 J; producción 0,000007571 kg/s; consumo 0,001027 kg/s;
inyección 0,000002011 kg/s; esfuerzo 18,493 N; velocidad 0,050440 m/s;
distancia 1,862302 m. Los límites siguen siendo 0,1 bar, 1 kg, 100.000 J,
0,1 kg/s de producción/consumo, 0,05 kg/s de inyección, 100 N, 0,1 m/s y 2 m.
Los estados de encendido de inyectores no tienen discrepancias.

El cerrado conserva el contexto histórico `Simulator.Paused=true`;
[paused03](fixtures/compatibility/twenty-20261015/steam-cycle161-drive-paused03/publication.json)
aporta sólo el centro geográfico estático post-Start.
[unpaused04](fixtures/compatibility/twenty-20261015/steam-cycle161-drive-unpaused04/publication.json)
es una captura distinta sin comparación Rust aprobada. El pase de 80 s no es
un recorrido completo ni paridad universal.

Latitud `Single`, tabla Summer/Clear y altura inicial calculan el ambiente.
No se usa temperatura observada del primer tick; la altura física nativa del
contexto sin viewer es 0, distinta de `WorldPosition.Location.Y`.

## Procedencia y publicación

Los manifiestos nativos y sus helpers/recibos se copian literalmente, incluidos
su estado histórico `captured_not_compared` y rutas de procedencia. La nueva
aceptación figura en recibos separados. Sólo `reference.json` se comprime sin
pérdida: `publication.json` distingue SHA comprimido y SHA del original
descomprimido. La proyección compacta de CI conserva extractor y hashes de
ambos formatos; no se presenta como byte idéntico a la captura completa.
No se empaquetan ENG/PAT/CON/DLL, runtimes ni recursos originales.

El [diagnóstico del recipiente propio02](fixtures/compatibility/twenty-20261015/steam-diagnostic02/publication.json)
sigue fallando equivalencia nativa. Su conservación pasó; el pase del modo
`native-or161` no sustituye ese resultado.

## Conducción y comprobación manual

[scenario_native_steam.toml](../examples/physics_lab/scenario_native_steam.toml)
usa material propio con selector explícito, sin apariencia ni audio originales.
Liberar vacío antes de arrancar, aplicar regulador/corte y variar fuego/soplador.
Contrastar producción con consumo. Activar el inyector y observar retardo de
flujo y caída de agua del tanque. Frenar, guardar/restaurar y comprobar
continuidad de mandos, filtros e inventarios.

El reaprovisionamiento de agua/carbón entrega y debita sólo masa aceptada, sin
reiniciar fuego, fallos ni estado térmico. Fogonero automático y válvula
compound no cambian este modo manual/simple. Las pruebas dirigidas y el
cerrado son evidencia de CPU; no atribuyen un pase al gate global ni una
aceptación visual/acústica que no se haya registrado.

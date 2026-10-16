# Vapor #223/#224: semánticas, medición y límites

El puerto acotado `native-or161` del 3F pasó producción, presión, consumo,
inyección, esfuerzo y cinemática en los cerrados06 y 08 de 80 s con límites originales.
También pasaron hot/cold como subsistemas y sus regresiones CPU con datos
genuinos. El recipiente propio conserva masa/energía, pero continúa sin
equivalencia numérica con el ciclo nativo. Son modelos explícitamente distintos.

El [catálogo](../oracles/steam-cycle-161-execution.json) registra archivos,
hashes y alcances; [NATIVE_STEAM161.md](NATIVE_STEAM161.md) explica el modo
jugable y su guía. Referencia: OR 1.6.1, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`.

## #223: conservación y propiedades

Las 157 consultas DLL previas verifican temperatura, densidad y entalpía de
agua/vapor, incluidas interpolaciones. El nuevo conjunto añade 310 consultas
reales de 50 factories y dos tablas 2D. CI usa los números DLL como oráculo y
las tablas Rust como implementación.

La extensión conservativa mantiene masa y energía interna de un recipiente
finito: fuego/agua/vapor, pérdidas, seguridad e inventario de ténder. El
[diagnóstico propio02](fixtures/compatibility/twenty-20261015/steam-diagnostic02/publication.json)
registró residuos máximos de 2,76e-10 kg y 5,01e-5 J.
`vessel_conserves_mass_energy_through_fire_load_injection_and_safety` volvió a
pasar en las 14 pruebas físicas dirigidas11. Es evidencia del balance propio
con propiedades de la referencia.

El original tiene otra semántica: `BoilerHeatBTU` es el inventario térmico de
OR y `EvaporationLBpS`, capacidad térmica de producción. No se añadió capacidad
como masa creada ni PV para convertir ese campo al balance propio. Se respetan
las salidas originales y sus unidades.

## Puerto realizado y fases reproducibles

`native_steam/{mod,firebox,boiler,cylinders,injectors,vacuum,bridge,tables}.rs`
traduce carbón/manual/simple/tanque; `typed/native_steam.rs` deriva hardware y
defaults del STF. El selector es explícito por ENG, sin calibración por nombre
ni cambios en los recursos del autor.

Se portaron normalización de volumen/áreas, masa de fuego, tasas y curvas;
arranque hot máximo menos 5 psi y cold 66%; humos y transferencia, filtros de
150/240 s, tabla inversa de presión, diagrama simple de cilindros/MEP/consumo/
esfuerzo, inyectores y energía de entrega, eyectores y auxiliares del alcance.
Se conservan `Single`, retardos y secuencia. El sobrecalentador legacy se
infiere tarde: eficiencia y caída inicial permanecen saturadas, aunque la
contrapresión posterior sea sobrecalentada. La propagación del vacío consume
fase/caudales previos y el eyector lee el mando actual.

La revisión del cerrado corrigió causas compartidas según fuente: recuperación
FCalc de fricción válida, altura correcta de Size y distancia rígida de ejes
de vapor. No se prescribió resistencia residual ni se ajustó una curva a la
trayectoria. Los intentos fallidos04/05 siguen en tmp; no se regeneraron
capturas ni tolerancias para ocultarlos.

Hot02/cold05 comparan 1.601 filas por caso con fronteras de 0/8/16/12/4 m/s y
tubo externo elegidas antes de capturar. Calor tuvo error máximo 0 J, presión
menor que 5,26e-7 bar e inyección menor que 1,08e-7 kg/s. Son subsistemas; no
prueban integración de ruta ni propagación por una formación.

El [cerrado08](fixtures/compatibility/twenty-20261015/steam-cycle161-closed08/publication.json)
completó 1.600 ticks a 20 Hz contra
[physical-native05](fixtures/compatibility/twenty-20261015/physical-native05/manifest.json),
manifiesto SHA256
`667fb11206aa479c6a6d5901e880f0d1c1f1ad1699643c598d667620db774014`.
Su binario medido tiene SHA256
`922c65c5fb31dbd280773b36412aef72b58af87dd98caaa1b50ca8443577fb77`.
Las fases son fuego en reposo, carga, inyección, deriva y frenado.
El [cerrado06 histórico](fixtures/compatibility/twenty-20261015/steam-cycle161-closed06/publication.json)
conserva sus archivos y el binario anterior sin cambios.

Máximos cerrados: 0,000034743 bar, 0,022361 kg, 33.234,26 J,
0,000007571 kg/s de producción, 0,001027 kg/s de consumo,
0,000002011 kg/s de inyección, 18,493 N, 0,050440 m/s y 1,862302 m.
Todos cumplen límites previos; inyectores tienen cero discrepancias.
Cada fábrica inicializa independientemente desde hardware y mandos. Sólo la
posición geográfica estática de paused03 completa el contexto; ningún estado
térmico, filtro, velocidad o fuerza observado después del inicio alimenta Rust.
El recibo contiene los 728 hashes de fuentes/configuración medidos y declara
el árbol sucio. El snapshot de build11 añade su inventario distinto de 752
fuentes, SHA256 `683608559ede78f65565b7543988a7c7cac4c726ce53281986142cab934db937`,
y el de 27 archivos de aceptación compilados. Los hashes comunes coinciden.
HEAD es ancestría, no una afirmación de contener ese código.

## Regresión y alcance probado

`native_steam_subsystem_oracle` pasó 3/3: 3.202 filas y 310 tablas, sin Wine,
ENG/PAT instalados ni rutas descargadas. La proyección compacta de 470.839 B
mantiene todos los ticks y decimales. Procedencia, algoritmo, manifiestos y
hashes de ambos formatos están publicados. Otras 9 pruebas del ciclo y 3 de
integración verifican filtros, inyección diferida, selección de tablas, fase
de vacío, rechazo de entradas no finitas, identidad, guardado exacto y rechazo
de estados/hardware/espejos adulterados.

El [recibo dirigido11](fixtures/compatibility/twenty-20261015/steam-cycle161-subsystems/execution/receipt.json)
conserva 59 PASS, cero fallos/ignoradas, con log SHA256
`aaf22f351caef1c7353c2ef6154ec3c282b997e0362a7bb571a7fe79e810aa57`.
Es un resultado dirigido, no el gate completo del workspace.

El cerrado aceptado dura 80 s en contexto histórico `Paused=true`, control
simple y una locomotora tanque. No es una ruta completa ni aceptación de
`Paused=false`, fogonero automático, ténder externo o compuesto nativo. La
captura unpaused04 queda sin comparación Rust aprobada. La configuración
jugable rechaza explícitamente el ténder requerido en este modo.

## Fallo histórico propio conservado

El diagnóstico propio02 mantiene `outside_prior_limits`: presión 6,28422 bar,
masa 317,893 kg, calor 972,936 MJ, fuerza 76,334 kN, velocidad 27,4625 m/s y
distancia 1.288,268 m. Su balance propio aprobado no demuestra paridad. El
nuevo modo no convierte ese fallo en pase ni cambia bytes del diagnóstico,
capturas o límites.

## #224: doble tracción y perfil compuesto declarado

Caldera/fuego/ténder/mandos conservan identidad por locomotora. Reguladores
locales respetan aislamiento, puertas, inversor, emergencia y protecciones.
Los ensayos dirigidos11 verifican consumo/calor y esfuerzo por slot, suma,
ausencia de tracción en el vagón, calderas distintas y balance por locomotora.
La conducción propia de doble vapor alterna bypass, mantiene receptor activo
y restaura exactamente; eliminar una caldera o su estado del snapshot se
rechaza antes de mutar.

El perfil HP/receptor/LP propio satisface ese alcance declarado; sus pruebas
no requieren compatibilidad universal Compound. OR 1.6.1 modela Vauclain
balanceado sin receptor. Aquí no existe medición de doble tracción o compuesto
nativo y no se afirma tal equivalencia.

[STEAM_RECEIVER_PROFILE.md](STEAM_RECEIVER_PROFILE.md) detalla gas ideal a
temperatura de caldera, flujos/energía advectada, trabajo HP y trabajo LP
separado. No es cierre completo de energía, temperatura y condensación del
receptor/LP. El borde menor que 1 N ya usa trabajo de cada fuerza por
desplazamiento. Sus dos regresiones unitarias
[pasaron en libs-and-mp03](fixtures/compatibility/twenty-20261015/steam-cycle161-own-ledger03/receipt.json):
0,5 N no crea trabajo LP y bypass no contabiliza admisión/trabajo de receptor
inexistentes. El extracto literal y el hash del log completo identifican ese
resultado propio, separado de cualquier comparación nativa.

Los criterios iniciales de #223 disponen ahora de conservación propia con
propiedades nativas, fases originales medidas y conducción cerrada acotada.
Los de #224 disponen de inventarios/mandos independientes y reparto/conducción/
guardado del perfil propio. El cierre administrativo y la validación conjunta
final corresponden a Root; estos resultados no amplían el alcance medido ni
sustituyen verificaciones manuales todavía no registradas.

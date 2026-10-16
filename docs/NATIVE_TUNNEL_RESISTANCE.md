# Resistencia aerodinámica de túneles originales

La física nativa importa `TunnelShape` y `NumPaths` de `tsection.dat` junto con
las referencias de cada sección del TDB. Los grupos consecutivos se conservan
por nodo vectorial, con el menor número de vías del grupo. No se deduce un túnel
a partir de su modelo visual, el nombre de una ruta ni una fuerza medida.

El TRK aporta `ORTSSingleTunnelArea`, `ORTSSingleTunnelPerimeter`,
`ORTSDoubleTunnelArea` y `ORTSDoubleTunnelPerimeter`. Las áreas sin sufijo usan
pies cuadrados, como el lector nativo. Cuando el área es cero o está ausente,
se reemplazan área y perímetro por el par de Open Rails 1.6.1 correspondiente a
la velocidad de diseño del TRK. Los límites locales no eligen ese par.

La fuerza sigue `TrainCar.UpdateTunnelForce` con sus operaciones `Single`, la
masa actual del coche, la masa y longitud de su formación, su gálibo `Size` y
su velocidad anterior al paso. Conserva la resta original de la longitud del
coche en `LengthAhead + LengthBehind`. No se reemplaza por una longitud tomada
de la traza de referencia. Cada sentido lleva el intervalo reflejado y la
longitud original independiente.

Los gálibos son constantes en el runtime actual. Por eso el máximo histórico
nativo de ancho por alto equivale a ese producto fijo y se puede recomponer
junto con la pertenencia al túnel desde posición, formación y mapa. Guardar no
requiere un segundo inventario de coches ni un caché mutable. La recarga de
mercancías cambia la masa, pero no esas dimensiones. Un futuro gálibo animado
necesitará ampliar este contrato. Las geometrías no finitas, singulares o fuera
del dominio de la fórmula no pueden introducir NaN o infinito en el movimiento.

Las pruebas escritas cubren la importación de grupos, sus vías, los defaults,
las unidades, entrada y salida parcial de un coche, ida/vuelta y recomposición
JSON. La comparación aislada contra llamadas originales usa las fronteras
explicitadas del ensayo y un límite previo de 0,01 N. Ese ensayo comprueba la
fórmula; el recorrido completo debe usar solamente el estado Rust y los
metadatos originales, conservando sus seis límites anteriores.

La selección sobre el mapa dirigido se comprueba para intervalos vectoriales
sin partición adicional en circuitos de vía. Las particularidades del recorrido
nativo a través de varios `TrackCircuitSection` no se certifican por aprobar el
ensayo aislado.

La [captura original diagnóstica 08](fixtures/compatibility/twenty-20261015/lirr-native08-diagnostics/README.md)
midió **303,552 N** de túnel a los 400 s y **238,720 N** a los 500 s,
con fuerzas de curva y viento nulas en esas dos muestras. Registró 4.781
muestras dentro de túneles y un máximo de 304,768 N. Sus 164 canales anteriores
y 32.583 filas reproducen exactamente el SHA de la referencia 07. Esta
telemetría localizó una fuerza omitida; el runtime calcula ahora su fuerza
desde los metadatos y su propia posición, sin alimentar esas salidas nativas.

El [replay completo 10 con build 11](fixtures/compatibility/twenty-20261015/lirr-full-service-replay10/README.md)
aprobó los seis límites anteriores: **0,0398531564 m/s RMS** de velocidad
(límite 0,75), **11,486328125 m** de error máximo de odómetro (límite 45),
3114,406783 N RMS de fuerza motriz y 72,907216 N RMS de freno (límites 5000),
además de las dos presiones. Comparó el servicio físico de Brooklyn–Hempstead
completo: 1629,10 s, 31.361,20 m nativos, siete vehículos y dos plataformas.
El manifiesto liga los CSV comprimidos sin pérdidas, el protocolo, la ejecución,
los binarios y sus fuentes. Los fallos anteriores y el intento prematuro
inválido permanecen identificados; no se reemplazó una referencia para lograr
la aprobación.

El mismo build incluyó correcciones de la proyección gravitatoria, aritmética
y memoria del gobernador, fase MU, radio/integración del eje, masa nativa fija
sin animación continua y avance Single para la formación escalar/rígida. No se
atribuye toda la mejora a un único cambio ni se ajustaron fuerzas a una traza.
Esta aceptación se limita a esa formación y recorrido: no certifica la
puntuación ACT, el tráfico, todos los túneles de otras rutas ni el integrador
elástico propio de Rust.

Fuentes fijadas: `Signals.ProcessTunnels` en `Signals.cs:4679`,
`Train.ProcessTunnels` en `Train.cs:2625`, `TrainCar.Initialize` en
`TrainCar.cs:779` y `TrainCar.UpdateTunnelForce` en `TrainCar.cs:1232`,
Open Rails 1.6.1, commit `d16e670da333d26d2edfc97d5631a19dadf49ce5`.

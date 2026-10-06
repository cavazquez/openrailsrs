# Diésel, vapor, electricidad y cremallera

El juego puede cargar formaciones de los tres tipos. Eso comprende los modelos,
cabinas, animaciones y recursos que el autor incluyó; no garantiza que estén
implementados todos sus sistemas físicos. El menú audita cada formación antes
de iniciar y distingue recursos ausentes de funciones todavía parciales.

## Diésel

Es el modelo más desarrollado. Lee parámetros originales de motor, curvas de
tracción, RPM, consumo, resistencia y frenos por vehículo. Hay comparaciones
con Open Rails 1.6.1 para Pullman y Class 47, con alcance y tolerancias en
[OR_PARITY.md](OR_PARITY.md). El resultado de esas pruebas no certifica todas las
locomotoras, transmisiones, frenos dinámicos ni scripts de sus autores.

## Vapor

Hay caldera, presión, demanda de cilindros, consumo, animación y sonidos de
marcha. La alimentación del fuego es simplificada y el inyector repone agua
automáticamente: no reproduce una reserva finita completa del ténder ni todas
las tareas del fogonero. Hall y King sirven para probar carga, cabina y sonido;
su comportamiento físico completo todavía no está certificado.

## Electricidad

La alimentación es física e independiente de las mallas. Cada coche motor
consulta la tensión y el tipo de suministro bajo su captador. Sin tensión,
con captador incompatible, pantógrafo bajado, disyuntor abierto o motor aislado,
no genera nueva tracción. Puede seguir por inercia; no se congela ni se le aplica
un freno artificial. Los motores diésel de una formación mixta conservan su fuerza.

Se admiten catenaria, tercer y cuarto riel mediante declaraciones explícitas.
El importador conserva `Electrified` y `MaxLineVoltage` del TRK. Los escenarios
pueden declarar sectores sin corriente o cambiar el tipo de suministro, con
validación de bordes, tensiones y superposiciones. Un archivo de ruta sin datos
eléctricos no se electrifica automáticamente al elegir una formación eléctrica.

`O` sube o baja el pantógrafo y `J` abre o solicita el cierre del disyuntor.
Las teclas se pueden reasignar en F10, sin compartir funciones. Los captadores
por riel no necesitan pantógrafo. El HUD informa tensión y causa del corte; F8,
en la página de tracción, detalla cada vehículo. Las cabinas con instrumentos
nativos de tensión, pantógrafo y disyuntor usan el estado de la simulación.
El movimiento exterior del pantógrafo respeta el mismo progreso físico.

Se leen los retardos `ORTSPantographs / Pantograph / Delay`,
`ORTSCircuitBreakerClosingDelay` y `ORTSPowerOnDelay`. La partida arranca con los
sistemas preparados cuando el suministro es compatible. Después de un corte,
se respetan los retardos de elevación, cierre y alimentación. La reconexión
predeterminada es automática cuando vuelve la tensión y sigue solicitado el
cierre. Guardar y cargar conserva las transiciones y los controles, también
para el tráfico y el simulador sin visor.

El contenido MSTS antiguo suele indicar solamente `Type ( Electric )`: se
mantiene el captador aéreo predeterminado de OR. Un autor puede declarar
`ORTSRSPickup ( third_rail )` o `fourth_rail` en su ENG, o el escenario puede usar
`[[train.electric_pickups]]`. No se adivina el equipo por el nombre del modelo.
Los límites opcionales `ORTSRSMinimumVoltage` y `ORTSRSMaximumVoltage` permiten
restringir las tensiones admitidas; sin ellos se requiere tensión positiva.

El captador se representa en el centro del coche motor. No se calcula contacto
geométrico, arcos eléctricos, frecuencia de red, varios pantógrafos independientes,
alimentación desde otro coche, baterías de tracción ni motores auxiliares duales.
Tampoco se ejecutan los scripts C# originales de alimentación ni se certifica el
filtrado de tensión de OR. Tercer y cuarto riel son perfiles físicos declarativos:
no agregan geometría de conductor a una ruta que no la tenga.

La referencia fijada es **Open Rails 1.6.1**, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`. El oráculo
[openrails-electric.json](../oracles/openrails-electric.json) ejecuta sus DLL
originales: ocho puntos de movimiento del pantógrafo y doce estados de
alimentación principal. La captura repetida produjo exactamente el mismo JSON.
La coincidencia de estos subsistemas no certifica la física completa de una
locomotora eléctrica. Los sectores físicos sin tensión y captadores por riel
son una extensión explícita: el suministro predeterminado original obtiene
la tensión nominal del TRK, no de cada objeto visible de catenaria.

Para declaraciones TOML y prueba manual, ver
[PLAYER_MANUAL_TESTS.md, sección 37](PLAYER_MANUAL_TESTS.md#37-alimentación-eléctrica-pantógrafo-y-disyuntor).

## Cremallera

El elemento dentado entre los rieles en algunas pendientes es una **cremallera**.
Se engrana con un piñón de una locomotora equipada para ese sistema. Un tercer
riel eléctrico también puede estar cerca del centro, pero suministra corriente
y cumple otra función.

La tracción y el frenado por cremallera **no están implementados**. Importar su
modelo 3D no crea el engranamiento ni elimina el límite de adherencia. Harían
falta declaraciones de tramo, locomotora compatible, transición entre adherencia
y cremallera, velocidad admitida y frenos propios. No se atribuye esta función
a la referencia 1.6.1 usando documentación de versiones posteriores.

## Qué comprobar en una partida

Elegí Pullman/121, 1960/R Stock y Hall/King: verificá la auditoría, el modelo,
la cabina y sus controles. Los eléctricos necesitan un perfil de alimentación
que corresponda a su equipo; los ejemplos cortos prueban cortes y reconexión.
Para vapor, ver animación y oír las emboladas no demuestra gestión completa
de la caldera. La prueba de sonido está en [NATIVE_AUDIO.md](NATIVE_AUDIO.md).

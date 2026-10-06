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

Cada vehículo declarado `Type ( Diesel )` conserva su depósito, RPM y estados
detenido, arrancando, en marcha y deteniéndose. `K` acciona el primer motor;
el panel `B` y la formación `F9` permiten elegir otros. Arrancar requiere
batería, combustible y regulador cerrado. La formación inicia con motores
preparados; un arranque posterior tiene retardo. Cortar el motor elimina
su fuerza mientras el tren puede seguir por inercia. Aislar el mando múltiple
no cambia la identidad ni repone el combustible del motor.

Se leen `MaxDieselLevel`, `DieselConsumptionTab`, `DieselUsedPerHourAtIdle`,
`DieselUsedPerHourAtMaxPower`, `StartingRPM` y `StartingConfirmRPM`. La tabla
original es RPM → litros/hora, distinta del consumo específico g/kWh.
El motor consume también en ralentí y durante las fases con combustión;
un tanque vacío impide volver a arrancar. Guardar conserva el depósito y
la transición de RPM. La conversión nativa `gal` usa galones estadounidenses;
`g-uk` conserva los británicos.

Los archivos reducidos del Pullman jugable incorporan el tipo y las reservas
del original: 250 galones británicos en el DMBSA y la declaración final de
830 galones estadounidenses en el DMBSH. Las tablas de consumo disponibles
se conservan. Así los controles también están disponibles en el recorrido
predeterminado; elegir la formación nativa sigue siendo necesario para cargar
todos los parámetros y recursos de sus archivos originales.

El oráculo [openrails-traction-operation.json](../oracles/openrails-traction-operation.json)
compara 40 puntos de arranque, parada, RPM y caudal contra las DLL originales
1.6.1. Es un motor sin caja de cambios; no certifica todas las transmisiones,
motores múltiples dentro de un ENG, suministro auxiliar ni variación de masa
por combustible. Los `Pickup` nativos de diésel y las tomas `IntakePoint`
permiten abastecer desde el panel B; el consumo acumulado no se reinicia al llenar.

## Vapor

Hay caldera, presión, demanda de cilindros, consumo, animación y sonidos de
marcha. El agua de la caldera se distingue de la reserva finita del ténder.
El carbón pasa del ténder al fuego antes de quemarse. Los inyectores transfieren
agua, enfrían la caldera y necesitan presión; no crean agua. Sin fuego no
hay producción nueva y la presión puede caer hasta cero. Un nivel de caldera
del 15 % o inferior activa el fallo por falta de agua y corta la fuerza.

`B` abre corte independiente del regulador, pala, tiro, dos inyectores,
soplador y purgas de cilindros. El fogonero automático regula esos controles
con las mismas reservas. Accionar pala, tiro, inyectores o soplador pasa a
manual; el corte y las purgas permiten conservar la asistencia automática.
El panel se puede usar en marcha o en pausa. `F8` muestra cantidades y caudales.
Los instrumentos CVF disponibles leen presión, nivel, carbón, agua del ténder
y controles; los SMS reciben los eventos originales de inyectores, tiro,
soplador y purgas. El vapor de silbato requiere presión.

Se corrigen las unidades de presión sin sufijo (PSI), el volumen de caldera
en pies cúbicos y el radio de la rueda motriz del bloque Engine. Un diámetro
menor de dos metros sigue siendo un diámetro. Las cantidades del oráculo
se comparan con el lector STF original. `BoilerVolume` se aproxima a capacidad
de agua a 1000 kg/m³; no representa el volumen real de vapor y líquido de OR.

La energía de caldera, combustión e inyección sigue siendo un modelo de
conservación simplificado, sin las tablas termodinámicas completas de OR.
Solo hay una caldera agregada por formación; no se certifican locomotoras
compuestas, gestión independiente de doble tracción, condensación, toma de
agua en marcha. Los abastecedores nativos permiten reponer agua del ténder y
carbón, con sus tomas y caudales. Llenar el ténder no llena la caldera ni repara
un fallo por falta de agua. Hall y King sirven para probar contenido,
cabina y sonido; su física completa todavía no está certificada.

## Abastecimiento desde WORLD

`B` ofrece iniciar y cancelar el abastecimiento de la toma más cercana. Se leen
el tipo, cantidad, caudal, animación y rango de velocidad del `Pickup`, y el
desplazamiento y ancho del `IntakePoint` del ENG/WAG. La toma debe quedar a
menos de **2,5 m más la mitad del ancho declarado**, como en OR 1.6.1. Se usa
la posición propia de cada coche y su orientación `Flip`; solo entran los
vehículos acoplados al tren del jugador.

La cantidad y caudal originales se convierten de lb a kg; el diésel conserva
la densidad nativa de 0,8508 kg/L. La conexión respeta el tiempo de animación
antes de transferir. Un depósito completo, un abastecedor agotado, regulador
abierto, velocidad incompatible o alejamiento de la toma detienen la operación.
El brazo regresa al reposo; no reproduce el bucle completo de una grúa.

Las cantidades transferidas y reservas de estación se guardan. Al cargar, la
conexión se cancela para exigir un nuevo contacto válido con el escenario.
La reserva finita del abastecedor es una ampliación del comportamiento habitual
de OR. No se certifican tomas en marcha, suministros de mercancías, varios
ténderes independientes ni variación de masa por combustible.

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

## Escape, vapor y viento

Los emisores ENG originales conservan posición, orientación y tamaño. La nueva
partícula hereda la velocidad del coche, también con `Flip`, marcha atrás y
dinámica por vehículo. Una sección desacoplada y estacionada no hereda la
velocidad del tren que se aleja. El humo y el vapor quedan en coordenadas del
mundo y su velocidad se aproxima al viento mediante arrastre; mover la cámara
no arrastra la estela. El origen flotante desplaza las posiciones sin saltos.

Lluvia, nieve y estelas usan el mismo viento. Con clima del lugar se usa la
dirección meteorológica de la muestra vigente; con clima manual o sin una
muestra vigente se conserva el viento suave predeterminado. Pausar detiene el
movimiento y la emisión; reiniciar limpia las partículas antiguas. El tiempo
acelerado envejece la estela según los segundos de simulación y el nacimiento
se limita al último intervalo para evitar ráfagas tras la carga.

Es un efecto de presentación con un máximo de 512 partículas y un solo dibujo.
Los coeficientes de arrastre y ascenso son ajustes visuales: no se certifican
como dinámica de fluidos ni cambian la fuerza del motor o la adherencia de la
vía. La nieve sigue sin calcular hielo. La
[sección 44 de pruebas manuales](PLAYER_MANUAL_TESTS.md#44-humo-vapor-viento-y-movimiento-del-tren)
explica cómo comprobarlo en una formación original.

## Qué comprobar en una partida

Elegí Pullman/121, 1960/R Stock y Hall/King: verificá la auditoría, el modelo,
la cabina y sus controles. Los eléctricos necesitan un perfil de alimentación
que corresponda a su equipo; los ejemplos cortos prueban cortes y reconexión.
Para vapor, ver animación y oír las emboladas no demuestra gestión completa
de la caldera. La prueba de sonido está en [NATIVE_AUDIO.md](NATIVE_AUDIO.md).
Los ejemplos `traction_operation` prueban motores, fogonero y agotamiento sin
un viaje largo. Pasos y resultados esperados en la
[sección 38 de pruebas manuales](PLAYER_MANUAL_TESTS.md#38-vapor-y-diésel-reservas-arranque-y-fogonero).

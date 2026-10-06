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

**Hoy un eléctrico puede traccionar en una ruta sin electrificar.** La catenaria
y el pantógrafo son visuales; no hay conexión física entre la tensión de la vía,
el captador y la fuerza del motor. Bajar el pantógrafo tampoco corta por sí solo
la tracción. El menú lo advierte al auditar un eléctrico.

Una locomotora puramente eléctrica real necesita un suministro compatible:
catenaria o tercer/cuarto riel, según su equipo. Sin suministro pierde la
capacidad de generar nueva tracción y puede seguir por inercia hasta detenerse;
no queda fijada a la vía. Una batería o un motor auxiliar permitirían circular
sin alimentación exterior solamente si el tren los tiene y se modelan.

Que no se vea una catenaria no demuestra un corte eléctrico: puede faltar la
geometría, existir otro captador o tratarse de un tramo neutro. La futura
implementación necesita suministro y captadores separados de la escena 3D,
con pantógrafo, disyuntor, tensión y transiciones sin alimentación.

La referencia fijada es **Open Rails 1.6.1**, commit
`d16e670da333d26d2edfc97d5631a19dadf49ce5`. Su código
`ScriptedElectricPowerSupply.LineVoltageV` obtiene la tensión nominal del TRK;
el suministro predeterminado considera pantógrafo y disyuntor. Elevar el
pantógrafo en una ruta no electrificada genera un aviso. No hay que interpretar
eso como detección de contacto con cada malla de catenaria. El manual explica
el uso de [pantógrafo y disyuntor](https://open-rails.readthedocs.io/en/latest/driving.html).

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
la cabina y sus controles. Para eléctricos, la advertencia describe una
limitación real; circular sin catenaria o con pantógrafo abajo todavía no sirve
para probar paridad eléctrica. Para vapor, ver animación y oír las emboladas
tampoco demuestra gestión completa de la caldera. La prueba de sonido está en
[NATIVE_AUDIO.md](NATIVE_AUDIO.md).

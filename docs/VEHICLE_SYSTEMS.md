# Equipos declarados por vehículo

`[[train.systems]]` y `[[extra_trains.systems]]` añaden equipos a un índice estable de la formación. Las definiciones se validan antes de iniciar. Cambiar de cabina no cambia ese índice. Un coche desacoplado conserva sus estados, pero no recibe mandos de la sección en marcha.

Los campos ausentes mantienen el comportamiento previo. Se importan los retardos de puertas, las curvas y rampas del freno eléctrico, los pantógrafos y los gobernadores de bloques Diesel que declara el contenido. Las ampliaciones de batería, transferencia térmica, control de deslizamiento, viento y protocolos de mando múltiple requieren un perfil explícito. Esto evita asignar equipos modernos a material que no los tiene.

## Estado y autoridad

La simulación conserva puertas, alimentación, motores, frenos, depósitos, calderas y mandos. El visor usa esos valores para instrumentos y animación. Los cálculos no leen la cámara ni el nivel de detalle. Cada pantógrafo consulta la tensión a su posición; levantarlo no genera energía. El bus de pasajeros identifica un proveedor real por componente conectado.

Una locomotora con batería de tracción utiliza una fuente por tick: red, diésel o batería. La reserva se descuenta por el trabajo mecánico y las pérdidas de conversión. La recarga tiene límite de potencia y capacidad. Las fuentes no se suman para multiplicar la potencia del mismo vehículo.

El freno eléctrico utiliza las curvas y el retardo del equipo. La receptividad se reparte una vez entre los coches; la parte rechazada pasa al reóstato o al freno de servicio según el perfil. Fuerza, energía devuelta y calor utilizan el mismo límite de contacto rueda–riel. La protección de bloqueo representa un grupo de ruedas por vehículo: modula el cilindro y distingue rueda rodando de rueda bloqueada. No representa cada eje de todas las locomotoras de la comunidad.

La capacidad térmica, radiadores y umbrales diésel se identifican en el perfil. Los fallos son opcionales y tienen causa: nivel insuficiente o temperatura excesiva. Rearmar no elimina una causa que todavía existe. Los modelos propios de refrigeración no certifican la red completa de tuberías de un motor comercial.

Los ensayos de varios motores mantienen una reserva por motor declarado. La comparación de gobernadores nativos cubre RPM y curvas de potencia; no certifica una instalación de depósitos compartidos entre varios motores.

Las locomotoras de vapor de una misma formación conservan calderas y controles separados. Un perfil puede declarar `native_injector_mm=[10,13]`; se usan las tablas originales de caudal de OR 1.6.1. La transferencia a la caldera descuenta agua del ténder, vapor y calor. Se conserva el alcance del modelo de caldera existente; no se afirma reproducción completa de toda la termodinámica de OR.

La compatibilidad de mando múltiple requiere el mismo protocolo o autorización explícita en ambos equipos. Sus retardos usan el reloj de la simulación. Cabina, instrumentos, aislamiento y conexiones conservan sus propios estados.

## Ejemplos

- `examples/electric_supply/scenario_two_pantographs.toml`: colectores con posiciones y tiempos distintos.
- `examples/electric_supply/scenario_battery.toml`: reserva, recarga y receptividad.
- `examples/electric_supply/scenario_dual.toml`: red, diésel y batería sin duplicar esfuerzo.
- `examples/traction_operation/scenario_equipment.toml`: dos motores y equipos de frenado declarados.
- `examples/traction_operation/scenario_native_motors.toml`: gobernadores de bloques Diesel nativos.
- `examples/traction_operation/scenario_motor_fault.toml`: circuito pequeño propio para una prueba térmica corta.
- `examples/traction_operation/scenario_multiple_unit.toml`: dos protocolos compatibles y un segundo de retardo.
- `examples/traction_operation/scenario_steam_equipment.toml`: dos calderas e inyectores de tamaños distintos.
- `examples/traction_operation/scenario_freight_traffic.toml`: masa de carga, tres paradas y circulación paralela.
- `examples/player_lab/scenario.toml`: puertas, cámara de pasajeros y límites de circulación.

Los recursos gráficos de estos laboratorios son propios y se pueden distribuir. El contenido instalado de un autor permanece en la biblioteca del usuario.

## Referencias comprobadas

La versión continúa fijada en `d16e670da333d26d2edfc97d5631a19dadf49ce5` de OR 1.6.1. Las capturas guardan hashes de fuentes y algoritmos, condiciones y tolerancias. Los scripts se niegan a sobrescribir una referencia existente.

- `vehicle-equipment-or-1.6.1.json`: 480 pasos de pantógrafo, 640 de inclinación y 56 casos de esfuerzo del freno eléctrico.
- `voltage-filter-or-1.6.1.json`: filtrado de tensión, pérdida y reconexión.
- `cant-legacy-local-or-1.6.1.json`: tablas antiguas, velocidades por categoría y transiciones de peralte.
- `steam-injectors-or-1.6.1.json`: 402 muestras de seis tablas de inyectores e interpolación original.

Los ensayos de extensiones comprueban autonomía, conservación de energía y masa, presión y distancias de parada, persistencia, clima de jugador y tráfico, protocolos y servicio completo de mercancías. Los oráculos anteriores conservan sus límites. Las capturas gráficas y el contrato de rendimiento son comprobaciones separadas; no sustituyen la aceptación física.

El escaneo de puertas tiene presupuestos de archivo y descompresión. Una forma que exceda el presupuesto se omite en la detección de equipos; no se expande sin límite para iniciar la formación.

Los manifiestos de recarga de sectores se generan en la caché del jugador. La ruta puede permanecer en sólo lectura. El cargador reconoce también nombres WORLD con cero negativo y conserva las dos partes del sector: objetos y terreno.

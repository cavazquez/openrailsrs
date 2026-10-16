# Bogies y ejes: geometría, fuerzas y presentación (#326)

La orientación del mesh y la respuesta mecánica usan canales separados. La animación toma las tangentes de vía y aplica `train.presentation.bogie_yaw_limit_rad`, un límite visual por coche. No lee ni integra la suspensión lateral. El paso físico toma geometría numérica del ENG/WAG, radio y peralte físico; no usa rotaciones de Bevy, pivotes de matrices ni el límite visual.

`openrailsrs-train::bogie_geometry` carga trocha, apoyos, número de ejes/bogies, batalla rígida, centro de gravedad y pestaña. Un valor autoral gana a los valores predeterminados. Sin la geometría del shape, la inferencia numérica de ejes/apoyos es una aproximación declarada: no certifica la misma inicialización que un shape original arbitrario. `support_offsets_m` permite describir apoyos asimétricos en coordenadas de formación, primero delantero positivo y luego trasero negativo. Invertir la formación transforma ambos apoyos.

La mecánica opcional se declara por vehículo:

```toml
[[train.bogie_dynamics]]
vehicle = 0
support_offsets_m = [10.0, -10.0]
shared_with = 1
[train.bogie_dynamics.lateral_suspension]
stiffness_npm = 1000000.0
damping_nspm = 100000.0
travel_m = 0.03
maximum_attack_rad = 0.04
```

El bloque visual `presentation.jacobs` sólo vincula el mesh de bogie compartido. La unión física requiere `bogie_dynamics.shared_with`. Los dos cuerpos deben ser adyacentes, usar la misma trocha y describir el mismo apoyo con una discrepancia máxima de 1 mm. Los ejemplos Jacobs existentes incluyen ahora ambas declaraciones.

Cada apoyo recibe la proporción de masa determinada por sus brazos respecto al centro del cuerpo. La aceleración lateral es `v²·curvatura − g·peralte/trocha`, con el signo del peralte relativo a la curva. Las cargas de los raíles izquierdo/derecho incluyen la transferencia a la altura del centro de gravedad. La velocidad inversa conserva el término centrífugo; la dirección de la vía y la orientación del cuerpo determinan el signo lateral.

La suspensión, extensión propia de Rust, integra un resorte y amortiguador mediante un paso implícito. El tope limita el recorrido, registra la reacción adicional y anula la velocidad que lo atraviesa. Los estados exponen desplazamiento, velocidad, esfuerzo solicitado, reacción lateral, carga por raíl y avisos de recorrido, ángulo de ataque y descarga de rueda. Las cargas normales se limitan entre cero y el peso sustentado; al descargarse una rueda se conserva el peso total y se señala que la transferencia solicitada superó ese límite. Un apoyo compartido combina las masas y cargas de ambos cuerpos, se integra una vez y reparte su reacción por masa sustentada. Usa la mayor batalla declarada y el menor recorrido/límite de ataque; las rigideces y amortiguaciones se suman en paralelo. No se añade resistencia longitudinal de curva por segunda vez.

El diagnóstico cuasiestático `NativeLateralState` porta la rama lateral de `TrainCar.UpdateTrainDerailmentRisk` de OR 1.6.1, incluido su comportamiento de conservar el último esfuerzo lateral cuando no hay coche delante. El ensayo dedicado prescribe los ángulos y esfuerzos del acoplador; no compara toda la geometría visual de mangueras ni el filtrado completo de un tren original. En conducción, los apoyos usan fuerzas instantáneas y direcciones obtenidas de la geometría de vía. Ese acoplamiento a la suspensión es una extensión, no un oráculo de un viaje OR. Cuando `asin(2·batalla/radio)` queda fuera de su dominio real, `native_domain_valid=false` conserva el último diagnóstico finito; los estados y límites de los apoyos siguen disponibles.

Los estados se guardan por identidad de coche, incluidos los cuerpos estacionados al cortar una sección. El formato nuevo lleva `bogie_state_version=1` y exige la colección completa; se rechazan estados no finitos, recorridos fuera del modelo o colecciones borradas antes de mutar la sesión. Una partida anterior, sin marcador ni estados, migra explícitamente a suspensión neutra. El primer paso recalcula cargas desde su posición y velocidad restauradas, sin aplicar impulsos ni modificar la velocidad longitudinal.

## Referencia y pruebas

`docs/fixtures/compatibility/bogie-dynamics-or161` conserva la captura real de 48 casos: cuatro perfiles numéricos propios, recta, peralte, marcha inversa, exceso de peralte, tracción/compresión de acoplador, parada, ausencia de coche delantero, contadores nulos y superación de Nadal con inicio del temporizador. El helper invoca el método de las DLL originales. `SpeedMpS` y su cache nativo `AbsSpeedMpS` se prescriben conjuntamente, como hace `TrainCar.Update`. Se evalúa una llamada por caso; no se presenta como validación nativa de un descarrilamiento completo ni de un viaje.

Los límites se declararon antes de comparar: fuerzas 0,25 N + 3×10⁻⁶ relativo; ángulos 10⁻⁷ rad; coeficientes y distancia 10⁻⁵; tiempo 10⁻⁶ s; booleanos exactos. El manifiesto registra el commit OR `d16e670da333d26d2edfc97d5631a19dadf49ce5`, hashes de DLL, fuentes, helper, script, input, protocolo y salida. El hash SHA-256 de la referencia es `a491890c45d1ec224d25dac2221550866839f15ce3295abec9b439e5ffcb9b3a`.

Los intentos de preparación permanecen en `tmp/next-ten-20261015/bogies`: 01 falló por el socket Wine bloqueado por el sandbox; 02 falló al compilar un campo protegido; 03/04 tenían el cache de velocidad sin inicializar y no cubrían flags de movimiento. Sólo 05 es la referencia completa. Se usó `OracleWine`, sin conexión con la pantalla o audio del usuario, con `Graphics=null` y `ShowCrashDialog=0` verificados, sin drivers prohibidos y sin procesos privados restantes. No se alteraron instalaciones ni fuentes originales.

Pruebas dirigidas:

```sh
cargo test -p openrailsrs-train bogie_geometry
cargo test -p openrailsrs-sim --lib bogie_dynamics
cargo test -p openrailsrs-sim --test bogie_dynamics_oracle --test bogie_drive --test articulated_train
```

Las pruebas propias cubren equilibrio, topes y límites por modelo, conservación de carga del apoyo compartido, fuerzas de conducción en la ruta sintética con peralte físico, independencia ante cambios visuales, guardado/reanudación y rechazo de estados adulterados. Los oráculos históricos de peralte se conservan sin regenerarlos.

## Conducción y comprobación visual

El agente principal ejecutó el laboratorio con RX 7600/Vulkan en una pantalla
Weston privada. Las tres vistas están en
`fixtures/visual/bogie_dynamics_20261015/`: tras recorrer 35,66 y 75,26 m y una
vista lateral a 35,07 m, con pausa al alcanzar el punto. Los dos cuerpos siguen
la curva, permanece un único enlace de mesh `BOGIE2` compartido y los extremos
flexibles coinciden con error registrado de 0 m. No quedaron shaders ni subidas
GPU pendientes. El recibo conserva los valores exactos de cada captura y los
hashes del ejecutable, fuentes y archivos.

Las cajas rojas y azules son geometría de diagnóstico propia; estas imágenes no
certifican el aspecto de trenes originales ni un contacto detallado rueda/raíl.
La marcha inversa, las fuerzas y la restauración se comprueban por separado en
las pruebas CPU. Las capturas usan el visor anterior a la corrección final del
importador ACT, que no interviene en este ejemplo sintético; las fuentes de
render y física registradas permanecieron iguales.

Abrir `target/debug/openrailsrs-viewer3d examples/player_lab/scenario_bogie_mechanics.toml --live` con el visor compilado por el flujo normal del proyecto. La ruta y los vehículos son sintéticos propios. Comparar con `scenario_jacobs.toml` en recta. Acelerar, observar los dos apoyos al entrar y salir de las curvas, frenar, invertir la marcha detenido y guardar/reanudar dentro de una curva. El mesh compartido debe permanecer sobre su apoyo, sin escalar ni separar las ruedas. El giro visual debe respetar los límites declarados de 0,12 y 0,20 rad; cambiar ese límite no debe alterar la conducción ni sus fuerzas. Pausar debe detener la física y conservar los estados.

F8 permite revisar curvatura y peralte; todavía no presenta los valores de
fuerza de cada apoyo. La captura numérica nativa, la conducción CPU y las vistas
GPU tienen alcances separados y no certifican todas las locomotoras originales.

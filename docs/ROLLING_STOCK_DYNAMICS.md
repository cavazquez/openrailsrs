# Frenos, cambios y acopladores

Esta ampliación cubre los issues #222, #225, #322, #323 y #398. Los sistemas se
ejecutan en Rust; Open Rails 1.6.1 aporta las referencias de compatibilidad.
Las capturas guardan entradas, versión, huellas de los binarios y tolerancias
declaradas antes de comparar. El contenido original sigue en la biblioteca del
jugador, fuera de Git.

## Freno independiente de vacío

El controlador del ENG acciona el grupo de locomotora y ténder. Los coches
remolcados conservan el freno de tren. Se leen las muescas, su carácter continuo
o discreto y las velocidades de aplicación, liberación y emergencia. Un
controlador ausente o `Dummy` conserva ese significado.

La tubería, el cilindro y el depósito mantienen presiones absolutas. La posición
del pistón cambia la compensación de volumen del depósito; el guardado conserva
ese estado y el mando independiente. Los valores heredados 21/25 del controlador
de vacío se interpretan en inHg, con la misma normalización del original.

Para probarlo:

1. Abrí el ejemplo **Freno independiente de vacío**
   (`examples/vacuum_independent/scenario.toml`).
2. Soltá el freno de tren con `;`. En `F9`, observá las fuerzas de locomotora y
   coche. Con `]` aplicá el independiente; `[` lo libera. También están en `B`.
3. Debe crecer el esfuerzo de la locomotora mientras el coche queda liberado.
   El freno de tren debe aplicar ambos vehículos.
4. Guardá con el independiente aplicado, cargá y verificá que mantiene su mando
   y continúa desde las mismas presiones. Liberalo y comprobá la recuperación.

## Caja de cambios diésel

Las marchas usan las velocidades y esfuerzos declarados por el autor. Las
velocidades sin unidad de MSTS están en mph; también se admiten unidades
explícitas. La relación se calcula con las RPM máximas y, para la transmisión
mecánica avanzada, el radio de rueda. Marcha solicitada y engranada son estados
distintos: el embrague puede demorar el cambio.

Las cajas heredadas manuales, automáticas y semiautomáticas conservan el orden de
actualización del controlador, el acoplamiento entre eje y motor, los avisos de
sobrerrégimen y los parámetros de deriva. La comparación de subsistemas incluye
1.293 puntos nativos: fuerza, marchas, embrague, RPM y caudal. Incluye la Class
101 original y una variante numérica de unidades identificada como tal.

La verificación del recorrido utiliza la Class 101 original sobre SCE, con los
mismos mandos de aceleración, cuatro marchas, deriva y frenado durante 300 s.
`gearbox_native_drive` calcula movimiento, fuerzas y combustible en la sesión
del juego; sus límites están fijados en el archivo de tolerancias de la captura.

El ensayo completo de 6.001 muestras queda dentro de esos límites: error máximo
de velocidad 0,0475 m/s frente a 0,1; odómetro 0,95 m frente a 3; combustible
0,0014 L frente a 0,05; cambios de marcha coincidentes. Son límites y resultados
de este vehículo, itinerario y secuencia de mandos.

La resistencia en curvas utiliza el peralte físico generado por el algoritmo
nativo ya existente. Se importa en un canal separado del ángulo visual y respeta
los dos bogies, la trocha y el sentido del recorrido. El modo de física simple
permite reproducir la configuración de la referencia; la configuración habitual
conserva el modelo de ejes Pacha.

Para probar la interacción sin instalar ese contenido:

1. Abrí **Diésel: caja manual de cuatro marchas**
   (`examples/traction_operation/scenario_gearbox.toml`).
2. Poné el inversor adelante, soltá el freno y acelerá con la caja en `N`.
   El motor consume combustible y sube de RPM; el tren permanece detenido.
3. `E` solicita una marcha superior; `Z` baja hasta neutro. En `B` aparecen
   marcha engranada, solicitada, RPM de motor/eje y deslizamiento del embrague.
4. Acelerá en primera, cerrá el regulador y solicitá segunda. Debe cambiar el
   régimen y el esfuerzo. Repetí hasta cuarta; no debe aparecer una quinta.
5. Guardá mientras hay un cambio solicitado y cargá la partida. El cambio
   pendiente y el combustible deben continuar desde ese punto.

`E` y `Z` son los valores iniciales; `F10` permite reasignarlos. Al actualizar
preferencias anteriores se preservan las teclas que el jugador ya usaba. Las
indicaciones `GEARS` del CVF reciben la marcha de la cabina activa y respetan
el orden inverso declarado por el autor.

Las transmisiones mecánicas avanzadas A–D tienen lectura de parámetros y
controles diferenciados; la captura de Class 101 certifica la transmisión
heredada. Sus embragues, gobernador, rueda libre y fallos especiales requieren
capturas adicionales por familia antes de afirmar la misma paridad.

## Acopladores y topes

Con `multi_body = true`, cada unión usa los extremos de sus vehículos y respeta
`Flip`, curvas de tracción/compresión, holgura, rigidez y límites. Los dos
extremos transmiten la misma fuerza y sus recorridos se suman en serie. Las
rigideces tradicionales se leen en N/m; las avanzadas definen fuerzas por zona.

La compresión utiliza el mismo conjunto físico para los topes. Su recorrido
finito limita la penetración y el impulso de contacto conserva el momento.
El diagnóstico registra energía absorbida, esfuerzo excesivo e impactos que
superan la capacidad de la curva. Al guardar se conserva el historial de carga;
al restaurar se cotejan los parámetros con el equipo original.

La envolvente y las restricciones de contacto se contrastan con getters,
`Train.ComputeCouplerForces` y `Train.AddCouplerImpulseForces` de las DLL. La ley
pasiva de descarga por puntos de retorno y la contabilidad explícita de energía
son extensiones de Rust. Sus pruebas comprueban conservación, disipación y
continuidad al guardar; no se presentan como una captura de histéresis de OR.

Para probarlo:

1. Abrí **Acopladores: holgura, frenado y topes**
   (`examples/traction_operation/scenario_couplers.toml`).
2. Desde el exterior, acelerá suavemente. Las holguras deben tomarse sin separar
   coches ni provocar saltos permanentes. `F8`, página **Fuerzas**, muestra cada
   unión, su extensión, esfuerzo y energía.
3. Cortá tracción y frená. Las uniones deben pasar a compresión y el tren debe
   quedar detenido; el freno no debe impulsarlo hacia atrás.
4. Guardá durante la marcha, cargá y continuá frenando. Las curvas y la historia
   de cada unión deben mantenerse.
5. Detenido, asegurá la parte remolcada con el freno de mano y separá/reacoplá
   desde `F9`. Cada unión debe recuperar el equipo que corresponde a sus coches.

## Reproducir las pruebas

Las pruebas con referencias guardadas funcionan sin Wine ni .NET:

```bash
cargo test -p openrailsrs-sim --test gearbox_oracle --test gearbox_drive
cargo test -p openrailsrs-sim --test vacuum_independent
cargo test -p openrailsrs-sim --test native_couplers --test couplers_live_drive
python3 -m unittest discover -s scripts -p test_rolling_stock_oracles.py
./check.sh
```

El ensayo de Class 101 necesita el contenido original instalado:

```bash
cargo run -p openrailsrs-sim --example gearbox_native_drive -- \
  --content-root '/ruta/Content/Demo Model 1' --out-dir tmp/class101-new
```

La salida debe ser nueva y estar fuera de la instalación. El comando verifica
las huellas ENG/PAT y falla cuando supera las tolerancias previas; conserva la
traza y el informe para diagnosticar una diferencia.

Las capturas nuevas de vacío y acopladores usan un prefijo Wine privado,
controladores de ventanas/audio deshabilitados, entorno sin conexión al
escritorio y limpieza en `finally`. El fallo controlado de aislamiento comprueba
que una excepción termina el proceso y que no quedan procesos del prefijo.
Las ejecuciones fallidas se conservan como fallos, sin registrarlas como paridad.

El catálogo `oracles/rolling-stock-families.json` separa estas familias de las
comparaciones aún pendientes, como termodinámica completa de vapor, protecciones
eléctricas específicas y física completa del recorrido LIRR. Las referencias
aprobadas de Pullman y Class 47 conservan sus bytes y tolerancias.

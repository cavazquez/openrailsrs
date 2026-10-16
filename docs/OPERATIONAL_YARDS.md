# Operaciones de patio: porciones, depósitos y mesas

Los ejemplos son contenido propio de openrailsrs. No distribuyen rutas ni material descargado. La referencia de compatibilidad sigue siendo Open Rails **1.6.1**; las mesas gráficas, los retardadores y el contacto entre trenes son extensiones Rust declaradas. El integrador de cada vehículo conserva sus frenos, gravedad, Davis, motores y acopladores. Estos ejemplos se ejecutan con **LiveDrive/LiveTraffic**: los runners antiguos rechazan explícitamente un patio en lugar de ignorarlo.

La fase conjunta aprobó los **20 ensayos CPU de patios**: recorridos reales, guardados, errores y conservación de material. También se capturaron y revisaron cuatro vistas exteriores relevantes de las mesas, con vehículo transportado, pausa, restauración y cambio de cámara/origen. No se presenta una captura interior de cabina ni una medición del coste marginal de la mesa; los contadores guardados corresponden a toda la aplicación.

## Abrir un ejercicio

En **Nueva partida**, elegí **Sin dato de país** o **Todos los países** y buscá la ruta del ejercicio: `meeting` para encuentros, `pool` para depósito, `runaround` para rodeo, `turntable` para mesa, `transfer` para plataforma y `gravity` para clasificación o recuperación. Después elegí el servicio por el nombre indicado abajo. Son los archivos independientes `scenario*.toml` de `examples/operational_yards/`; el catálogo los descubre junto a los demás ejemplos del proyecto.

Abrí **F9** → **Patio, mesas y clasificación** para las órdenes; **M** muestra vías, formaciones y reservas. **P** pausa. El jugador comienza detenido; aplicar freno y poner neutro antes de cambiar de conductor.

Los ejercicios de tripulación AI, pool, clasificación y recuperación colocan al jugador en una vía de observación separada. No es necesario simular un conductor dentro de un vagón sin cabina para dar una orden a su tripulación.

## Porción móvil: `scenario_meeting.toml` — #217

1. Elegí «Horario: encuentro con una porción móvil».
2. En exterior, observá `incoming`, que parte de 150 m, y `receiver`, que espera a 450 m. El primer tren debe recorrer la vía hasta el enganche trasero del otro, a 430 m. La tripulación sólo cambia regulador, freno e inversor.
3. Guardá cuando `incoming` esté entre 300 y 400 m. Reanudá: debe seguir desde allí, con los mismos vehículos y consumibles.
4. Ambos deben detenerse antes de transferir la porción. El contacto automático exige distancia menor a **8 mm** y velocidad menor a **0,005 m/s**; el enganche normal de F9 conserva su validación habitual.
5. F9 debe mostrar una sola formación `receiver`, con las identidades originales de ambos trenes. Se conectan el freno y el bus en el nuevo enlace. El historial guarda quién recibió a quién; guardar y reanudar no crea otra locomotora.

El importador de horarios nativos convierte `$pickup=otro_servicio` en estos encuentros, también cuando el receptor es AI. Vincula el marcador al vector original y al subtrayecto del receptor. Las operaciones originales y sus recursos permanecen intactos. El encuentro espera a los actores físicos; informa incompatibilidad, ausencia, contacto pendiente o vencimiento. Respeta las señales y el TCS nativos: una orden de encuentro no concede permiso para rebasar una señal roja ni otro tren.

Los comandos nativos aún no admitidos, un actor ausente y una preparación previa incompatible generan errores explícitos. No se afirma soporte completo de todos los dialectos de `$attach/$transfer` ni de todos los horarios comunitarios.

## Pool: `scenario_pool.toml` — #218

1. Elegí «Depósito: reservar, despachar y devolver material».
2. F9 → Patio → reservar `first-duty` en `depot`. Sólo hay una locomotora física: intentar reservar `second-duty` simultáneamente debe indicar que no queda material disponible.
3. Guardá y reanudá con la reserva pendiente. La misma locomotora debe seguir reservada, sin una segunda copia.
4. Despachá `pool-loco`. Su tripulación habilita los equipos por las operaciones habituales. La locomotora recorre la salida, invierte en la vía de maniobras y vuelve físicamente al depósito.
5. Cuando termine la ida y vuelta y esté detenida íntegramente dentro de `storage` (20–180 m), pulsá devolver. Esta orden incluye poner neutro, cerrar el regulador y aplicar todos los frenos de mano por su tripulación. Devolver antes de terminar el recorrido, en movimiento, fuera del depósito o con una formación incompleta debe fallar sin perder material.
6. Reservá `second-duty` y despachá de nuevo. La posición se proyecta sobre la salida sólo si conserva todos los coches en la misma vía; el combustible y el odómetro continúan. El pedido ya servido no puede reservarse otra vez.

El pool guarda reservas exclusivas, pedidos completados, devoluciones y material real. No fabrica un tren cuando se agota. El ejemplo define el depósito y sus pedidos en TOML; no se presenta como una implementación completa de todos los archivos `.pool-or` ni de la creación forzada de stock de Open Rails.

## Rodeo AI: `scenario.toml` — #338

1. Elegí «Patio: una tripulación AI rodea sus coches».
2. En F9 → Patio, iniciá `crew-loop`.
3. La locomotora debe asegurar y cortar dos vagones en la vía superior, avanzar hasta librar el cambio derecho, invertir por `bypass1/bypass2/bypass3`, librar el cambio izquierdo y volver a invertir.
4. Los dos vagones deben permanecer en sus coordenadas originales mientras la locomotora rodea. Guardá durante el paso por la vía paralela y reanudá.
5. La locomotora recoge la sección por su frente, reconecta los equipos y sale empujando los mismos vagones. F9 conserva su cabina y las identidades físicas tras cambiar el orden de los coches.
6. El estado debe indicar rodeo completado sólo al terminar el recorrido con los tres vehículos reunidos. Un cambio ocupado o reservado, un plan discontinuo y un corte incompatible impiden la maniobra con diagnóstico.

El plan es una secuencia explícita de vías reales, cortes y recogida. La AI no recibe una nueva posición ni mueve los coches para resolver la maniobra. Una nueva ruta requiere su propio plan; no se afirma planificación universal automática de cualquier instalación comunitaria.

## Mesa giratoria: `scenario_turntable.toml` — #344

1. Elegí «Mesa giratoria: traslado y salida».
2. La locomotora debe estar completa sobre el tablero de 30 m, detenida, sin tracción y con freno aplicado al menos al 50 %.
3. F9 → Patio → mesa `turntable` → puerto `salida`. El giro dura aproximadamente 10,5 s simulados. La locomotora debe girar con el tablero; la cabina y la cámara exterior siguen el mismo movimiento.
4. Pausá a mitad del giro: vehículo y mesa deben permanecer inmóviles. Cambiá entre cabina y exterior, guardá y reanudá; la orientación intermedia debe conservarse.
5. Hasta alinearse se bloquean todos los accesos. Al terminar, la vía activa pasa a `deck1`, sin cambiar odómetro, combustible, identidad ni destino. Luego se puede conducir por la salida.
6. Probar puerto inválido, formación sobresaliente, sobrecarga, movimiento ya iniciado o reserva ajena: la orden debe rechazarse antes de mutar la formación.

Las coordenadas del tablero pertenecen al mundo ferroviario. El visor aplica la transformación de la ruta y el origen flotante una sola vez, también a las cámaras. Los puertos deben coincidir con sus vectores reales: 1 mm de longitud y 5 cm de posición admiten sólo el error geométrico declarado de la configuración. La rotación no sustituye una vía faltante.

## Plataforma: `scenario_transfer.toml` — #345

Repetí el procedimiento de la mesa con `transfer`. El tablero se desplaza **25 m** a **1,5 m/s**, aproximadamente 16,7 s simulados, conservando su rumbo. Al finalizar se alinea con el otro vector. Guardar durante el desplazamiento, pausa, cambios de cámara y controles de capacidad tienen el mismo contrato.

Los ejemplos usan puertos declarados en el escenario. No se afirma importación universal de todos los modelos de moving tables de WORLD. Una formación con cremallera no puede cambiar de itinerario por una mesa sin un mapa de cremallera específico para cada puerto: la operación se rechaza antes de mutar.

## Clasificación por gravedad: `scenario_gravity.toml` — #346

1. Elegí «Clasificación por gravedad: elegir vía y retardar».
2. F9 → Patio → `release-wagons` → elegir clasificación A o B, antes de liberar. Debe conservarse la posición inicial de los dos vagones. Los cambios ocupados o reservados no se mueven.
3. Liberá los vagones. La pendiente real es **−4 %** y el freno inicial declarado **0 %**. No hay locomotora ni velocidad impuesta.
4. El retardador de la vía elegida actúa entre 60 y 280 m, al 60 % por defecto. Deben disminuir velocidad y detenerse con la formación completa dentro de la vía. F9 muestra fuerza, intensidad y energía absorbida; un retardador no consume trabajo con ruedas detenidas.
5. Pulsá asegurar en vía de clasificación. La tripulación aplica frenos de mano; F9 debe aceptar sólo el destino correspondiente a la vía elegida. Guardá durante el descenso y después de asegurar; reanudar conserva posición, energía e identidad.
6. La ocupación incluye todos los vagones, también cuando ruedan libres. Un vagón sin frenos no se detiene por una ayuda de conducción ante una reserva: sus frenos físicos sí siguen actuando.

El contacto entre trenes usa sus huellas sobre el mismo vector, masa e impulso inelástico. Subdivide el tiempo al aproximarse; no corrige posiciones reubicando stock. Registra velocidad relativa, impulso y energía absorbida, y ordena frenado físico; impactos superiores a 0,5 m/s activan emergencia. La prueba de impacto con velocidad inicial prescrita es una prueba del solver, separada de la conducción completa. Es una extensión de contacto Rust, no una captura de colisiones de la DLL de OR.

Los destinos A/B se declaran como alternativas exclusivas mediante `simulation.rail_operations.yard_delivery_alternatives`. Deben referirse al mismo servicio y a las mismas identidades, en zonas físicas disjuntas. Asignar los mismos vagones a dos entregas sin ese grupo sigue siendo un error; no se cuenta una entrega en ambas vías.

La gravedad, Davis, frenos y acopladores comparten el integrador existente, contrastado por las suites de OR 1.6.1 del proyecto; las curvas nativas mantienen sus referencias y tolerancias anteriores. Los retardadores, clasificación y ledger de impacto tienen pruebas propias: no se inventa un oráculo OR para ellos.

## Recuperación opcional: `scenario_recovery.toml` — #348

1. Elegí «Recuperación opcional: vagones sin control».
2. Antes de 60 s, colocá la protección `secure-on-slope`. Esa ocupación bloquea la vía a los otros servicios; no crea un freno invisible para los vagones descontrolados.
3. Ajustá `crest-retarder` al 100 % y liberá `release-wagons`. La insuficiencia de frenos sólo existe en este incidente declarado.
4. Esperá la detención física de ambos vagones dentro de 200–299 m de `slope`; luego asegurá la recuperación. Debe comprobarse protección, velocidad, huella completa y ausencia de impactos superiores al límite de 0,5 m/s.
5. Guardá durante el movimiento y reanudá. No se puede iniciar la misma release otra vez ni borrar un impacto para aprobar la recuperación.
6. Intentar recuperar sin protección, tarde, en movimiento, fuera de zona, con un impacto excesivo o sin el actor independiente debe dar diagnóstico. El desacople ordinario continúa rechazando trenes en movimiento y vagones sin asegurar.

## Validación CPU

`crates/openrailsrs-sim/tests/operational_yards.rs` aprobó **20/20** en la tercera ejecución dirigida conjunta, en 1,35 s de ejecución sin contar la compilación. Incluye los siete ejemplos, movimientos por vías reales, recogida de identidades originales, dos salidas y devoluciones del mismo material, guardados JSON, reservas exclusivas, pausa, sobrecarga, definiciones corruptas, protección y colisiones con frenado prolongado. Los intentos anteriores y sus fallos se conservaron: se corrigieron la asignación alternativa de destinos, la autorización de inversión AI, la confirmación del encuentro en su mismo tick y la parada completa del pool; no se redujeron las exigencias de contacto ni se reubicaron vehículos.

Por separado, `physical_extensions` aprobó **14/14** y `twenty_joint_physics` **1/1**. Son ensayos propios de extensiones físicas. Las **3 comparaciones con llamadas originales de OR 1.6.1** en `physics_seven_native` se ejecutaron explícitamente y pasaron: tablas de vapor, emergencia/depósitos/rearme y propagación de freno de tres coches. En el primer comando estaban ignoradas; ese estado no se contó como aprobación. Usan la captura verificable de `docs/fixtures/compatibility/twenty-20261015/physical-native05`, sin instalar Wine para repetir la comparación. No convierten los retardadores o el contacto Rust en funciones originales de OR.

Desde la raíz del repositorio, para reproducir estas suites CPU:

```bash
cargo test --workspace --all-features --test operational_yards -- --test-threads=1
cargo test --workspace --all-features --test physical_extensions --test twenty_joint_physics -- --test-threads=1
cargo test --workspace --all-features --test physics_seven_native -- --include-ignored --test-threads=1
```

Los recibos locales de esa ejecución están en `tmp/twenty-20261015/directed-tests03.log`, `directed-tests03-result.json` y `physics-native-tests03.log`. Las pruebas locales de impulso están en `yard_gravity.rs` y las del importador de encuentros nativos en `import_timetable.rs`. El visor añade pruebas ECS del marco de coordenadas, origen y pausa de las mesas. La evidencia gráfica siguiente es independiente de esas pruebas CPU.

## Vistas de las mesas ejecutadas

Se publican cuatro PNG auténticos de 1280×720: [giro intermedio](fixtures/compatibility/twenty-20261015/operational-yards/turntable-mid.png), [giro terminado](fixtures/compatibility/twenty-20261015/operational-yards/turntable-final.png), [traslación intermedia](fixtures/compatibility/twenty-20261015/operational-yards/transfer-mid.png) y [traslación terminada](fixtures/compatibility/twenty-20261015/operational-yards/transfer-final.png). Se ven el tablero y la carga unidos durante el movimiento, y la salida alineada al finalizar. Son modelos propios de laboratorio, renderizados con Vulkan en una AMD Radeon RX 7600.

El [recibo compacto](fixtures/compatibility/twenty-20261015/operational-yards/qa-summary.json) registra el binario, hashes, estado, controles y transformaciones reales. Las órdenes mantuvieron regulador cero, freno al 100 % y neutro; pausaron el movimiento y restauraron un `SessionSnapshot` serializado en JSON. La cámara pasó a 5000 m y regresó a órbita, activando el origen flotante de producción. Los cuatro procesos aceptados y sus compositores privados terminaron con código 0. Estas son vistas exteriores relevantes y cambios entre cámara libre y órbita; no se deduce una prueba de cabina interior de esos PNG.

Los intentos anteriores se conservan: el primero tenía un mando de autodrive incorrecto en el arnés, y el segundo pasó las comprobaciones de estado pero quedó tapado por el menú de pausa. El tercer intento completó tres capturas; su cuarta ejecución se interrumpió antes del PNG y conserva los códigos desconocidos de su recibo original. El reintento final sí produjo la imagen y terminó normalmente. No se publican esos intentos como aprobados ni se sustituyen sus archivos.

Los bins de cuadro registrados fueron de 25 ms y el HUD mostró alrededor de 30,8 FPS, con picos RSS de 527–563 MiB. Son contadores de la aplicación completa y de bases distintas: no se convierten en coste marginal, tiempos aislados de GPU ni estabilidad de memoria. No se midió un baseline con la mesa desactivada. El recibo conserva los valores originales para revisar esos límites.

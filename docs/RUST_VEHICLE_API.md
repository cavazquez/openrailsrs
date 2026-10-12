# Extender sistemas de vehículos en Rust

El simulador usa Rust para frenos, motores, alimentación y mandos. La API es experimental y está vinculada a la versión de openrailsrs: una extensión se compila con el simulador. El host C# continúa como opción para contenido original que lo solicita explícitamente; no es necesario para escribir un sistema Rust.

## Contratos

- **Datos del autor:** `openrailsrs-formats` interpreta ENG/WAG/Includes; `openrailsrs-train` construye parámetros inmutables. Las unidades internas son kg, m, m/s, N, W y segundos. Los circuitos identifican explícitamente bar/psi o vacío. Conservá la fuente y sus hashes; un parámetro ausente no equivale a un equipo equipado.
- **Estado:** el estado mutable de un sistema pertenece a `TrainSimState` y usa Serde. Agregá un campo con valor por defecto para los escenarios anteriores, un validador y una versión propia cuando el formato pueda cambiar. Conservá temporizadores, reservas finitas y mandos. No guardes punteros, entidades Bevy, dispositivos, nombres de carpetas ni recursos GPU en el estado físico.
- **Mandos:** utilizá `operate_car`, `operate_cab`, `set_direction`, `steam_command`, `toggle_diesel_engine`, `toggle_pantograph`, `toggle_circuit_breaker`, `set_independent_brake` y las operaciones de acople/abastecimiento. Estos métodos comprueban equipo, posición y enclavamientos. El controlador de baja tensión usa `PowerSupplyState::update`; los circuitos de freno conservan sus retardos y aislamiento. Un diagnóstico se limita a observar; una intervención física se integra en el sistema correspondiente antes del siguiente paso.
- **Tiempo:** `LiveDriveSession::step_realtime` recibe segundos, conserva su remanente y divide el avance en cuantos de hasta 0,05 s. Los sistemas consumen ese reloj ferroviario. Una pausa no produce pasos. Los filtros y retardos dependen de segundos; el render sólo interpola posiciones y presenta efectos. `PowerSupplyState::update` recibe tiempo absoluto, no FPS.
- **Persistencia:** `snapshot_with_scripts`, `validate_snapshot` y `restore_snapshot` conservan toda la sesión. Para un sistema nuevo, su configuración debe corresponder al contenido guardado y el estado debe validarse antes de modificar la sesión. El tráfico y las reservas se restauran juntos. Un error de versión, contenido o estado devuelve un error visible; no se rearma un freno ni se repone combustible para ocultarlo.
- **TCS y presentación:** `EtcsTcs::compute` produce el estado del DMI a partir de una sesión; por sí mismo no impone un freno. `BasicEtcsTcs` es el proveedor Rust habitual. El host C# dispone de su contrato acotado en [TCS_CSHARP_HOST.md](TCS_CSHARP_HOST.md). Una nueva intervención se conecta a la física, con pruebas de su liberación y prioridad frente al mando del jugador.

## Ejemplo reproducible

`crates/openrailsrs-sim/examples/rust_vehicle_extension.rs` implementa un diagnóstico con versión, tiempos de baja tensión y fuga de manguera. Observa la batería y los circuitos reales, guarda su estado junto con la sesión, continúa desde el mismo reloj y rechaza versiones incompatibles. Utiliza la formación propia del ejemplo `traction_operation`; no precisa .NET, dispositivo de sonido o GPU.

```bash
cargo run --locked -p openrailsrs-sim --example rust_vehicle_extension
```

El programa comprueba que la batería apagada produce dos segundos de baja tensión a través de un guardado/restauración, que vuelve la alimentación al conectar batería y que una versión de diagnóstico desconocida se rechaza. El resultado final es JSON. Esta extensión de ejemplo observa la sesión desde un cliente Rust; no se carga como complemento binario desde el menú.

## Prueba manual de los contratos

Nueva partida → `traction_operation` → formación diésel → Jugar. En F9 desconectá batería con regulador cerrado: el HUD pierde baja tensión y la tracción no debe aparecer por tener el motor dibujado. Guardá, volvé al menú y continuá: la batería sigue apagada. Conectala y esperá su retardo real.

Elegí el Laboratorio de cabina y carteles, que tiene dos vehículos. En F9, con el tren detenido, desconectá la manguera del segundo coche con llaves abiertas. La tubería cae; al aislarla con las llaves cesa la fuga. Guardar conserva la conexión y el estado de los circuitos. En B del ejemplo de vapor se comprueban corte, fuego e inyectores con reservas reales. Estas acciones ejercitan las mismas funciones que usa el ejemplo Rust.

## Verificación de una contribución

Comprobá un ciclo de mando, retardo, agotamiento o interrupción, liberación y recuperación; repetilo desde un snapshot y con diferentes frecuencias del render. Compará contra los oráculos fijados en Open Rails 1.6.1 para las funciones equivalentes. Para equipos o efectos propios, identificá la extensión y sus ensayos; no cambies la tolerancia de un oráculo para aprobar una diferencia.

Los puntos de integración están en `live_drive.rs`, `physics.rs`, `brake.rs`, `power_supply.rs`, `electric.rs`, `diesel_operation.rs` y `steam.rs`. Los ejemplos originales permanecen intactos; los perfiles del jugador se guardan aparte.

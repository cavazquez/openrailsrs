# Frenos y alimentación nativos

Los trenes habituales funcionan con Rust. El host .NET de TCS sigue siendo opcional; no hace falta instalarlo para estos frenos ni para la alimentación. Los scripts C# arbitrarios de freno o alimentación de otros autores requieren un port específico y no se ejecutan con el host TCS actual.

## Contenido cubierto

- Bristol/Birmingham Pullman: mismos ocho vehículos, EP avanzado en cabeza y seis coches, con vehículo neumático al final. Conserva sus oráculos físicos anteriores.
- Class 47 y seis Mk2 de Demo Model 1: freno neumático con distribuidor y relé; conserva su comparación original de 250 segundos.
- Class 121: cilindros y depósitos de vacío; el original 1.6.1 también usa VacuumSinglePipe para su declaración de dos tuberías.
- Hall y King con sus ténderes: parámetros de vacío específicos de los archivos OpenRails, incluyendo geometría, volúmenes y unidades inHg.
- Material 1960 y R Stock: EP antiguo sin diámetro de cilindro. Mantiene el depósito auxiliar y la actualización eléctrica de presión antes de calcular la fuerza del cuadro siguiente.

Se selecciona `OpenRails/<nombre>.eng|.wag` cuando existe, respetando mayúsculas de Windows. Sus Include se resuelven desde ese archivo; formas, texturas, sonidos y cabinas siguen en el directorio original del vehículo. La expansión es acotada y rechaza enlaces fuera de la instalación. La última declaración de un parámetro de freno prevalece.

El vacío almacena presión absoluta de tubería, cilindro y depósito. Admitir aire aplica el freno; evacuarlo lo libera. Una manguera abierta destruye el vacío y una llave cerrada aísla el cilindro. El mando llega después de la propagación por la tubería. Un vehículo air_piped o vacuum_piped transporta el mando pero no genera un freno de servicio propio; conserva el freno de mano.

## Alimentación

Cada motor tiene alimentación principal, auxiliar, batería, cabina y salida para pasajeros independientes. El motor diésel detenido o sin combustible no entrega tracción. El eléctrico pierde su fuente cuando pierde contacto o tensión y espera los retardos al reconectar. Las auxiliares diésel pueden seguir funcionando con el contactor de tracción abierto. En vapor, la alimentación mecánica no depende de la batería; sus luces e instrumentos eléctricos sí.

Se leen ORTSPowerOnDelay, ORTSAuxPowerOnDelay y ORTSTractionCutOffRelayClosingDelay. Un reinicio interrumpido vuelve a contar la demora; guardar conserva los relojes. Las partidas anteriores sin este subsistema inicializan una alimentación preparada, igual que el arranque habitual.

F9 muestra principal, auxiliar, cabina y pasajeros por motor. Si el autor declara un interruptor ETS, aparece **Alimentación de pasajeros**; no aparece para un vehículo sin ese equipo. La salida admite el mínimo RPM declarado. No se afirma que este indicador simule calefacción y refrigeración de todos los coches.

Los faros dependen de la batería y la luz de cabina requiere alimentación de cabina. V conserva la orden del limpiaparabrisas, pero movimiento, barrido y sonido requieren batería. Al reconectarla vuelven a responder a sus interruptores. El HUD de vacío muestra inHg y las agujas CVF convierten bar, PSI, kPa o inHg según su archivo.

## Referencia reproducible

`tools/openrails-reference/BrakePowerReference.cs` ejecuta las clases de las DLL originales 1.6.1, verificadas contra `oracles/openrails-reference.toml`. Aporta únicamente entradas y lee estados; no sustituye sus ecuaciones. Los archivos de `examples/brake_supply_native` contienen parámetros y huellas de procedencia, sin imágenes, formas, SMS/WAV ni scripts originales. Las descargas permanecen fuera de Git.

- Cinco perfiles de vacío, 320 puntos: presión absoluta de tubería/cilindro/depósito y fuerza de zapata; tolerancia 0,0005 PSI y 5 N. Incluye servicio, aplicación plena, liberación y purga del depósito.
- Seis perfiles EP antiguos, 384 puntos: cilindro, demanda automática, depósito y fuerza; tolerancia 0,001 PSI y 5 N.
- Alimentación diésel, eléctrica y vapor, 303 estados: igualdad exacta de principal, auxiliar, baja tensión y cabina, con interrupciones de batería, llave, fuente y contacto.

Dos capturas independientes fueron idénticas byte a byte. Los binarios originales y sus referencias anteriores no se modificaron. La procedencia y las entradas están fijadas por SHA-256; check.sh verifica esa integridad.

```bash
python3 scripts/prepare_brake_supply_fixtures.py --content-root '/ruta/Content' --out-dir tmp/brake-supply-new
python3 scripts/capture_brake_power_reference.py --profiles tmp/brake-supply-new/profiles.json --out-dir tmp/brake-power-reference-new
python3 scripts/run_oracles.py --verify-only --source-root ../openrails
./check.sh
```

La captura requiere Wine y la instalación original fijada; comparar las referencias guardadas no requiere Wine ni .NET. Estos son ensayos de subsistemas con entradas prescritas: no certifican recorridos completos de Hall/King/metro, todas las válvulas de emergencia, compresores ni scripts de terceros. El modelo térmico de vapor mantiene el alcance descrito en TRACTION_SUPPORT.md.

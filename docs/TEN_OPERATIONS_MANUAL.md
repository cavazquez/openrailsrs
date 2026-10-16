# Cómo probar las operaciones, el sonido y el escenario

Tras compilar con `./check.sh`, iniciar `target/debug/openrailsrs-viewer3d`
desde la raíz del proyecto para abrir el menú de esta versión local.

Elegir los ejemplos desde **Nueva partida**, con país **Todos** si un filtro
los oculta. Los ejercicios propios no requieren descargar rutas. Los casos
MidEast, BNSF, SCE y Chiltern usan la instalación original indicada por cada guía.

1. **Rodeo de MidEast — #211.** Elegir **Firsby to Spilsby Return**, con
   **84030 y dos Mk1**. Hacer las seis paradas y las maniobras de Spilsby. Los
   coches deben permanecer en su vía mientras la locomotora los rodea, y el
   reacople debe permitir la vuelta a Firsby. Seguir las instrucciones de
   [rodeo original](MIDEAST_RUNAROUND.md), incluidos los marcadores y el guardado.
2. **Dos secciones propias — #215.** Abrir **Porciones: dos cortes con
   identidad**. En F9 asegurar y separar dos colas, guardar/reanudar y recogerlas.
   Deben conservarse ambas identidades, sus vías y sus frenos; una sección
   distante no se recoge pulsando su nombre. [Pasos](PORTIONS_AND_SERVICE_TRANSFERS.md).
3. **Porciones de servicios — #216.** Abrir **Porciones de dos servicios:
   unir y devolver**. Recoger **Porción-Sur**, guardar, devolverla y volver a
   tomarla. Deben existir siempre los mismos seis coches, sin duplicados ni
   cambios de posición; el servicio devuelto conserva su horario y equipos.
   [Pasos y comandos admitidos](PORTIONS_AND_SERVICE_TRANSFERS.md).
4. **Actores y objetivos ACT — #213.** En BNSF Scenic elegir **4. Everett
   Switching**. Revisar en M las seis formaciones estáticas y en F7 las
   órdenes del autor. Guardar antes de maniobrar y reanudar debe conservar
   posición, UID y ocupación. F9 muestra los UID al recoger los coches.
   Llegar al final del PAT no cumple órdenes pendientes.
   [Alcance y prueba](NATIVE_ACTIVITY_OPERATIONS.md#condiciones-act-y-persistencia).
5. **Instrucciones PAT — #214.** Elegir **PAT: corte 40102 y código no
   admitido**. El primer marcador debe cortar y asegurar los vagones una sola
   vez. Reanudar no repite el corte. El código no admitido debe quedar pendiente
   con explicación, sin convertirse en una espera corriente ni aprobarse.
   [Pasos](NATIVE_ACTIVITY_OPERATIONS.md#puntos-de-espera-pat).
6. **Mercancías — #212, aceptación original pendiente.** Elegir **Actividad:
   carga, composición y guardado**. B/F9 permiten cargar el vagón alineado hasta
   2,4 t, cancelar, descargar y guardar durante la transferencia. Cantidad y
   masa deben cambiar juntas. El tutorial 6 de Zig Zag original tiene un archivo
   de frenos ausente en el paquete del autor; esa aceptación permanece abierta.
   [Ejercicio, cantidades originales y faltante](NATIVE_ACTIVITY_OPERATIONS.md).
7. **Señales enlazadas — #219.** En la bifurcación de MidEast comparar los
   cabezales con F4. La rama ajena puede mostrar rojo mientras la seleccionada
   permite pasar. Una sección o reserva en el bloque protegido debe impedir el
   permiso de todo el poste. Guardar no libera la ocupación.
   [Configuración y evidencia](OPERATIONAL_SIGNALS.md).
8. **Sonido según vía — #238.** En Demo Model 1/SCE, conducir la **Class 47
   con seis Mk2**, con audio y grupo de rodadura activos en F10. Al atravesar
   regiones de vía, el sonido debe cambiar conforme pasa cada coche. Probar
   ambos sentidos, cabina, pasajeros, exterior y pausa. Reagrupar coches debe
   conservar sus sonidos originales.
   [Trayecto y diagnóstico](TRACK_SOUND_REGIONS.md).
9. **Mecánica de bogies — #326.** Elegir **Laboratorio de mecánica lateral
   y bogie compartido**. Conducir la curva y revisar los apoyos, el peralte y
   la curvatura indicada en F8. Las fuerzas y el estado de
   suspensión se conservan al guardar y se verifican en los ensayos numéricos;
   F8 todavía no muestra esos valores mecánicos. El límite visual del modelo
   y la respuesta mecánica son independientes.
   [Ejercicio y límites](BOGIE_DYNAMICS.md).
10. **Estaciones y sombras — #239.** En **Chiltern extendido**, verano y
    despejado, alternar 1/2 en las seis estaciones y mover la cámara con P activo.
    Edificios y vegetación deben mantener sus posiciones y transparencias;
    las sombras próximas deben permanecer tras la carga y los cambios de vista.
    [Doce capturas comparables y procedimiento](STATION_VISUAL_COMPARISON.md).

Las comparaciones nativas conservan Open Rails 1.6.1 y sus tolerancias. La
prueba de una función o actividad concreta no certifica todas las rutas,
locomotoras ni sistemas del simulador original.

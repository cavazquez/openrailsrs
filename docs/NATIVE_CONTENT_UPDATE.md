# Contenido nativo, biblioteca y validación de la alpha.4

El contenido original permanece en los datos del usuario, fuera de Git y del
Snap. La referencia sigue siendo Open Rails 1.6.1. Los informes distinguen
recursos completos, circulación, final del itinerario y paridad con oráculos.

## Probar el selector

1. Abrí Nueva partida, elegí Chiltern v4 y un servicio original. En **Imagen /
   Mapa** deben aparecer su recorrido y paradas. La leyenda separa catenaria,
   riel eléctrico y sectores sin tensión; la pendiente corresponde al sentido
   del viaje. Un mapa sin esos datos no certifica compatibilidad eléctrica.
2. En Tren, buscá **Railcarx2**. Deben verse dos coches originales. ↶/↷ gira la
   cámara, +/− cambia distancia, **Vehículo** enfoca un coche y **Todo** recupera
   el conjunto. Los coches invertidos conservan la orientación de su CON.
3. Abrí **Filtros**. País filtra procedencia, tracción y trocha usan datos
   declarados por los vehículos, Recursos separa completos/incompletos/pendientes.
   **Favorito** guarda la selección; **Sólo favoritos** debe recuperarla después
   de cerrar y abrir el visor. **Quitar filtros** elimina los filtros activos.
4. Seleccioná una formación gris: deben cambiar el resumen y el diagnóstico.
   Los recursos obligatorios ausentes impiden Jugar; los avisos opcionales y de
   compatibilidad permiten seleccionarla y revisar dónde colocar los archivos.
5. A 640 × 480 debe quedar una opción de tren completa visible al entrar. La
   rueda desplaza el cuerpo; el pie de lanzamiento permanece dentro de la ventana.

## Actividades e inversiones

ACT, SRV y PAT conservan las referencias originales. Un punto intermedio del
PAT no debe desviar el recorrido hacia una vía vecina; sólo `IsJunction` crea
un nodo de cambio, contrastado con las DLL originales. Los nodos PAT con inversión
o espera generan tramos ordenados. Las esperas absolutas 3HHMM usan el reloj de
la actividad, incluido el cruce de medianoche. La duración declarada es una
estimación; terminar más tarde no demuestra por sí solo que el archivo sea inválido.
En Brooklyn–Hempstead el límite inicial debe ser **15 mph / 24,14 km/h**, no
los 500 mph declarados como máximo del vehículo. Los carteles se aplican al
sentido nativo correcto y los aumentos esperan que la cola los deje atrás.

En LIRR, seleccioná **04CentralPickups** con **1xNECX102**. Al aproximarse a una
inversión el monitor debe pedir detenerse y cambiar el inversor; no debe saltar
ningún coche. La cola debe despejar el cambio de vía antes de iniciar el tramo
siguiente. Guardá antes de invertir y reanudá: deben mantenerse posiciones,
combustible, frenos y odómetro. El piloto automático se usa para verificar el
itinerario; no interpreta todos los eventos de carga o composición del ACT.

Los tutoriales antiguos de Zig Zag tienen comillas y bloques incompletos que el
lector original recupera. El importador reproduce esa lectura acotada sin editar
los archivos del autor. Los Includes absolutos se reinterpretan desde TRAINSET
únicamente cuando existe ese sufijo dentro del paquete; no se buscan archivos en
el directorio arbitrario de Windows. Rutas UNC, escapes y ciclos se rechazan.

## Horarios originales y operaciones F9

Los `.timetable_or` se descubren en ACTIVITIES/OpenRails y aparecen como servicios.
También se pueden preparar con:

```bash
target/debug/openrailsrs import-timetable /ruta/ROUTES/SCE \
  /ruta/ROUTES/SCE/ACTIVITIES/OpenRails/SCE_total.timetable_or \
  --service T_0730_SHOTTS --out-dir tmp/horario-sce
target/debug/openrailsrs play-service tmp/horario-sce/scenario.toml \
  --out-dir tmp/horario-sce/prueba
```

La formación `tt_MT SCE BlGr # Set 101 364` debe recorrer Haymarket → Edinburgh,
esperar la salida del sucesor T_0800_EDBS, invertir y volver a Haymarket. `$forms`
reutiliza el tren y sus sistemas; no genera una copia en el punto de salida.

El alcance de división/unión es **una sección propia estática**. Se admite
`$detach/units=-N/static=nombre` (o el corte equivalente en un tramo invertido),
con `/manual` o `/auto`, y `$pickup=nombre`. En manual, F9 debe asegurar la cola
con frenos de mano antes de desacoplar. Para recogerla hay que llegar a su
enganche, acoplar, conectar mangueras/llaves y soltar frenos de mano. La sección
estacionada sigue ocupando vía aunque el jugador cambie de sentido. Guardar
conserva el tramo, las operaciones completadas y la sección independiente.

En horarios se rechazan con diagnóstico comandos desconocidos, corte por el
extremo no admitido, sección ajena, porción móvil y pools. Las operaciones
codificadas en puntos de espera PAT todavía no se interpretan: pueden quedar
como esperas y esos ACT están fuera del alcance comprobado. Las maniobras de
rodear coches por otra vía todavía requieren intervención manual.

## Señales e instrumentos

MidEast Coast, Portugal 79 y Coals to Newcastle conservan sus SIGSCR, dirección
TDB, estados gráficos y aspectos nativos. MR/LR resuelven cabezas y grupos del
recorrido, las máscaras filtran aspectos básicos y la señal de aguja utiliza el
primer cambio enfrentado. La autorización de maniobra requiere concesión,
dirección, reserva y condiciones de plataforma; una ocupación incompatible
sigue bloqueando la circulación. Estos contextos se contrastan en pruebas de
sesión y en 419 casos de funciones y 256 máscaras capturados de OR.
En una ruta sin mallas originales de vía, el reemplazo procedural conserva
el sentido, los offsets y las curvas del WORLD/TDB. Coals permite comprobar
este caso: el tren debe circular encima de los rieles generados, no a su lado.
El reemplazo no reproduce texturas de mallas que el paquete no incluye.

El amperímetro usa esfuerzo motor y velocidad de ejes. Acelerá, soltá regulador y
frená: corriente y carga deben responder, sin una oscilación gráfica inventada.
Con patinaje, esfuerzo motor y fuerza efectiva sobre el carril pueden diferir.
La escala sigue siendo la del CVF del autor. La cabina usa el archivo ENG
original del CON, aunque tenga otro nombre que su modelo 3D; si no hay interior
3D pero sí un CVF 2D válido, se abre esa cabina. AMMETER, AMMETER_ABS y LOAD_METER
tienen 48 resultados de las DLL originales; no certifican cada motor eléctrico
ni todas las cabinas de la comunidad.

## Repetir rendimiento y regresiones

```bash
./check.sh
python3 scripts/benchmark_service_journey.py \
  --scenario examples/chiltern_extended/scenario.toml \
  --route-root /ruta/Chiltern --formation-cars 8 \
  --out-dir tmp/viaje-clima
```

La prueba requiere el visor con todas las features, GPU y Weston. Recorre las seis
estaciones con cielo despejado, lluvia intensa y nieve intensa, con directorio de
jugador y compositor privados. Mide RSS, VRAM del proceso y percentiles de cuadros
del viaje entero; separa carga inicial y ventana detallada de hasta 12000 cuadros.
`--speed-mul 16` sirve como estrés acelerado del streaming y la física, y queda
etiquetado como tal. No permite concluir FPS de conducción a velocidad normal.
`--particle-budget 512/2048/8192` permite comparar presupuestos usando el mismo
binario, recorrido, semilla y cámara. El reporte rechaza shaders fallidos,
sectores cercanos incompletos, errores SIGSCR y paradas fuera de ±10 m/0,1 m/s.

Las descargas, rutas absolutas del usuario, ajustes, partidas y trazas quedan
fuera del paquete publicado. La alpha del canal edge sigue siendo de desarrollo.

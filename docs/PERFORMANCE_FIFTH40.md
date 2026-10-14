# Rendimiento y pruebas prolongadas

Los cambios de esta tanda conservan las formas, texturas, SIGSCR y sonidos del autor. La caché guarda derivados bajo la carpeta de datos del jugador; no modifica ni incorpora las descargas al repositorio. Open Rails 1.6.1 sigue siendo la referencia de compatibilidad. Las pruebas de este documento miden nuestro motor; no son una certificación de paridad visual o acústica.

## Preparación, carga y residencia

- Las mallas de ajuste que nunca se dibujan usan ocho esquinas cuando existen partes originales. Se evita publicar una segunda copia completa de todos sus vértices.
- La preparación de formas, sus niveles de detalle, los radios seguros de cada banda y la resolución de dependencias de texturas siguen en trabajadores. La publicación recibe también esos límites: mirar un objeto por primera vez no vuelve a recorrer todos sus vértices en el hilo del cuadro. La publicación comparte un límite de trabajo de cuatro milisegundos por cuadro y un presupuesto de transferencias de ocho MiB, reducido a dos MiB bajo presión. La forma principal y cada banda de detalle se publican en turnos separados; los bosques, transferencias y otras piezas auxiliares que entran durante el viaje se envían de a una por cuadro. Un objeto individual puede superar el límite de tiempo; el informe lo identifica por nombre.
- El presupuesto admite al menos una entrega pendiente por cuadro. Esto impide que un límite menor que el coste fijo del sistema deje la carga detenida indefinidamente. Una regresión reproduce el caso con presupuesto de tiempo cero y comprueba que la cola instanciada se vacía.
- El pronóstico toma cabecera, cola y el punto del itinerario que se alcanzaría en treinta segundos, hasta dos kilómetros por delante. La cámara mantiene su área inmediata. Los sectores adicionales tienen un límite de dieciocho, reducido a seis bajo presión de memoria.
- La prioridad combina distancia, tiempo de llegada y dirección de cámara. Cada cuarto lote de formas conserva un turno ordenado para impedir que los objetos detrás del jugador queden postergados indefinidamente.
- La caché de geometría incluye fuente `.s`, descriptor `.sd`, versión y código de preparación. La importación nativa incluye las dependencias realmente leídas, los nombres de los directorios y el código de parser, grafo y SIGSCR. Un cambio, archivo dañado o edición durante la preparación obliga a reconstruir. Cada caché está limitada a 256 MiB en disco; la importación admite entradas de hasta 64 MiB y no contiene identificadores GPU.
- La residencia usa histéresis: entra en presión al 90 % de la memoria del dispositivo activo y sale por debajo del 78 %. Se elige la cuenta DRM con mayor residencia propia en VRAM más GTT: una GPU abierta solamente durante la enumeración no aporta su capacidad al umbral de otra GPU. La telemetría conserva las cuentas por PCI y distingue memoria del proceso, GTT y uso del dispositivo; una lectura no disponible no se interpreta como cero. Se retiran únicamente objetos y recursos sin referencias vivas, en lotes limitados.
- Luces, vegetación, sombras, partículas y transferencias comparten presupuesto. Los faros del jugador tienen prioridad sobre luces de pasajeros y estaciones. Los ajustes de F10 permiten fijar calidad y límites; la reducción automática afecta a la presentación.

## Bancos de audio

Los programas SMS se cargan en el trabajador de audio. Sus WAV se decodifican cuando un evento los necesita, con PCM compartido entre vehículos. El depósito procura conservar hasta 64 entradas de caché y admite 128 MiB de PCM único; libera primero archivos sin bucles activos. Si todas las entradas están sostenidas por bucles vivos, preserva esos bucles y puede superar el objetivo de 64 entradas sin superar el presupuesto de PCM. Este presupuesto no incluye metadatos de programas ni sobrecarga del asignador. Las colas de estado, clima y truenos tienen capacidad finita. La cámara cambia la mezcla sin volver a cargar el banco.

El informe contiene lecturas de SMS por programa y vehículo, tiempo de apertura y decodificación de WAV, aciertos, fallos, expulsiones y PCM residente. Las cifras por banco pueden referirse al mismo WAV compartido: no deben sumarse. `decoded_mib` representa la asignación única del depósito. La apertura de archivo se mide separada de la decodificación; las lecturas que el decodificador hace mientras procesa están incluidas en su tiempo.

## Repetir las mediciones

Compilar con las mismas opciones para referencia y candidato. Guardar los dos ejecutables fuera de `target` antes de recompilar. Las herramientas necesitan el visor con `dev-tools` o `--all-features`, Weston y una GPU Vulkan; utilizan una pantalla privada. Los bancos de audio se ejercitan mediante una salida ALSA nula, sin abrir los parlantes.

```sh
python3 scripts/check_fifth40_performance.py \
  --baseline /ruta/al/visor-anterior \
  --baseline-resources /ruta/a/recursos-de-esa-revision \
  --candidate /ruta/al/visor-nuevo \
  --route-root /ruta/al/ROUTES/Chiltern \
  --out-dir tmp/performance-comparison

python3 scripts/check_native_cache_performance.py \
  --cli target/debug/openrailsrs \
  --route-root /ruta/al/ROUTES/Chiltern \
  --out-dir tmp/native-cache-comparison

python3 scripts/check_viewer_residency.py \
  --viewer /ruta/al/visor-nuevo \
  --route-root /ruta/al/ROUTES/Chiltern \
  --budget-mib 1024 --out-dir tmp/residency-comparison

cargo run --locked -p openrailsrs-sim --example traffic_benchmark -- 32
```

La primera herramienta compara carga y conducción con la misma cabina, formación, cámara, semilla, clima y resolución. Ambos visores deben usar sus recursos y shaders correspondientes. Guarda P99, máximos, RAM, memoria gráfica y atribución de trabajo a lectura/preparación/publicación/liberación. El ensayo de caché exige que el grafo y los programas SIGSCR importados sean idénticos byte a byte en modo fresco, frío y caliente. El ejemplo de tráfico compara uno, dos, cuatro y ocho trabajadores, separa carga de conducción y exige el mismo hash de estado final; las reservas y la aplicación de órdenes conservan un orden determinista.

En la comparación gráfica, «fría» significa una carpeta nueva de caché de la aplicación; «caliente» reutiliza esa carpeta. No se vacían las cachés de archivos del sistema operativo ni la de shaders de Mesa. En equipos con dos GPU hay que fijar tanto Vulkan como el compositor privado: `MESA_VK_DEVICE_SELECT` selecciona el visor y `DRI_PRIME` selecciona GL. El recibo conserva el nombre efectivo de ambos renderizadores, además de las otras cargas del equipo.

La prueba de residencia utiliza contadores DRM reales. `OPENRAILSRS_VRAM_BUDGET_MIB=1024` cambia el umbral del proceso, conservando la capacidad física del dispositivo. El umbral activa una política de liberación y menor trabajo cosmético; los recursos imprescindibles pueden superar ese presupuesto. El ensayo exige liberaciones reales y una vecindad visible completa tras ocho kilómetros. No llena la GPU deliberadamente ni reproduce un agotamiento del controlador.

Para diagnóstico se puede desactivar una caché con `OPENRAILSRS_SOURCE_CACHE=off` o `OPENRAILSRS_GEOMETRY_CACHE=off`. `OPENRAILSRS_PROFILE_SYSTEMS=1` habilita histogramas acotados por sistema para distinguir CPU de simulación, sectores y clima. No es necesario borrar el contenido original. El informe conserva el hardware y los hashes utilizados; los porcentajes globales de GPU también pueden incluir compositor y otros procesos.

En equipos con poco espacio, `--release-case-cache` retira únicamente la caché privada de cada comparación cuando sus dos ejecuciones fría/caliente ya terminaron. Conserva resultados, capturas y un manifiesto de los bytes retirados. Los datos de partidas y las rutas instaladas no están dentro de esas carpetas nuevas de ensayo. Los logs del visor toleran un archivo de salida lleno o una tubería cerrada: un fallo de diagnóstico no debe interrumpir la conducción.

## Recorrido prolongado

```sh
python3 scripts/check_viewer_soak.py \
  --viewer /ruta/al/visor-guardado \
  --route-root /ruta/al/ROUTES/Chiltern \
  --scenario examples/chiltern_extended/scenario.toml \
  --routes-json /ruta/a/rutas-de-prueba.json \
  --audio-null --framepace 30 --seconds 7200 --out-dir tmp/endurance
```

El JSON contiene una lista de objetos con `path` absoluto al escenario y `route_root` absoluto a la ruta instalada. Cada tres minutos se cambia de recorrido; cada minuto cambia cámara y clima y se serializa/restaura la simulación. El reloj del ensayo usa tiempo real, aunque la conducción simulada avance a cuatro veces la velocidad. Al cumplirse la duración, se detiene el desplazamiento y se espera a completar recursos y shaders antes de capturar.

Cada treinta segundos se guardan entidades **activas** —separadas de la capacidad de índices de Bevy—, mallas, imágenes, fuentes, atlas de letras, RAM, VRAM, audio y trabajos pendientes. El calendario usa intervalos absolutos para evitar que cada carga retrase todos los cambios siguientes; un intervalo omitido no aumenta los contadores de operaciones completadas. Comparar el mismo recorrido y fase de carga entre ciclos: el número de objetos de una estación grande difiere del laboratorio pequeño. Una prueba con `--smoke` valida el mecanismo y se identifica como breve; nunca cuenta como ensayo de varias horas.

Al cambiar de ruta se retiran explícitamente las entidades y los recursos del escenario anterior. Los recursos internos de Bevy, el cuadrilátero compartido de Hanabi, las tablas de tono y los atlas de letras se conservan. El HUD reutiliza la fuente predeterminada: crear otra copia idéntica en cada entrada retenía siete imágenes nuevas de letras por transición. El ensayo prolongado detectó esa acumulación; el test de cinco recreaciones exige una sola fuente y comprueba los componentes de texto reales.

Reservar una GPU para la prueba prolongada. En este equipo, varios ensayos nativos concurrentes produjeron `DeviceLost` en RADV; esas ejecuciones se conservaron como fallidas. El límite de presentación se pasa explícitamente al visor. Una prueba interrumpida, incluso después de más de dos horas, no se presenta como la duración solicitada completada.

## Mediciones verificadas

Chiltern v4, AMD Ryzen 5 9600X y Linux/Mesa 26.0.8. Los recibos guardan ejecutables, hashes y opciones; son mediciones locales de una configuración concreta.

- Importación nativa del grafo y SIGSCR con el CLI actualizado: 10,14 s fresca, 9,92 s fría, 0,111 s caliente y 0,114 s al repetir. Las cuatro salidas contienen los mismos 42.629.796 bytes y el mismo SHA-256; los aciertos calientes no reescriben la entrada. El tamaño anterior de 32 MiB impedía cachear este grafo.
- Audio en el punto de 2,5 km, con 48 programas SMS, 174 streams, 69 muestras referenciadas y 38 voces activas: el PCM único pasó de 10,447 a 4,939 MiB. Se decodificaron 38 muestras necesarias y se observaron 4.726 reutilizaciones en la ejecución caliente. Esta prueba utiliza ALSA nulo y no sustituye una escucha manual.
- Tráfico propio de 33 servicios durante 60 segundos simulados: los trabajadores 1/2/4/8 obtuvieron el mismo SHA-256 del estado final. Los tiempos de conducción fueron 1.259/1.295/1.317/1.334 ms; en esta carga pequeña el trabajo paralelo no mejoró la velocidad. La carga inicial se midió por separado: 33,2/33,0/33,8/32,5 ms. En el corredor de repositorio Chiltern, con dos servicios AI y el jugador, también se obtuvo el mismo estado: 116/133/138/161 ms de conducción y 21,1/18,5/17,1/25,9 ms de carga. El modo de un trabajador sirve como referencia serial del mismo binario.
- Residencia real durante ocho kilómetros en RX 7600, con cuentas por PCI: 8.176 MiB de capacidad del dispositivo activo. La política física completó 8.001,9 m, mantuvo 9 sectores GPU y retiró 73 mallas; el umbral de proceso de 1.024 MiB completó 8.000,3 m, mantuvo 7 sectores y retiró 99 mallas. Ambas terminaron con cero objetos próximos y transferencias pendientes. La VRAM propia fue 2.303,3 frente a 2.301,6 MiB; GTT de 84,2 MiB en ambas. Bajo presión, las transferencias bajan de ocho a dos MiB, la vegetación a la mitad y las luces de estación de veinticuatro a cuatro; los ocho faros del jugador se conservan. El umbral no es un límite duro de un GiB. La primera ejecución prepara la caché que reutiliza la segunda: estas cifras acreditan residencia y liberación, no una ganancia causal de FPS. El recibo anterior sumaba 8.688 MiB entre las GPU abiertas; la selección posterior usa la GPU realmente residente.
- La comparación gráfica inicial identifica regresiones además de mejoras. Con lluvia fría, el P99 de conducción pasó de 106 a 95 ms y el máximo de 107 a 98 ms. En despejado caliente, el P99 pasó de 55 a 95 ms. Esta comparación requiere continuar el diagnóstico de CPU; no acredita una mejora general de fluidez.
- El perfil posterior separó los picos: el registro de pasos a nivel consumía una mediana de 15,2 ms por cuadro por resolver repetidamente enlaces ausentes. Se conserva su definición y sus puntos viales mientras están cargados, con invalidación al cambiar el grafo y poda al descargar sectores. La ocupación y el movimiento de barreras siguen actualizándose. En los dos diagnósticos se observaron P99 de 43,9 y 9,3 ms en el sistema de publicación del mundo; tenían distinto estado de caché.
- La comparación de las revisiones internas 15 y 17 de esta tanda fija tanto el compositor GL como el visor Vulkan a RX 7600, y conserva formación, cámara, semilla, distancia de 2,5 km y opciones. P99 de conducción: despejado frío 89→65 ms, despejado caliente 81→56 ms, lluvia intensa fría 112→76 ms y caliente 140→79 ms. Sus máximos bajaron de 139,7/135,7/182,8/276,1 a 103,3/111,8/101,0/160,7 ms. No se retiró trabajo visual para la medición. Los picos iniciales se registran por separado: 2.029/1.901/1.924/2.227→2.559/1.946/1.926/2.264 ms. La mejora de conducción no resuelve esos picos iniciales, ni garantiza fluidez en todos los equipos. Los recibos anotan el ensayo prolongado en la integrada y las cargas CPU de compilación, simulación y prueba de Snap que coexistieron.

- La carga simultánea de CPU afecta las comparaciones anteriores. Después de terminar la simulación de veinte trenes y las pruebas del Snap por software, el mismo ejecutable 17 pasó de P99 de 76/79 ms a 27/26 ms en lluvia fría/caliente. No se atribuye esa diferencia a un cambio de código. La comparación posterior 17→19, con esa carga retirada y ambos renderizadores en RX 7600, dio P99 de 27→26 ms en frío y 26→26 ms en caliente; máximos de 51,5→38,1 y 42,0→46,4 ms. Los radios LOD preparados por el trabajador eliminan cálculos repetidos; esta medición no demuestra una mejora general de FPS.

- La comparación quieta del commit de referencia `d9dba7af` con el ejecutable 22, con lluvia intensa, fija RX 7600 para Vulkan y GL y comparte únicamente la carga del ensayo prolongado en la integrada. En frío y caliente, el P99 de conducción quedó en 26→26 ms; máximos de 47,09→40,09 y 51,72→43,61 ms. Los máximos iniciales fueron 1.594→1.720 ms en frío y 1.606→1.561 ms en caliente. Se redujeron máximos de conducción, pero no se acredita una mejora de P99 ni de la carga fría.

- La comprobación final `d9dba7af`→27 repitió lluvia intensa fría/caliente con cuatro lanzamientos, sin compilaciones ni simulaciones adicionales, y sólo el ensayo prolongado en la integrada como fondo. P99 de conducción 40→28 y 28→27 ms; máximos 97,90→68,56 y 44,00→44,41 ms. Los máximos iniciales fueron 1.609,88→1.694,78 y 1.549,67→1.808,62 ms: la carga inicial empeoró en ambos casos y el máximo caliente de conducción subió 0,41 ms. Las fases del escenario del ensayo de fondo no son idénticas entre lanzamientos; estos cuatro recibos no se presentan como prueba causal aislada de la poda ni como una mejora general de FPS. Las cuatro capturas nuevas terminaron con cero formas próximas, transferencias y shaders pendientes; la cabina y su entorno fueron inspeccionados. Los dos cachés privados se retiraron después de medir cada lado.

- La segunda comparación quieta recuperó el ejecutable exacto de la revisión interna 15 y lo midió frente a los recibos de la 22, con las mismas opciones y sólo el ensayo de la integrada como trabajo de fondo. Lluvia fría: P99 46→26 ms y máximo 87,41→40,09 ms; caliente: 45→26 ms y 96,61→43,61 ms. Es la reparación de tirones introducidos durante esta tanda; no se presenta como mejora global frente a `d9dba7af`, cuyo P99 sigue en 26 ms. Los máximos iniciales de 15→22 fueron 1.584→1.720 ms en frío y 1.623→1.561 ms en caliente. La carga fría continúa pendiente de mejora.

- La geometría nativa mantiene datos del autor y niveles de detalle: el candidato 22 registró cero aciertos y 474 fallos en la ejecución fría, frente a 469 aciertos y cinco fallos en caliente. Las dependencias, archivos corruptos y ediciones durante lectura se comprueban por separado. La caché sólo guarda preparación CPU; no retiene handles GPU.

- Los límites fijos de F10 se midieron en orden normal/reducido/normal, con lluvia 1,0, semilla 42, cámara, formación, punto de 2,5 km y física iguales. Partículas 2.048→512→2.048, luces de estación 24→0→24 y divisor de vegetación 1→4→1; los ocho faros tienen prioridad. Las tres vecindades terminaron completas. P99 26→27→26 ms y ocupación global de RX cercana al 22 %: estos ajustes responden correctamente, pero no aumentaron los FPS en esa escena. La compilación, simulación adicional y render del Snap estaban detenidos; el ensayo de la integrada continuaba y se registra como carga de fondo.

- La política de asignación se contrastó con cuatro ejecuciones quietas 22→23: mismo recorrido, recursos, calidad y simulación, cambiando sólo `wgpu::MemoryHints` de `Performance` a `MemoryUsage`. P99 quedó en 26 ms, en frío y caliente. Máximos de conducción 38,72→39,69 ms y 47,73→32,61 ms; máximos iniciales 1.634,48→1.652,28 y 1.588,94→1.593,68 ms. La pequeña regresión fría y la inicial se conservan; no se promete una mejora general de FPS.

- La prueba 16 completó 7.200,05 segundos reales, treinta y nueve cambios de ruta y ciento veinte serializaciones/restauraciones en memoria. Fuentes y atlas quedaron en 1/12, y las colas se vaciaron, pero la GTT del laboratorio pasó de 2.112 a 3.062 MiB: la estabilidad se rechazó. El control posterior 22 se interrumpió deliberadamente a 1.470 s y no cuenta como dos horas completadas. En tres visitas comparables, Vulkan tenía aproximadamente 505/490/511 MiB vivos frente a 2.112/2.368/2.368 MiB reservados; se identificaron bloques de 256 MiB anclados por sólo 0,214–6 MiB de buffers. Las tres texturas de partículas ahora conservan su identidad entre rutas. El candidato 23 usa bloques de dispositivo de 8–64 MiB, conservando calidad y recursos. El diagnóstico 23 también se interrumpió, a 1.260 s, después de confirmar una retención de Bevy: `BinUnpackingBindGroups` conserva referencias a buffers cuando sus vistas fuente ya se eliminaron. La poda usa la API pública y conserva únicamente las claves de fuentes vivas después de preparar los grupos. La aceptación final repetirá dos horas e incluirá claves obsoletas y retiradas; ni una reserva menor ni una muestra inicial favorable acreditan estabilidad prolongada.

El [recibo público de rendimiento](fixtures/compatibility/fifth40-20261013/performance.json) contiene hashes completos de ejecutables, escenarios, formaciones y salidas, métricas por caso y límites. Distingue las referencias globales de las internas, memoria viva de reservas y simulación de render. El recibo de estabilidad sólo se aprobará al terminar la duración real y comprobar los ciclos comparables. La prueba de poda exige fuentes y grupos activos, retiros acumulados positivos tras recargas y cero claves obsoletas después de retirar. En un renderer sin preprocesado GPU, mapas ausentes producen ceros: esos ceros no verifican la corrección.

## Comprobación manual

1. Con Chiltern instalado, iniciar un servicio desde cabina y conducir hasta cambiar de sector. Volver al exterior y mirar a ambos lados. Los objetos próximos deben aparecer sin esperar a volver a entrar en cabina; los edificios y detalles conservan el modelo del autor.
2. En F10, fijar calidad y probar lluvia y nieve intensas. Repetir con adaptación automática. Los faros siguen iluminando la vía y la simulación mantiene su reloj mientras la presentación reduce trabajo cosmético si hace falta.
3. Cambiar cabina/exterior/pasajeros varias veces con audio activo. Los bucles de motor y rodadura continúan; el cambio de mezcla no debe arrancar de nuevo cada WAV.
4. Cerrar y volver a abrir el mismo servicio. La carga caliente debe producir aciertos de caché; no debe alterar señales, materiales o posiciones. Un descriptor o recurso cambiado debe obligar a reconstruir.
5. Alternar rutas, guardar y reanudar durante un viaje largo. Comprobar que las métricas se estabilizan al repetir una ruta y que las colas se vacían al detenerse, sin interpretar el aumento normal de recursos al llegar a una estación como una fuga.

- El ensayo 27 conserva 239 muestras íntegras hasta 7.170,042 s, pero el wrapper terminó con señal143 sin informe ni captura final. No se redondea a dos horas; también queda rechazado para estabilidad: en visitas equivalentes al laboratorio (120 s, nieve intensa, cámara de conducción y mismo presupuesto), las texturas GPU sin etiqueta aumentaron de 40 a 52 y 64. Los mapas reales de materiales permanecieron en 31 entradas. Se prepara una sola imagen persistente de densidad de niebla con etiqueta propia, sin cambiar sus bytes, sampler ni efectos; el contador de esa etiqueta observa todas las asignaciones y no depende del recorte de las dieciséis mayores. La aceptación sigue pendiente de comprobar la corrección en GPU y completar otra prueba íntegra de dos horas.

- El diagnóstico 32 completó 420,011 s con salida independiente 0 y trece cambios observados. En las doce transiciones posteriores a la primera publicación GPU (60→420 s), la densidad compartida mantiene una asignación de 32.768 bytes; la muestra inicial sin etiqueta se conserva como no disponible. Los grupos retirados llegaron a 26, quedaron cero obsoletos y la captura nueva terminó lista. Esto verifica el pool de niebla, no explica por sí solo el crecimiento de todas las texturas anónimas. La nueva prueba independiente de 7.200 s comenzó el 14 de octubre a las 03:59:49 UTC en la integrada; su estabilidad sigue pendiente con criterios de comparación y tolerancias declarados antes de medir.

- La réplica final D9→32 completó cuatro capturas con los mismos datos y RX 7600, sin otras compilaciones ni simulaciones aparte del ensayo en la integrada. P99 frío/caliente de conducción 37→40 y 29→32 ms; máximos 65,40→78,16 y 50,65→55,39 ms. El máximo inicial frío mejora 1.707,11→1.655,11 ms, pero el caliente empeora 1.734,41→2.007,91 ms. Se conservan esas regresiones: no se afirma una mejora general del candidato final. Como las fases del ensayo de fondo varían entre lanzamientos, queda predeclarada otra comparación con orden ABBA y sin ese fondo después de terminar las dos horas.

- El CLI final 32 repitió el grafo nativo: 12,384 s fresco, 11,900 s frío, 0,146 s caliente y 0,128 s al repetir. Las cuatro salidas vuelven a contener 42.629.796 bytes y SHA-256 `b4cdbfbef1d845a597cb04136e4597a54fa56c7edbed9c2c4e237e4b325446d0`; el caché caliente no se reescribe. Esta réplica declara como fondo el ensayo de la integrada y la QA del TGV: mide importación y equivalencia, no FPS.

- En el candidato32, normal/reducido/normal repetido con lluvia original recorrió2,5 km por caso: P99 de conducción33/28/28 ms y máximos57,61/46,68/47,09 ms. También mejora la segunda repetición normal, por lo que estos datos no aíslan una ganancia de FPS por bajar calidad. Los presupuestos sí cambian2048/512/2048 partículas,24/0/24 luces de estación y divisor1/4/1de vegetación; ocho luces del tren quedan protegidas y las colas terminan vacías.
- La residencia del candidato32 completó8 km con ambas políticas: con capacidad física8176 MiB y con umbral del proceso1024 MiB. VRAM del proceso2051,63→1879,95 MiB, GTT22,17 MiB en ambos;73→99 mallas de terreno y10→234 formas retiradas. El umbral provoca una política de presión, sin imponer un límite rígido de 1GiB ni provocar agotamiento de la GPU. El segundo caso tiene caché caliente, por lo que no se utiliza para atribuir una mejora de FPS.

- La réplica final de tráfico compila el ejemplo contra las bibliotecas release32 identificadas por SHA, sin recompilar dependencias. Con 1/2/4/8 hilos,33 trenes propios conservan el mismo estado final: conducción1248,8/1219,5/1288,0/1436,7 ms y carga35,78/33,03/31,83/32,03 ms. Los tres trenes del corredor del repositorio también conservan su estado:95,68/115,79/141,22/188,99 ms de conducción. Son60 segundos simulados, sin renderizado; la variante de 8 hilos es más lenta y no se afirma una mejora general de escala.

- El ensayo independiente32 también se rechaza: a 840,033 y 3000,010 s, la misma escena de laboratorio con nieve intensa/cabina conserva 764 entidades, 26imágenes Main,12 atlas, 31 materiales y el mismo presupuesto; las imágenesGPU aumentan 31→43y las texturas anónimas39→51(+3.162.752bytes). La imagen de niebla sigue siendo única de 32 KiB, pero no explica la retención restante. Se detuvo sólo el supervisor propio a 3096,785 s, con 103 muestras hasta3090,012 s, 17 cambios de ruta y51restauracionesJSON en memoria. No se presenta como prueba de 7200 s ni como guardado de partidas en disco.

Los resultados ejecutados de esta tanda se registran junto a sus hashes y configuración. Las cifras de una GPU y un recorrido no constituyen un mínimo garantizado para otros equipos.

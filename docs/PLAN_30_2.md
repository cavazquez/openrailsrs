# Segunda entrega de 30 mejoras

Esta lista conserva los criterios originales de los issues. Un issue se cierra sólo después de implementar, probar y registrar su evidencia. Open Rails sigue fijado en 1.6.1; esta entrega no migra Bevy.

## Seguimiento

- [x] [#231](https://github.com/cavazquez/openrailsrs/issues/231) — Variar masa del vehículo al consumir o reponer combustible
- [x] [#246](https://github.com/cavazquez/openrailsrs/issues/246) — Animar compresión de topes según fuerzas entre vehículos
- [x] [#252](https://github.com/cavazquez/openrailsrs/issues/252) — Actualizar carteles de destino según servicio y sentido
- [x] [#263](https://github.com/cavazquez/openrailsrs/issues/263) — Agregar lavaparabrisas y depósito en material equipado
- [x] [#264](https://github.com/cavazquez/openrailsrs/issues/264) — Permitir abrir ventanas y puertas de cabina compatibles
- [x] [#274](https://github.com/cavazquez/openrailsrs/issues/274) — Conservar silueta de edificios al cambiar LOD
- [x] [#275](https://github.com/cavazquez/openrailsrs/issues/275) — Suavizar apariencia de bosques a diferentes distancias
- [x] [#279](https://github.com/cavazquez/openrailsrs/issues/279) — Mejorar sombras de contacto bajo ruedas, bogies y durmientes
- [x] [#287](https://github.com/cavazquez/openrailsrs/issues/287) — Mejorar puentes desde arriba, abajo y a distancia
- [x] [#289](https://github.com/cavazquez/openrailsrs/issues/289) — Incorporar corrientes y ondas en ríos y canales
- [x] [#337](https://github.com/cavazquez/openrailsrs/issues/337) — Configurar prioridades del despachador
- [x] [#352](https://github.com/cavazquez/openrailsrs/issues/352) — Crear tutorial guiado de conducción básica desde cabina
- [x] [#353](https://github.com/cavazquez/openrailsrs/issues/353) — Crear tutorial guiado de alimentación y cortes eléctricos
- [x] [#355](https://github.com/cavazquez/openrailsrs/issues/355) — Incorporar ejercicios de diagnóstico y respuesta a emergencias
- [x] [#356](https://github.com/cavazquez/openrailsrs/issues/356) — Crear desafíos con condiciones meteorológicas adversas
- [x] [#357](https://github.com/cavazquez/openrailsrs/issues/357) — Mostrar informe de conducción detallado
- [x] [#358](https://github.com/cavazquez/openrailsrs/issues/358) — Ofrecer instructor opcional de frenado
- [x] [#362](https://github.com/cavazquez/openrailsrs/issues/362) — Iniciar prácticas en estaciones o situaciones preparadas
- [x] [#364](https://github.com/cavazquez/openrailsrs/issues/364) — Seleccionar exploración libre sin objetivos de actividad
- [x] [#366](https://github.com/cavazquez/openrailsrs/issues/366) — Crear perfiles de ajustes separados de vehículos originales
- [x] [#368](https://github.com/cavazquez/openrailsrs/issues/368) — Sugerir formaciones apropiadas para el recorrido
- [x] [#370](https://github.com/cavazquez/openrailsrs/issues/370) — Estimar disco, RAM y VRAM antes de jugar
- [x] [#371](https://github.com/cavazquez/openrailsrs/issues/371) — Mostrar cambios entre versiones descargadas de rutas
- [x] [#372](https://github.com/cavazquez/openrailsrs/issues/372) — Reauditar sólo recursos modificados
- [x] [#375](https://github.com/cavazquez/openrailsrs/issues/375) — Gestionar desinstalación con dependencias visibles
- [x] [#376](https://github.com/cavazquez/openrailsrs/issues/376) — Exportar e importar respaldos de partidas y ajustes
- [x] [#379](https://github.com/cavazquez/openrailsrs/issues/379) — Gestionar cola de descargas de repositorios originales
- [x] [#382](https://github.com/cavazquez/openrailsrs/issues/382) — Cancelar trabajo obsoleto al cambiar ruta o selección
- [x] [#395](https://github.com/cavazquez/openrailsrs/issues/395) — Crear editor de evolución meteorológica de actividades
- [x] [#400](https://github.com/cavazquez/openrailsrs/issues/400) — Preparar reporte desde el juego con captura y diagnóstico

## Verificación

Los perfiles, respaldos, informes y descargas viven en los datos del usuario. Los archivos del autor no se modifican. La evidencia gráfica y las métricas están en `docs/fixtures/compatibility/player-expansion-2026-10-12/`. La comprobación general pasó: 1862 pruebas Rust del workspace, 4 pruebas adicionales con contenido nativo y 94 pruebas Python; formato, Clippy y build correctos. Las 17 capturas gráficas y 7 páginas del menú completaron sus controles.


## Cómo probar cada cambio

Iniciá el visor actualizado con `target/debug/openrailsrs-viewer3d --menu`. Para los ejemplos, elegí el nombre indicado en Nueva partida. Para las vistas originales, necesitás tu instalación de Chiltern; los recursos descargados no se distribuyen con este repositorio. Guardar ajustes conserva preferencias para la próxima sesión; guardar partida conserva el estado físico.

1. **#231 · Masa por combustible.** Abrí `traction_operation`, avanzá y mirá combustible en B y masa efectiva en F9. La masa disminuye con el consumo y aumenta al abastecer; la posición y velocidad no saltan. Guardá y continuá: conservan sus valores. Se verifica combustible diésel; no se afirma masa completa de arena, agua de calefacción o reservas de vapor.
2. **#246 · Topes.** Usá el Laboratorio, vista exterior y dinámica por vehículo. Acelerá, cerrá el regulador y frená: el tope compatible del coche responde a la compresión, con máximo 100 mm en este modelo y límite general de 150 mm. El cuerpo queda rígido. Un modelo sin claves de tope conserva su geometría original.
3. **#252 · Carteles.** En el Laboratorio, acercate al frente. Se leen destino y servicio. El cambio de servicio operativo actualiza el destino; invertir cambia el indicador de sentido. Los nodos admitidos se llaman DESTINATION, DESTINATIONBOARD, DESTBOARD, HEADCODE o DEST_DISPLAY, con sufijos numéricos o `_`. No se inventa un panel en modelos sin uno compatible.
4. **#263 · Lavaparabrisas.** En F9 del Laboratorio, activá la bomba. La reserva baja y, desde una cabina compatible, aparecen gotas del lavado sin provocar lluvia fuera. Desconectá batería: deja de consumir. Con reserva vacía no pulveriza. Guardar conserva el depósito. Sólo se habilita en equipos declarados.
5. **#264 · Ventanas y puerta.** En F9 del Laboratorio, abrí cada ventana y la puerta. Sus claves originales se mueven; cerrar devuelve al reposo. Abrir la puerta exige detenerse. Un tren sin esas declaraciones muestra que no dispone del equipo. Guardar conserva posiciones y mandos.
6. **#274 · Edificios.** En Chiltern, alejás/acercás la cámara junto a las estaciones. Los niveles que pierden límites de silueta o materiales conservan el nivel completo anterior. Se mantiene la transición existente, con histéresis y fundido, y la iluminación del autor. Esta política conservadora puede retener más detalle.
7. **#275 · Bosques.** Mirá copas en los bordes de pantalla y alejás la cámara. Los límites incluyen toda la expansión de los billboards, en cualquier orientación; no se elimina un bosque porque sus bases hayan salido del encuadre. Se conservan textura, población, semilla, máscara de alfa y fundido de distancia. No reemplaza los árboles originales por otra especie.
8. **#279 · Contacto.** En F10 → Imagen y cabina, cambiá sombras de contacto entre desactivadas, 8 y 16 pasos. Mirá ruedas, bogies y durmientes en exterior; al entrar en cabina se desactivan. Mové la cámara: no queda una mancha fija en el terreno. Requiere sombras generales; el efecto depende de profundidad visible y tiene alcance de 35 cm.
9. **#287 · Puentes.** Mirá estructuras originales desde cabina, exterior y una cámara baja permitida por el terreno. El LOD conserva superficies y materiales en lugar de perder el tablero o el techo. No se desplaza terreno para ocultar huecos. Las caras disponibles dependen de la geometría del autor.
10. **#289 · Agua.** Cerca de un HWater, F10 → Hora y clima → Corrientes y ondas. Elegí su dirección, velocidad y ondas; desactivá animación para reducir coste. La elevación no oscila por parche, evitando juntas abiertas. Pausar congela la corriente; los cambios de origen no desplazan el plano. La corriente es visual y no altera las vías.
11. **#337 · Prioridades.** En M, ajustá la prioridad del servicio entre −5 y +5. La explicación del despacho indica el motivo de una concesión o espera. Una prioridad alta no atraviesa ocupación ni anula una reserva vigente. La espera prolongada aumenta preferencia cuando el recurso se libera; guardar conserva la prioridad.
12. **#352 · Tutorial básico.** Nueva partida → Ruta → Modo → Tutorial guiado → Conducción desde cabina. Seguí las instrucciones: neutro, puertas, adelante, tracción, frenado y detención. La pantalla avanza al comprobar los controles y estados reales. F7 → Ejercicio permite repetir el paso o empezar de nuevo; guardar conserva el progreso.
13. **#353 · Tutorial eléctrico.** Elegí `electric_supply` y su formación eléctrica. El tutorial pide pantógrafo, disyuntor, atravesar el corte de 150–230 m y recuperar tensión. Verificá pérdida de fuerza y recuperación con retardo, sin frenar artificialmente. Se rechaza un tren no eléctrico o una ruta sin sector declarado sin tensión.
14. **#355 · Emergencias.** En Modo → Tutorial, probá emergencia, batería y motor en `traction_operation`. La emergencia preparada comienza en movimiento y exige parar, rearmar y recargar tubería. Batería y motor usan operaciones F9/B y sus retardos reales. No se borran daños físicos para aprobar una instrucción.
15. **#356 · Desafíos.** Modo → Desafío meteorológico → lluvia, nieve o niebla. El programa tiene semilla, intensidad y viento reproducibles; respeta los mandos y señales. La siguiente parada después de partir completa el ejercicio. F7 → Informe muestra errores, ayudas y paradas. Elegí tráfico del escenario si querés incluirlo.
16. **#357 · Informe.** F7 → Gráficas e incidencias. Conducí, frená y superá brevemente un límite: aparecen velocidad, freno solicitado/aplicado, energía, combustible, sobrevelocidad, comodidad y motivos de intervención. Una acción corta de freno también se cuenta. Las gráficas conservan hasta 3.600 muestras de un segundo y los totales no dependen del render. Guardar conserva la información.
17. **#358 · Instructor.** Activá el asesor en Ejercicio. Con el tren en movimiento y una parada próxima, muestra distancia estimada, pendiente y agarre. Bajo lluvia/nieve, o en descenso, la recomendación puede adelantar la frenada. No mueve el regulador ni el freno. Sin una parada futura no inventa un objetivo.
18. **#362 · Inicio preparado.** Modo → Práctica preparada; elegí estación y punto de parada o 500 m antes. Al Jugar, el tren aparece en la vía resuelta, con controles y frenos inicializados. El horario futuro se refiere al nuevo comienzo. El archivo original no cambia; guardar continúa desde esa posición.
19. **#364 · Exploración.** Modo → Exploración libre; elegí un PAT en Ruta → Opciones y una estación de comienzo. No aparecen objetivos de actividad o maniobras, pero siguen vigentes física, señales, suministro y límites. El fin físico del recorrido sigue siendo su límite. Guardar conserva el modo y posición.
20. **#366 · Perfiles propios.** Tren → Ajustes propios por vehículo. Cambiá masa, freno o resistencia; se muestra autor 100% frente al factor propio y el identificador de versión. Guardá ajustes e iniciá una nueva partida: F9/F8 reflejan la física ajustada. Restaurar valores del autor quita el perfil. Una versión de origen distinta no hereda ese ajuste. La física personalizada necesita validación propia.
21. **#368 · Sugerencias.** En Tren, revisá las tres sugerencias y sus motivos: origen, servicio, recursos, cabina, trocha y alimentación declaradas. Cambiá de ruta para actualizar los avisos. Una opción incompleta o incompatible sigue en la lista y puede seleccionarse para leer el diagnóstico; una recomendación no certifica todos los scripts.
22. **#370 · Estimaciones.** Hora y clima muestra inventario de disco y rangos separados de RAM y recursos gráficos. Abrí antes el preview del tren para incluir su geometría/texturas. Cambiar distancia, detalle o sombras cambia los rangos. Si faltan datos se indica; son estimaciones, no memoria medida. F8 → Diagnóstico muestra mediciones reales y los datos DRM disponibles. En render por CPU los recursos gráficos usan RAM; no hay que sumar RAM y VRAM como consumos independientes.
23. **#371 · Versiones.** Biblioteca → Copias y respaldos. Elegí una versión como referencia y compará otra. Aparecen archivos nuevos, modificados o retirados y partidas que conservan la copia anterior. La comparación usa huellas del contenido instalado; no cambia las partidas a la nueva edición.
24. **#372 · Reauditoría.** Completá un recurso faltante en la ubicación indicada y pulsá Revisar archivos. Se invalidan ese recurso y las formaciones dependientes; las fuentes, formas y cabinas sin cambios se reutilizan. La comprobación incluye Includes, modelos, cabinas, SMS y texturas. El menú conserva su selección durante la revisión.
25. **#375 · Retirar contenido.** Biblioteca → Copias y respaldos → Revisar dependencias. Leé las partidas y paquetes afectados antes de Retirar. Sólo se mueve esa instalación administrada, en un contenedor de recuperación propio. Recuperar devuelve todos los archivos originales; si el destino ya existe, se conserva la copia recuperable. Para retirar, volvé al menú principal y esperá que termine la cola. Las carpetas manuales no son objetivos de retirada.
26. **#376 · Respaldos.** Creá uno en Biblioteca → Copias y respaldos. Importarlo agrega entradas nuevas en Continuar y un archivo separado de ajustes; no reemplaza ranuras. Usar ajustes importados es otro botón explícito. Para otro equipo, copiá el JSON en la carpeta `backups` indicada. Se incluyen versión, referencias y huellas; no credenciales ni estados opacos de scripts externos. Los contenidos ausentes se enumeran, sin descargarlos automáticamente.
27. **#379 · Cola.** Biblioteca → Descargas originales; agregá dos paquetes diferentes. Hay progreso y cancelación por trabajo, y no se repite una instalación pendiente del mismo paquete. Cancelar uno deja seguir al siguiente. Al reiniciar, la cola queda detenida hasta Retomar. Las fuentes siguen siendo las del catálogo original y cada versión conserva su destino.
28. **#382 · Cancelación.** Cambiá rápidamente ruta, servicio o formación mientras se prepara una vista; al salir se cancelan lecturas obsoletas y no aparece un resultado de la selección anterior. Al Jugar se detienen auditorías de menú innecesarias. Los trabajos de instalación ya solicitados siguen su propia cancelación y no se interrumpen por cambiar de pantalla.
29. **#395 · Editor climático.** Ruta/Hora y clima o F10 → Editor de evolución. Agregá fases, editá hora, transición, intensidad, viento y semilla; mové el tiempo del preview. Guardar escribe `weather-profiles/custom.json`; cargar valida el archivo antes de aplicarlo. Activar utiliza el mismo programa para clima visual y adherencia de jugador/tráfico; guardar partida conserva programa y reloj. La fase inicial comienza en 0 y no puede borrarse.
30. **#400 · Reporte.** Pausa → Informe de error → Preparar. Revisá el texto y datos físicos. Capturar guarda sólo la escena de la ventana del juego y oculta UI/inspector durante la captura. Los archivos quedan en `reports`; no se envían. Copiar texto o abrir el formulario de GitHub requieren otro clic; el envío lo hace el jugador. No se incluyen rutas personales, entorno, logs completos ni estados de scripts. El diagnóstico identifica la versión y SHA del binario.

## Evidencia y límites

- Oráculo nuevo: `oracles/openrails-consumable-mass.json`, obtenido invocando `MSTSWagon.UpdateLocomotiveLoadPhysics` en las DLL sin modificar de OR 1.6.1. Cinco reservas y peso adherente, tolerancia de 0,02 kg. OR activa esa masa con animación de carga continua; aquí la reserva diésel efectiva se aplica también al contenido sin esa declaración. No se alteran las tolerancias de velocidad/posición existentes.
- Las prácticas eléctricas y de emergencia usan las mismas funciones que las suites originales de alimentación, frenos y controles. Los equipos no declarados permanecen deshabilitados.
- Pruebas de seguridad: un respaldo con fases vacías o punto de práctica inválido se rechaza antes de escribir entradas; retirar y recuperar conserva incluso un archivo del autor llamado `openrailsrs-recovery.json`.
- Las capturas internas comparan cámaras y materiales en las condiciones registradas; no se presentan como paridad de píxeles universal con Open Rails. Las referencias visuales OR ya fijadas conservan su propio alcance.
- Las estimaciones previas usan metadatos y bytes decodificados del preview, más rangos de expansión/cachés. No consultan ni inventan una cifra exacta de VRAM. La biblioteca y los límites de importación son acotados y se muestran como tales.

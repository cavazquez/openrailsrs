# Actividades nativas — alfa.9

Los espaciadores MSTS invisibles se conservan como vehículos de la formación:
su longitud ocupa la vía, mantiene separados los coches y viaja con su UID al
guardar. El cargador aplica el piso de masa física de 0,1 kg de Open Rails
1.6.1 (`MSTSWagon.Parse`, caso `wagon(mass`). El lector conserva la cantidad
declarada en el WAG y no reescribe los archivos originales. Las masas
inválidas o no positivas siguen rechazándose.

Los UID de coches se leen como `Int32`, como en OR. Se conservan `-8` y `-7`
del CON de Chiltern sin renumerarlos. Las identidades positivas mantienen
su representación anterior. Una extensión interna separada guarda la pareja
servicio/UID negativo sin confundir `-8` con el UID positivo `65528`. Los
eventos ACT siguen usando sus identificadores originales de 32 bits. La
extensión cabe exactamente en JSON y TOML; la interfaz muestra la identidad
original con signo.

Antes de habilitar **Jugar**, el menú comprueba la actividad elegida en un
trabajador cancelable. La comprobación comparte la preparación de lanzamiento
y el cargador de sesiones de AI y formaciones estáticas. Revisar los archivos
del tren del jugador ya no basta: un error de recorrido, paradas o geometría
de otro actor bloquea el lanzamiento con su nombre y motivo. Cambiar servicio,
formación, itinerario, hora o estación del año invalida el resultado anterior;
**Reauditar** permite revisar recursos completados o modificados.

Esta comprobación carga los actores sin adelantar su reloj. El precalentamiento
del tráfico, la circulación y el cumplimiento de los objetivos originales se
verifican por separado. Un archivo ACT que se puede importar no constituye,
por sí solo, una actividad completa certificada.

La revisión del código OR distingue los PDP marcados `9`, el enlace dirigido
de vectores y desvíos, las inversiones y las tareas de pasajero del jugador.
En modo actividad, la marca `9` por sí sola no prueba que el PAT esté roto.
Una estación existente fuera del recorrido no se elimina para satisfacer
`AllStops`. Las limitaciones de colocación y los archivos originales ausentes
se informan por separado.

La distribución incluye código y recursos propios. Las rutas descargadas y
las cabinas comerciales permanecen en las instalaciones locales del jugador.

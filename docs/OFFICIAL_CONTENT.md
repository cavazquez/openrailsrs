# Contenido del catálogo oficial

En el menú principal, **Biblioteca → Descargas del autor** muestra autor, origen,
tamaños anunciados y tipo de distribución. El catálogo se conserva desde el
[repositorio oficial](https://github.com/openrails/content/tree/2bc71f58d04fbce693564db74d577c0ee00b7e31),
publicado por [Open Rails](https://www.openrails.org/download/content/).
La entrada **Chiltern** se actualiza a **Chiltern v4** desde el
[repositorio original de DocMartin](https://github.com/DocMartin7644/Chiltern-Route-v4),
verificado el 6 de octubre de 2026. La captura del catálogo de OR todavía usa
el nombre y enlace anteriores; GitHub redirige esos enlaces de v2 y v3 a v4.
La URL y el hash del catálogo conservan la procedencia de la lista original.
La entrada actual registra por separado su `source_url`, fecha de verificación
y `previousSources`, para reconocer las instalaciones del enlace antiguo.

## Descargar y jugar

1. Elegí **Demo Model 1** para una primera prueba: ZIP del servidor de Open Rails,
   unos 260 MiB de descarga y 315 MiB instalado. Otros paquetes pueden ser mucho
   mayores; el menú muestra los tamaños del catálogo.
2. Pulsá **Buscar actualización e instalar**. La descarga y la preparación corren en segundo
   plano. El panel informa MiB, extracción y auditoría. Podés cancelar; los
   temporales se limpian y el contenido anterior se conserva.
3. Al terminar, volvés al menú y elegís la ruta instalada y una actividad.
   Se utiliza su red importada, escenario nativo, formación y recorrido. La
   selección de formación conserva los diagnósticos de modelos, texturas,
   cabinas, sonidos y sistemas antes de iniciar.
   Demo Model 1 admite la formación original **MT_MT_Class 47 & 6 mk2 PP**:
   siete vehículos, 318800 kg, 139,90 m y cabina 3D. Los `.inc` se expanden
   antes de interpretar masa, motor, frenos, cabina, luces y efectos.
   Su referencia 2D ausente se informa como alternativa no disponible; la
   cabina 3D válida permite conducir. La descarga probada contiene 23
   formaciones: 15 con recursos obligatorios válidos y 12 con tracción.
   Las formaciones AI pueden no traer cabina. Otras copias conservan faltantes
   reales de modelos o `.inc`; no se fabrican esos archivos.
4. **Reauditar esta copia** vuelve a preparar una instalación sin red, útil
   después de actualizar el lector o agregar recursos legítimos del autor.
   Buscar actualización consulta el origen de nuevo y reutiliza una copia
   cuando su commit o ETag coincide; un ZIP sin esos metadatos requiere
   volver a descargarlo para comparar su SHA-256.

## Ediciones y almacenamiento

Las descargas **no se versionan en Git ni fijan las futuras actualizaciones**.
Cada paquete queda en `official-content/<id>-<hash>` dentro de la carpeta de
datos del usuario. En cada actualización de GitHub se consulta la rama actual
del autor, incluidas redirecciones de repositorio. El commit registrado identifica
esa copia descargada; una actualización posterior se instala junto a ella.
Chiltern conserva también el piloto local que ya tenías.

**Chiltern v4** tiene su propio id, `chiltern-v4`, y conserva las copias
anteriores y la instalación local. El catálogo ofrece una sola descarga actual;
el antiguo comando `--package chiltern` es un alias de `chiltern-v4`.
Se consulta la rama actual del repositorio original al instalar o buscar una
actualización. Los 8,51 GiB
anunciados corresponden a sus archivos originales; el ZIP, la extracción
temporal y la preparación requieren espacio adicional. Ruta, trenes, texturas
y sonidos quedan fuera del checkout. El menú identifica la versión cuando el
repositorio registrado corresponde a un origen conocido, además de la fecha e
identificador de cada copia. Si una copia hecha con el enlace de v2 ya registró
el repositorio canónico de v4, se muestra como v4. Una versión desconocida se
identifica por fecha y commit/hash, sin atribuirle una versión anterior.

Para probar la revisión `8236df20`, elegí **RS_Football Special** y la formación
del mismo nombre: Banbury General, Bicester North, Princes Risborough y High
Wycombe. Conserva el trazado y las plataformas originales, con vapor y cabina
2D. Algunas otras actividades del paquete necesitan compatibilidad adicional
o referencias de plataforma de la misma edición. El menú informa esos errores;
una formación con archivos completos no garantiza que su actividad se pueda
importar. [Prueba manual](PLAYER_MANUAL_TESTS.md#41-chiltern-v4-descarga-del-autor-y-copias-separadas).

El selector de ruta distingue las copias por fecha del origen y commit/hash:
por ejemplo **Chiltern · origen 2026-10-04 · abcdef12**. GitHub proporciona
fecha de actualización del repositorio (o fecha del commit); para ZIP se usa
`Last-Modified`. Esa fecha no certifica una publicación formal del autor.
Cuando no hay fecha, se muestra **descarga abcdef12**. La fecha local de
descarga se registra aparte, sin presentarla como fecha del autor.

La prioridad de almacenamiento, compartida por CLI y visor, es:

- `OPENRAILSRS_PLAYER_DIR`: ubicación explícita, también para un modo portátil.
- Snap: `$SNAP_USER_COMMON/openrailsrs`, común entre revisiones, sin escribir
  dentro de `$SNAP`. [Variables oficiales de Snap](https://snapcraft.io/docs/reference/development/environment-variables/).
- Linux: `$XDG_DATA_HOME/openrailsrs` o `~/.local/share/openrailsrs`.
- Windows: `%LOCALAPPDATA%/openrailsrs`.
- macOS: `~/Library/Application Support/openrailsrs`.

Mover o reemplazar el binario no mueve los paquetes. Se copian preferencias
y tres partidas antiguas desde `player-data` si el nuevo destino todavía no
las contiene, conservando los originales. Las instalaciones antiguas cercanas
al lanzamiento se descubren sin mover sus recursos; para reubicarlas conviene
copiar el paquete entero y reauditarlo, ya que sus manifiestos preparados
contienen rutas absolutas.

La copia conserva licencias y archivos originales. `openrailsrs-content.json`
registra catálogo, autor, URL, commit cuando corresponde, SHA-256 del ZIP,
fechas del origen/descarga, ETag y tamaños. Ese hash identifica la descarga;
no es una firma del autor.
`openrailsrs-audit.json` contiene el resultado por formación y
`openrailsrs-prepared.json` enumera las rutas/actividades preparadas localmente.

## Comandos

```bash
target/debug/openrailsrs content --list
target/debug/openrailsrs content --package demo-model-1
# Chiltern v4, desde el repositorio original y sin reemplazar la copia anterior:
target/debug/openrailsrs content --package chiltern-v4
# Auditar/importar una descarga hecha por CLI, sin ventana:
target/debug/openrailsrs-prepare-content /ruta/al/paquete-instalado
```

`OPENRAILSRS_PYTHON` permite elegir Python 3. El helper y catálogo están
embebidos en los binarios y se materializan en la carpeta de datos: descargar
no depende de un checkout ni del directorio actual. Python 3 sigue siendo
una dependencia de ejecución; la receta Snap lo incluye, junto con
certificados TLS y permisos de red/gráficos. El paquete se construye con
core24 y confinamiento estricto. La [guía de distribución](DISTRIBUTION.md)
registra las pruebas del binario trasladado y el estado de la instalación Snap.
La CLI instala; la preparación se hace en el
menú o con `openrailsrs-prepare-content`. Los ZIP gratuitos directos y repositorios GitHub
del catálogo tienen instalación automática. **Ver catálogo oficial** abre
la página para los paquetes que usan una web, instalador o distribución comercial.

## Recursos faltantes y CAF 6000

**Abrir carpeta del escenario** abre su ubicación real en el gestor de archivos;
cada edición instalada también ofrece **Abrir carpeta**. **Copiar diagnóstico**
reúne escenario, servicio, formación, referencias ausentes y destinos absolutos
en el portapapeles. Es útil para completar archivos del autor y reauditar la
copia. Si el sistema no ofrece portapapeles o gestor de archivos, se muestra el
error y el panel conserva los detalles.

La [sección Contenido de la web](https://cavazquez.github.io/openrailsrs/contenido.html)
presenta los mismos registros del catálogo integrado, incluida Chiltern v4.
Permite buscar por ruta/autor y filtrar gratuitos o instalables desde el juego. Las webs comerciales
y las descargas que requieren acceso manual conservan el enlace original;
no se muestran como ZIP instalables automáticamente.

El menú y **Descargar contenido oficial** muestran la carpeta del escenario
seleccionado, el archivo de actividad/servicio y la formación `.con`. El detalle
**Ver todos los faltantes y sus ubicaciones** abre una ventana propia que
enumera cada archivo ausente, el
ENG/WAG, modelo, CVF, SMS o Include que lo referencia y sus destinos absolutos.
Si hay varias ubicaciones admitidas por el cargador, basta con una de ellas.
Los archivos obligatorios impiden iniciar; sonidos, scripts y cabinas
alternativas opcionales conservan sus avisos. La auditoría identifica todas
las texturas y gráficos ausentes, sin detenerse en el primero de cada modelo.

**Buscar en el repositorio original** y **Actualizar desde el repositorio
original** aparecen solamente si el recurso pertenece a un paquete GitHub
registrado en el catálogo integrado. Se usa la URL original del catálogo,
incluso cuando GitHub redirige una ruta renombrada por su autor. Un manifiesto
no puede cambiar ese enlace mediante `revision.repository` o `download_url`.
La búsqueda envía únicamente el nombre del archivo, sin rutas locales. La
actualización conserva la copia anterior y la nueva se audita por separado.
El repositorio de un escenario no se atribuye a una formación de otra biblioteca.

Si no se conoce ese origen, se muestran los nombres y destinos para completar
los recursos manualmente; no se ofrecen modelos de otros autores ni sitios
alternativos. Los paquetes ZIP del catálogo oficial mantienen su descarga
original, pero no se les inventa un repositorio para buscar dependencias.
Después de copiar los archivos originales, pulsá **Reauditar esta formación**
en el detalle. Se vuelven a leer los recursos y se conservan ruta, actividad,
formación, recorrido y hora elegidos. **Reauditar esta copia** en el gestor
repite además la preparación del paquete instalado.

Los nombres `caf6000_motor.s`, etc. del ejemplo Mitre son marcadores de un
ejemplo de física, no archivos de un modelo original. No se identificó un
repositorio original para ese modelo y no se ofrece una descarga sustitutiva.
No representan el material rodante real del Mitre. Para probar una formación
real obtenida de su origen, conservá la estructura y los nombres del autor:
`rolling-stock/TRAINS/TRAINSET/<carpeta-del-autor>/` y sus `.con` en
`rolling-stock/TRAINS/CONSISTS/`, dentro de la carpeta de datos. Reiniciá el
menú y elegí su formación original; se audita antes de iniciar. Las ubicaciones
del detalle corresponden a la formación seleccionada, no a un modelo parecido.

Demo Model 1 es pequeño comparado con rutas de varios GiB, pero pesado para
Git (260 MiB comprimido / 315 MiB instalado). Se conserva como opción del
catálogo. Para distribuir una edición con contenido precargado, primero hay
que confirmar la licencia de redistribución y preparar un paquete separado.
La descarga gratuita no establece por sí sola ese permiso.

## Límites y validación

La descarga admite hasta 8 GiB y la extracción hasta 32 GiB, con comprobaciones
de espacio libre. Se instala en una carpeta nueva mediante publicación atómica.
La extracción rechaza rutas que escapan, enlaces, nombres ambiguos por
mayúsculas, archivos especiales, paquetes cifrados y metadatos reservados al
preparador. Cada redirección se verifica **antes** de emitir la siguiente
petición: solamente HTTPS y proveedores del catálogo. Los manifiestos locales
usan rutas canónicas confinadas al paquete, también al encontrar enlaces.
`Include` admite anidación, Unicode UTF-16, separadores Windows y diferencias
de mayúsculas. Se limita a 32 niveles y 32 MiB; rechaza ciclos y referencias
fuera de la instalación `TRAINS`, incluidos enlaces que escapan.

Un paquete instalado no certifica todos sus sistemas. Las actividades no
soportadas muestran el error del importador; los recursos obligatorios ausentes
siguen impidiendo iniciar. No se obtienen automáticamente todos los archivos
faltantes de una formación: algunas dependencias provienen de MSTS u otros
autores y requieren su propia distribución/licencia. Tampoco se ejecutan
scripts C# o instaladores encontrados al descargar.

```bash
python3 scripts/test_official_content.py
```

Las regresiones usan archivos y respuestas de red en memoria: publicación
atómica, cancelación, procedencia, recursos corruptos, travesía, enlaces,
límites de tamaño y rechazo de redirecciones antes de contactar su destino.

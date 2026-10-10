# Binario Linux trasladable y Snap

Los recursos originales se instalan por separado. Los paquetes incluyen tres
binarios, shaders, recursos propios y ejemplos; excluyen descargas, datos del
jugador, salidas de simulación y credenciales. El manifiesto de recursos es una
lista explícita. El instalador y el catálogo de contenido están embebidos.

## Paquete Linux

```bash
CARGO_BUILD_JOBS=2 cargo build --locked --release -p openrailsrs-cli -p openrailsrs-viewer3d --bins
python3 scripts/package_linux.py --binaries target/release \
  --output tmp/dist/openrailsrs-linux --archive tmp/dist/openrailsrs-linux.tar.gz
```

Descomprimí el archivo completo y conservá `bin/` y `share/` junto a `Jugar.sh`.
En la carpeta extraída, incluso si su nombre contiene espacios:

```bash
./Jugar.sh --check  # dependencias, recursos y catálogo
./Jugar.sh          # menú, sin Rust ni Cargo
./Jugar.sh --cpu    # software Mesa/lavapipe
./Jugar.sh --gpu    # exigir una GPU compatible
```

`LEEME.txt` contiene los pasos y `BUILD.json` registra commit, estado de fuentes,
SHA-256 de binarios y `required_glibc`, obtenido de las versiones ELF con
`readelf` cuando está disponible. Desde cualquier carpeta también podés usar
la ruta absoluta de `bin/openrailsrs content --list`. Python 3 y las bibliotecas gráficas/audio del
sistema siguen siendo dependencias del paquete portátil. Un binario compilado
en una distribución reciente puede requerir su versión de glibc; para publicar
una descarga general, compilá en la distribución mínima soportada. La
construcción de QA de esta revisión necesita **glibc 2.43 o superior**: se
comprobó la dependencia ELF y no se presenta como compatible con Ubuntu 24.04
u otras distribuciones con versiones anteriores. El Snap se compila dentro de
core24 y tiene una construcción independiente.

El visor resuelve los recursos junto al ejecutable. `OPENRAILSRS_RESOURCES`
permite indicar otra carpeta absoluta que contenga `assets/` y `examples/`.
Los datos del jugador conservan la ubicación XDG habitual, independientemente
del directorio de ejecución. `OPENRAILSRS_PLAYER_DIR` permite aislar una prueba.

## Snap de desarrollo

`snap/snapcraft.yaml` usa core24, amd64 y confinamiento estricto. Usa Python y
certificados TLS del paquete o de core24; incluye bibliotecas gráficas y mapas
XKB, y declara interfaces de red, audio, pantalla y GPU. El lanzador configura
los directorios de Vulkan y teclado y evita un `PYTHONHOME` incompatible con
el intérprete de la base.
Para un checkout con descargas/builds locales, prepará primero una copia limpia:

```bash
python3 scripts/package_snap.py --source-dir tmp/dist/snap-source
cd tmp/dist/snap-source
snapcraft pack --use-lxd
sudo snap install --dangerous openrailsrs_0.1.0-alpha.2_amd64.snap
openrailsrs.cli content --list
openrailsrs
```

Snapcraft necesita su entorno LXD configurado. `--refresh` actualiza las fuentes
de una copia preparada para reintentar la construcción. La herramienta filtra
archivos por Git y las adiciones explícitas de empaquetado; nunca copia `target/`,
`tmp/`, `.git` ni recursos descargados no versionados.

Las descargas, guardados, ajustes y caché se almacenan en
`$SNAP_USER_COMMON/openrailsrs`, conservado entre revisiones. `$SNAP` es de solo
lectura. Para contenido de un disco externo, conectá explícitamente
`sudo snap connect openrailsrs:removable-media`; la carpeta debe seguir siendo
accesible al usuario. La construcción local no publica en Snap Store: la subida
se hace con un comando separado.

## Corrección de audio preparada

Las fuentes incluyen el puente ALSA–PulseAudio y el descubrimiento del socket de la sesión. Se comprobó la apertura real con contenido original en cabina y exterior bajo el perfil estricto, usando un payload de prueba sin cambiar el Snap instalado. La revisión 2 de edge todavía no incluye esta corrección; requiere construir y publicar otra revisión. [QA de audio](NATIVE_AUDIO.md#audio-dentro-del-snap).

## Alpha disponible en Snap Store

Actualmente **latest/edge** ofrece **0.1.0-alpha.2**, revisión **2**, publicada
el 9 de octubre de 2026. Corrige el arranque en Wayland y el aviso falso de
NVIDIA: resuelve el socket de la sesión, conserva el directorio privado de
Snap y usa X11 si Wayland no responde y hay una conexión X11 disponible.

El **9 de octubre de 2026** se publicó **0.1.0-alpha.1**, revisión **1**, para
Linux amd64 en [Snap Store](https://snapcraft.io/openrailsrs), canal **latest/edge**.
Tiene `grade: devel`, `confinement: strict` y base core24. Stable, candidate y
beta todavía no tienen revisiones publicadas.

```bash
sudo snap install openrailsrs --edge
openrailsrs
# Si ya está instalado:
sudo snap refresh openrailsrs --edge
```

El menú permite descargar las rutas y trenes originales desde **Biblioteca**.
El Snap no incluye ese contenido ni datos del jugador. La actualización
conserva las descargas, ajustes y partidas en `$SNAP_USER_COMMON/openrailsrs`.

Snap Store ofrece stable, candidate, beta y edge, sin un riesgo llamado alpha.
Una versión devel no puede publicarse en stable o candidate. Ver
[canales de Snapcraft](https://ubuntu.com/docs/snapcraft/9/reference/channels/).

El paquete de la primera revisión mide **120.311.808 bytes**. Se descargó nuevamente desde
edge y su SHA-256 coincide con el archivo probado:
`1135d595164cbe13298bb511dacff1ffbb5f6f159e2f40a7a225efc908125bd0`.
Se construyó con Snapcraft 9.1.4 y Rust 1.99.0 en Ubuntu 24.04/core24; sus
binarios requieren glibc 2.39, provista por la base.

La instalación real en una instancia LXD con confinamiento estricto comprobó
la CLI y sus quince entradas de catálogo, HTTPS con validación de certificados,
el menú y escenas de lluvia y nieve. Se usaron X11 virtual, Vulkan por software
y partículas CPU. Los datos comunes sobrevivieron a las actualizaciones locales.
Las ocho pruebas de empaquetado y la auditoría de archivos propios pasaron.
Esta prueba no certifica GPU física, audio, Wayland ni un recorrido original
completo dentro de Snap; HTTPS verifica conectividad, no la instalación completa
de una ruta. [Capturas, hashes y alcance de QA](fixtures/compatibility/snap-edge-2026-10-09/README.md).

El paquete de QA anterior, `0.1.0`, precede los cambios de Hanabi y no corresponde
a esta publicación.

La revisión 2 también mide **120.311.808 bytes**; su SHA-256 es
`ad8ef7086b46b85afbf0cb16537f7ac051fd54b348b08f0155b875af0a7ab8ad`.
El paquete completo se instaló en confinamiento estricto y abrió el menú con
Wayland, con el respaldo X11 y con X11 explícito. El binario y lanzador extraídos
también abrieron con Wayland y RX 7600 dentro de la instalación estricta anterior
del equipo, sin sustituirla. Pasaron 13 pruebas de empaquetado, nueve pruebas
Rust del binario, formato y Clippy. [Capturas y alcance de esta corrección](fixtures/compatibility/snap-wayland-2026-10-09/README.md).
Para elegir el backend explícitamente, consultá [VIEWER3D.md](VIEWER3D.md#snap-waylanderrorconnectionnocompositor-antes-del-menú).

Para inspeccionar una distribución extraída, sin subirla ni imprimir valores
sensibles:

```bash
python3 scripts/package_linux.py --audit /ruta/al/paquete-extraido
```

El ensamblador aplica esa comprobación automáticamente a los archivos propios
y binarios: rechaza rutas personales de Linux/macOS/Windows, claves privadas y
formatos comunes de tokens. También excluye configuraciones del jugador,
repositorios Git, credenciales y descargas. No es un detector universal de
secretos. Los baselines, capturas, logs, CSV y replays de desarrollo no se
distribuyen. Se conservan los escenarios jugables, shaders, licencias y
créditos públicos; los contactos de una licencia no se censuran.

Para una nueva revisión, reconstruí y probá el Snap antes de subirlo. Si la
cuenta todavía no está autenticada, iniciá sesión:

```bash
snapcraft login
snapcraft upload openrailsrs_0.1.0-alpha.2_amd64.snap --release latest/edge
```

No se guardan credenciales de Snapcraft dentro del repositorio ni del paquete.
La tienda muestra el usuario y nombre público del publicador, tomados de la
cuenta de Ubuntu One; revisalos antes de publicar. Esos datos son ajenos al
binario. Ver [datos del publicador](https://dashboard.snapcraft.io/docs/reference/v1/snap.html#about-publisher).
El comando de publicación sigue la
[guía oficial de revisiones y releases](https://documentation.ubuntu.com/snapcraft/8.9.0/how-to/publishing/manage-revisions-and-releases/).

## Revisión de empaquetado del 7 de octubre de 2026

La inspección del Snap anterior encontró una ruta personal en la descripción
de `examples/chiltern/scenario.toml` y cuatro registros de ensayos incluidos.
No detectó claves privadas ni los formatos de tokens examinados en los archivos
propios y binarios. La descripción se corrigió y los registros quedan fuera
del ensamblado. Los 205 recursos actuales incluyen los shaders de Hanabi,
34 candidatos de escenarios TOML y las licencias. Se comprobó esa selección
con los binarios anteriores para auditar archivos, sin presentarla como una
alpha nueva ni como una prueba del juego actualizado dentro de Snap.

Snapcraft validó la receta con `expand-extensions`; las siete pruebas de
empaquetado pasaron y `check.sh` completó 1.681 pruebas Rust y 67 Python.
En esa fecha el Snap no estaba instalado en el host y no se había publicado en
tienda. La prueba y publicación del 9 de octubre se registran arriba.

## Estado de las pruebas del 5 de octubre de 2026

Se construyó el Snap real con Snapcraft 9.1.3 en core24, Rust estable 1.99.0.
El paquete portátil de QA, compilado en el host con Rust 1.97.1, se trasladó y
abrió desde una carpeta vacía. El menú mostró sus escenarios empaquetados,
fuentes con tildes y los botones de carpeta/diagnóstico. La CLI listó los quince
registros sin utilizar el checkout.

La instalación del Snap en el host requiere la autenticación del administrador:
`snap install` devolvió «access denied» y `sudo` pidió una terminal para
introducir la contraseña. No se presenta la construcción como certificación
del confinamiento. Cuando esté instalado, falta comprobar el menú, una partida,
red/descarga original, portapapeles y persistencia entre revisiones.
El usuario pospuso la instalación hasta volver a su equipo. El `.snap` de QA
queda en `tmp/five-items/snap-source/openrailsrs_0.1.0_amd64.snap`; los resultados
del paquete portátil y del renderizador se registran en [PLAYER_POLISH_QA.md](PLAYER_POLISH_QA.md).

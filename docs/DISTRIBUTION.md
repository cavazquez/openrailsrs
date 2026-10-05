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

`snap/snapcraft.yaml` usa core24, amd64 y confinamiento estricto. Incluye Python,
TLS y bibliotecas gráficas; declara interfaces de red, audio, pantalla y GPU.
Para un checkout con descargas/builds locales, prepará primero una copia limpia:

```bash
python3 scripts/package_snap.py --source-dir tmp/dist/snap-source
cd tmp/dist/snap-source
snapcraft pack --use-lxd
sudo snap install --dangerous openrailsrs_0.1.0_amd64.snap
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
accesible al usuario. La receta de desarrollo no publica nada en Snap Store.

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

# Paquetes de contenido por URL

Esta guía corresponde a **0.1.0-alpha.10** y posteriores. La entrega para Snap
y sus [fuentes exactas](https://github.com/cavazquez/openrailsrs/releases/tag/v0.1.0-alpha.10)
se publican por versión; para repetir su compilación, usá ese archivo de fuentes.

El formato `openrailsrs-package`, versión 1, permite instalar rutas, trenes o
ambos desde un ZIP del autor. Cada paquete incluye `openrailsrs-package.json`.
Las carpetas `ROUTES` y `TRAINS` son opcionales por separado; `contents` declara
exactamente cuáles contiene el paquete.

El juego instala cada edición en la biblioteca del usuario y conserva las
anteriores. La importación comprueba el formato y luego prepara la red y audita
las formaciones. Una instalación correcta permite revisar el contenido; para
habilitar **Jugar** también deben pasar las comprobaciones de recursos, ruta y
tráfico de la actividad elegida.

## Ejemplos incluidos

Estos ejemplos contienen datos propios bajo GPL-3.0-only. La ruta es una red
mínima para probar la importación, sin actividades ni paisaje completo. Los
vehículos son modelos de laboratorio. Se pueden empaquetar con los comandos de
esta guía y reemplazar sus datos por contenido del autor con permiso de distribución.
Para ofrecer servicios en **Nueva partida**, una ruta debe incluir actividades
ACT u horarios compatibles. Los ejemplos de ruta se comprueban en **Biblioteca**.

Para probar la importación en la alpha.10, copiá uno de estos enlaces al
manifiesto publicado y pegalo en **Instalar desde URL**:

- [Sólo ROUTES](https://raw.githubusercontent.com/cavazquez/openrailsrs/main/docs/examples/content-packages/downloads/routes-only/openrailsrs-package.json).
- [Sólo TRAINS](https://raw.githubusercontent.com/cavazquez/openrailsrs/main/docs/examples/content-packages/downloads/trains-only/openrailsrs-package.json).
- [ROUTES y TRAINS](https://raw.githubusercontent.com/cavazquez/openrailsrs/main/docs/examples/content-packages/downloads/routes-and-trains/openrailsrs-package.json).

### Sólo ROUTES

[Ejemplo de ruta](examples/content-packages/routes-only/)

```text
routes-only/
├── openrailsrs-package.json
├── LICENSE.txt
├── README.txt
└── ROUTES/
    └── ExampleRoute/
        ├── ExampleRoute.trk
        ├── ExampleRoute.tdb
        └── WORLD/
            └── w-000000-000000.w
```

El manifiesto declara `"contents": ["ROUTES"]`. Una ruta real conserva además
sus archivos de terreno, texturas, formas, actividades, servicios y recorridos.
Cada TRK debe estar directamente dentro de `ROUTES/NombreDeRuta/` y su ruta debe
tener `WORLD`. Los trenes pueden provenir de una instalación vinculada.

### Sólo TRAINS

[Ejemplo de trenes](examples/content-packages/trains-only/)

```text
trains-only/
├── openrailsrs-package.json
├── LICENSE.txt
├── README.txt
└── TRAINS/
    ├── CONSISTS/
    │   └── example.con
    └── TRAINSET/
        └── ExampleLab/
            ├── lab.eng
            ├── lab.wag
            ├── lab.s
            ├── coach.s
            └── lab.ace
```

El manifiesto declara `"contents": ["TRAINS"]`. Los vehículos ENG y WAG van
dentro de `TRAINS/TRAINSET/NombreDelTren/`; las formaciones originales conservan
sus referencias y nombres. También se admiten paquetes de formaciones, cabinas o
sonidos sin ENG/WAG. Un paquete de recursos compartidos aporta archivos a una
instalación vinculada y no crea un vehículo ficticio para poder seleccionarlo.

### ROUTES y TRAINS

[Ejemplo combinado](examples/content-packages/routes-and-trains/)

```text
routes-and-trains/
├── openrailsrs-package.json
├── LICENSE.txt
├── README.txt
├── ROUTES/
│   └── ExampleRoute/
│       ├── ExampleRoute.trk
│       ├── ExampleRoute.tdb
│       └── WORLD/
└── TRAINS/
    ├── CONSISTS/
    └── TRAINSET/
        └── ExampleLab/
```

El manifiesto declara `"contents": ["ROUTES", "TRAINS"]`. Las dos carpetas
comparten la misma raíz de contenido, como en una instalación nativa de MSTS u
Open Rails. Las dependencias adicionales pueden seguir siendo locales.

## Manifiesto del paquete

Este es el manifiesto mínimo del ejemplo de trenes:

```json
{
  "format": "openrailsrs-package",
  "format_version": 1,
  "id": "example-trains-only",
  "name": "Ejemplo trains-only",
  "version": "1.0.0",
  "author": { "name": "OpenRailsRS contributors" },
  "license": "GPL-3.0-only",
  "contents": ["TRAINS"],
  "dependencies": []
}
```

`id` usa hasta 80 caracteres: letras minúsculas, números, punto, guion y guion
bajo, comenzando con una letra o número. `name`, `version`, `license` y
`author.name` son textos obligatorios de hasta 256 caracteres. Usá un
identificador SPDX de licencia cuando corresponda, o una referencia a los
permisos que incluís con el paquete. Conservá las licencias y créditos del autor.

`contents` admite `ROUTES`, `TRAINS` o ambos, sin duplicados. Omití físicamente
las carpetas que no declarás. `dependencies` es opcional y puede contener hasta
16 requisitos. La versión 1 rechaza campos desconocidos para detectar errores
en los nombres del manifiesto.

El JSON usa UTF-8 y tiene un límite de 128 KiB. Se admite la raíz directamente
en el ZIP o dentro de una carpeta contenedora, como en los ZIP de GitHub. Debe
haber un solo manifiesto de raíz. `openrailsrs-content.json`,
`openrailsrs-prepared.json`, `openrailsrs-audit.json` y `openrailsrs-import` son
metadatos generados por el juego y se excluyen del paquete publicado.

## Validar y crear un ZIP

Desde la raíz del repositorio, validá uno de los ejemplos sin red ni instalación:

```bash
python3 scripts/download_official_content.py \
  --check-package docs/examples/content-packages/routes-only
```

También está disponible en la CLI compilada:

```bash
openrailsrs content --check-package /ruta/a/mi-paquete
```

Creá el ZIP y su manifiesto de descarga. Reemplazá el dominio de ejemplo por el
servidor HTTPS del autor:

```bash
python3 scripts/build_content_package.py \
  docs/examples/content-packages/routes-only \
  --output tmp/publicacion/ruta-ejemplo/ruta-ejemplo.zip \
  --archive-url https://tu-sitio.example/paquetes/ruta-ejemplo/ruta-ejemplo.zip
```

El comando produce `ruta-ejemplo.zip` y `openrailsrs-package.json` en
`tmp/publicacion/ruta-ejemplo/`. Para crear los otros paquetes, usá como origen
`docs/examples/content-packages/trains-only` o
`docs/examples/content-packages/routes-and-trains` y un directorio de salida
propio. Las fuentes permanecen intactas.

El manifiesto externo conserva los datos del manifiesto incluido y agrega:

```json
"archive": {
  "url": "https://tu-sitio.example/paquetes/ruta-ejemplo/ruta-ejemplo.zip",
  "sha256": "el comando escribe aquí el SHA-256 real de 64 dígitos"
}
```

El texto anterior es ilustrativo; el generador escribe un hash válido. Publicá
primero el ZIP y después el manifiesto externo. El juego exige que el ZIP tenga
ese SHA-256 y que los metadatos de su manifiesto incluido coincidan. El hash
comprueba identidad e integridad del archivo, sin acreditar autoría o permisos.

## Importar desde el juego

1. Abrí **Biblioteca → Descargas del autor → Instalar desde URL**.
2. Pegá la URL HTTPS del `openrailsrs-package.json` publicado y pulsá
   **Comprobar enlace**. Se muestran autor declarado, versión, licencia,
   carpetas incluidas, dependencias y URL del ZIP antes de descargarlo.
3. Revisá la procedencia en la página del autor y confirmá el origen para
   instalar. La descarga, extracción y auditoría permiten cancelar.
4. Completá o vinculá las dependencias y reauditá la copia antes de iniciar.

También podés pegar la URL directa del ZIP. En ese caso el manifiesto incluido
se verifica después de descargarlo y el juego registra el SHA-256 obtenido.
La URL al manifiesto externo permite contrastarlo además con el hash publicado.
Los repositorios GitHub conservan su flujo actual y reconocen el manifiesto
cuando está en la raíz de su contenido; los paquetes anteriores del catálogo
siguen siendo compatibles.

```bash
openrailsrs content --inspect-url \
  --url https://tu-sitio.example/paquetes/ruta-ejemplo/openrailsrs-package.json

# Después de comprobar la procedencia y los permisos:
openrailsrs content --confirm-original \
  --url https://tu-sitio.example/paquetes/ruta-ejemplo/openrailsrs-package.json
```

Los enlaces de paquetes usan HTTPS público, sin credenciales, parámetros ni
fragmentos. Las redirecciones conservan el servidor de la URL solicitada.
Publicá archivos directos, accesibles sin iniciar sesión; para GitHub se puede
usar la raíz del repositorio. La descarga tiene un máximo de 8 GiB y la
extracción de 32 GiB. Un error conserva la edición anterior y descarta la copia
parcial. Los paquetes descargados nunca se ejecutan como instaladores.

## Dependencias locales y recursos comerciales

Un paquete público incluye únicamente recursos que su autor puede distribuir.
Los recursos de Microsoft requieren los permisos de sus condiciones de licencia;
alojar una copia en otro servidor no concede esos permisos.
[Condiciones de Microsoft](https://www.microsoft.com/en-us/legal/intellectualproperty/copyright/permissions).

Para declarar un recurso externo, agregá una dependencia con sus rutas nativas
exactas. Este ejemplo usa nombres ilustrativos que debés reemplazar por los del
contenido original:

```json
"dependencies": [
  {
    "id": "cabina-original",
    "name": "Cabina y sonidos de la instalación original",
    "files": [
      "TRAINS/TRAINSET/TrenOriginal/CABVIEW/original.cvf",
      "TRAINS/TRAINSET/TrenOriginal/SOUND/original.sms"
    ],
    "instructions": "Vinculá tu instalación original desde Biblioteca y reauditá este paquete."
  }
]
```

Cada dependencia declara entre 1 y 64 archivos relativos a `TRAINS`, `ROUTES` o
`GLOBAL`, con separadores `/` y sin `..` ni rutas absolutas. El juego comprueba
su presencia en el propio paquete y en las instalaciones que el usuario haya
vinculado expresamente. Un requisito ausente impide iniciar una actividad que
use esa ruta o formación. Las referencias reales de los vehículos se auditan
además para detectar archivos inválidos o incompletos.

`instructions` admite hasta 1024 caracteres. `source_url` es un campo opcional
con la página HTTPS del proveedor: informa el origen y nunca inicia una descarga
de dependencias ni ejecuta comandos. Las cabinas y sonidos comerciales del Mitre
se declaran con sus nombres reales dentro de `SERIES2000` y permanecen en la copia
local de su propietario.

Para vincular originales, abrí **Biblioteca → Copias y respaldos**, escribí o
explorá la raíz que contiene `TRAINS/TRAINSET` y pulsá **Vincular instalación
original**. Guardá los ajustes en **F10** para conservar el vínculo y reauditá
el paquete o la formación. Se admiten hasta ocho instalaciones compartidas. El
juego lee esas referencias sin copiar los recursos comerciales al paquete ni al
repositorio. En Snap, la carpeta debe ser accesible con los permisos de la aplicación.

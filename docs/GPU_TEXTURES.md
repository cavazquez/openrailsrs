# Texturas ACE, DDS y KTX2, caché y carga comprimida en GPU

DDS es un contenedor, no una garantía de compresión. Puede almacenar RGBA sin
comprimir o bloques BC/DXT junto a los mipmaps. La implementación preserva los
bloques DXT1, DXT3 y DXT5 que ya trae una ACE; no vuelve a comprimirlos ni cambia
sus colores, alfa o mipmaps. Las ACE estructuradas se exportan como DDS RGBA8:
esa conversión no reduce VRAM.

El visor carga automáticamente las ACE exteriores elegibles como BC1/BC2/BC3,
sin tener que convertir previamente toda la ruta. Consulta los formatos del
dispositivo Bevy. Si no hay soporte BC, usa los píxeles RGBA y mipmaps originales.
La cabina y las imágenes que necesitan modificación/lectura de píxeles conservan
RGBA. Las imágenes estáticas del escenario se liberan de RAM después de subir
a GPU. La simulación y el streaming siguen necesitando CPU.

El lector DDS también usa las capacidades reales del dispositivo y sRGB para
albedo. La alternativa CPU de DDS conserva todos los mipmaps DXT1/3/5 de una
textura 2D; BC7 y otros formatos necesitan soporte del dispositivo. La preparación
de terreno que modifica píxeles se ejecuta sobre RGBA, nunca sobre bloques BC.

## Conversión sin modificar los originales

Desde la raíz del repositorio, después de `./check.sh` o de compilar el workspace:

```bash
target/debug/openrailsrs textures-dds \
  "${CHILTERN_ROUTE:-$HOME/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern}/TEXTURES" \
  --out-dir tmp/chiltern-dds
```

También acepta una sola ACE. Recorre subdirectorios sin seguir enlaces simbólicos
y mantiene su estructura relativa. Reutiliza archivos DDS idénticos; rechaza
sobrescribir un DDS diferente. `dds-report.json` enumera procedencia, formato,
mips y tamaños de payload RGBA/DDS. Los tamaños comparan bytes de textura;
no equivalen a toda la VRAM del proceso ni a su RSS.

El directorio de exportación es una copia para herramientas/paquetes; el visor
no lo monta como caché automáticamente. El ahorro de las ACE DXT se aplica
directamente al cargarlas. No se borra ni se renombra ninguna ACE de la ruta.

Para comparar con la carga RGBA, en una sesión separada:

```bash
OPENRAILSRS_TEXTURE_UPLOAD=rgba ./scripts/run_chiltern_service.sh --direct
```

Compará la misma estación, pose, hora, clima y radio. Las texturas y recortes
deben mantenerse; F8 → Diagnóstico permite observar RSS y VRAM cuando están
disponibles. El archivo `.stream.json` de las capturas registra `texture_upload`.
No se promete un porcentaje de ahorro global: depende de cuántas ACE sean DXT,
de los materiales que necesiten RGBA y de los demás buffers del renderizador.

Referencias: [Bevy Image](https://docs.rs/bevy/latest/bevy/image/index.html),
[formato DDS y mipmaps de Microsoft](https://learn.microsoft.com/en-us/windows/win32/direct3ddds/dds-file-layout-for-textures).

## Medición en el contenido instalado

El 5 de octubre de 2026 se exportaron 12624 ACE de Chiltern: 3858 DXT1 y
8766 RGBA8, con un ahorro agregado de payload del 15,78% frente a RGBA.
La misma pose de Northolt Park, radio 900 m y RX 7600, pasó de 1064,4 MiB
de VRAM del proceso en RGBA a 808,2 MiB en BC (24,1% menos). El error RGB
medio del área de escenario fue 0,014/255. No se extrapola a otras escenas.
Las muestras/informe están en `tmp/cabin-dds-verification/`; la copia masiva
temporal se limpió para recuperar espacio.

[Registro verificable](fixtures/textures/2026-10-05.json).

## KTX2 nativo y caché sin pérdida

El resolver admite `.ktx2` para texturas de materiales de escenario, trenes y
terreno. Una referencia ACE busca primero ACE, después DDS y finalmente KTX2
en cada carpeta; conserva las prioridades originales de temporada y noche.
No reemplaza un archivo ACE existente por una variante diferente. Las rutas
de instrumentos que procesan píxeles ACE conservan su decodificación original.

Al cargar ACE/DDS elegibles, el visor escribe un derivado KTX2 en
`cache/textures-v1` dentro de la carpeta de datos del usuario. Conserva bloques
BC1/2/3 o RGBA8 exactos, alfa y todos los mipmaps, con Zstd por nivel.
**Zstd reduce almacenamiento; no reduce los bytes de textura subidos a GPU.**
No se realiza una nueva compresión con pérdida. El hash del original invalida
el derivado si el autor lo cambia; un derivado corrupto se reconstruye. La
caché se publica con archivos temporales y queda fuera de Git y del paquete.

```bash
# Primera ejecución: crea el derivado; segunda: informa cache_hit=true.
target/debug/openrailsrs textures-ktx2 "$CHILTERN_ROUTE/TEXTURES/oak25_1.ace"
target/debug/openrailsrs textures-ktx2 "$CHILTERN_ROUTE/TEXTURES/oak25_1.ace"
# Decodificación RGBA con la misma caché:
target/debug/openrailsrs textures-ktx2 "$CHILTERN_ROUTE/TEXTURES/oak25_1.ace" --rgba
```

También acepta carpetas y DDS/KTX2. `--cache-dir` permite una caché aislada para
medir; el JSON informa formato, mipmaps, bytes de carga y tiempo por textura.
Un KTX2 nativo se lee directamente, sin generar una copia derivada.

Para comparar toda una escena sin caché:

```bash
OPENRAILSRS_TEXTURE_CACHE=off ./scripts/run_chiltern_service.sh --direct
```

La alternativa CPU descomprime BC1/2/3 a RGBA conservando mipmaps y dimensiones;
otros bloques nativos requieren soporte del dispositivo. Bevy 0.19 permite
UASTC, pero no ETC1S/BasisLZ en este cargador. Los KTX2 sin formato Vulkan
explícito deben tener un descriptor UASTC válido. Se rechazan dimensiones
mayores a 8192, matrices/cubemaps, cadenas de mipmaps inválidas e inflación Zstd
fuera de los tamaños esperados antes de entregarlos al cargador de Bevy.

En una muestra de cuatro texturas reales de Chiltern, la carga fría sumó
43,58 ms y la caliente 17,06 ms; el payload permaneció en 4,17 MiB.
Es una muestra de texturas, **no una medición de aceleración del arranque completo**.
La [verificación de esta entrega](PLAYER_POLISH_QA.md) identifica las capturas
con GPU, las comprobaciones de originales y los límites visuales restantes.

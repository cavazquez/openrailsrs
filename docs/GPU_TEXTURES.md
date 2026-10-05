# Texturas DDS y carga comprimida en GPU

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

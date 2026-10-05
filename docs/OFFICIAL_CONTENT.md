# Contenido del catálogo oficial

En el menú principal, **Descargar contenido oficial** muestra autor, origen,
tamaños anunciados y tipo de distribución. El catálogo se conserva desde el
[repositorio oficial](https://github.com/openrails/content/tree/2bc71f58d04fbce693564db74d577c0ee00b7e31),
publicado por [Open Rails](https://www.openrails.org/download/content/).

## Descargar y jugar

1. Elegí **Demo Model 1** para una primera prueba: ZIP del servidor de Open Rails,
   unos 260 MiB de descarga y 315 MiB instalado. Otros paquetes pueden ser mucho
   mayores; el menú muestra los tamaños del catálogo.
2. Pulsá **Instalar y auditar**. La descarga y la preparación corren en segundo
   plano. El panel informa MiB, extracción y auditoría. Podés cancelar; los
   temporales se limpian y el contenido anterior se conserva.
3. Al terminar, volvés al menú y elegís la ruta instalada y una actividad.
   Se utiliza su red importada, escenario nativo, formación y recorrido. La
   selección de formación conserva los diagnósticos de modelos, texturas,
   cabinas, sonidos y sistemas antes de iniciar.
   En Demo Model 1, la formación push-pull original utiliza campos `Include`
   `.inc` todavía no expandidos por nuestro lector. La auditoría lo detecta;
   hay nueve alternativas con recursos completos y tracción, por ejemplo
   **MT SCE BlGr # Set 101 194**, para probar el escenario desde el exterior.
   Esas nueve formaciones son de tráfico AI y no incluyen cabina: la cámara
   del conductor muestra la vía sin panel. La cabina original del jugador
   requiere ampliar la lectura de `Include`.
4. Si cancelaste después de descargar, volvé a instalar el mismo paquete: se
   reutiliza esa copia para auditarla, sin descargar otra vez el archivo.

Cada paquete queda en `player-data/official-content/<id>-<hash>`; la ubicación
respeta `OPENRAILSRS_PLAYER_DIR`. Chiltern y los ejemplos existentes conservan
sus archivos y versiones. Los repositorios GitHub se resuelven a un commit
concreto del autor, registrado en el manifiesto. El enlace de Chiltern del
catálogo actualmente dirige a v4: se instala por separado del piloto fijado.

La copia conserva licencias y archivos originales. `openrailsrs-content.json`
registra catálogo, autor, URL, commit cuando corresponde, SHA-256 del ZIP y
tamaños. Ese hash identifica la descarga; no es una firma del autor.
`openrailsrs-audit.json` contiene el resultado por formación y
`openrailsrs-prepared.json` enumera las rutas/actividades preparadas localmente.

## Comandos

```bash
target/debug/openrailsrs content --list
target/debug/openrailsrs content --package demo-model-1
# Auditar/importar una descarga hecha por CLI, sin ventana:
target/debug/openrailsrs-prepare-content /ruta/al/paquete-instalado
```

`OPENRAILSRS_PYTHON` permite elegir Python 3. El helper está en `scripts/` y se
distribuye junto al proyecto. La CLI instala; la preparación se hace en el
menú o con `openrailsrs-prepare-content`. Los ZIP gratuitos directos y repositorios GitHub
del catálogo tienen instalación automática. **Ver catálogo oficial** abre
la página para los paquetes que usan una web, instalador o distribución comercial.

## Límites y validación

La descarga admite hasta 8 GiB y la extracción hasta 32 GiB, con comprobaciones
de espacio libre. Se instala en una carpeta nueva mediante publicación atómica.
La extracción rechaza rutas que escapan, enlaces, nombres ambiguos por
mayúsculas, archivos especiales, paquetes cifrados y metadatos reservados al
preparador. Cada redirección se verifica **antes** de emitir la siguiente
petición: solamente HTTPS y proveedores del catálogo. Los manifiestos locales
usan rutas canónicas confinadas al paquete, también al encontrar enlaces.

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

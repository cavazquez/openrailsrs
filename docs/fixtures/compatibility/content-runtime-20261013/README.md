# Biblioteca, contenido por URL y Blue Pullman en Snap

Pruebas del 13 de octubre de 2026 para #373, #412 y #413. La identidad del
artefacto y las fuentes, URLs originales, fechas, tamaños y resultados se
registran en [receipt.json](receipt.json). El artefacto registrado es de QA con
cambios sin commit; no se publicó en la tienda. Una entrega final necesita su
propio build y comprobación.

## Qué se comprobó

- **Carpeta de contenido:** Pegar → Explorar carpeta → Usar esta carpeta mediante
  clics reales, tanto en Snap estricto como en el paquete portátil trasladado
  fuera del repositorio. Reiniciar conserva la elección y los paquetes anteriores.
  Una carpeta de sólo lectura conserva la preferencia válida. Las rutas largas
  de instalaciones compartidas se muestran debajo del botón y se ajustan al ancho.
- **Actualización de Snap:** el cambio de revisiones de QA conserva el manifiesto
  y hash de Demo Model 1, la ubicación elegida y los trenes instalados en ella.
  `removable-media` permaneció desconectado; se probaron destinos accesibles en
  el perfil del usuario, sin ampliar permisos.
- **ZIP original:** la URL de Demo Model 1 reconoce y reutiliza la copia existente.
- **Repositorio y trenes sin ruta:** Biblioteca acepta la URL pública
  `https://github.com/Jeanprisca/MAJ`, muestra cuenta/revisión/fecha y pide confirmar
  el origen fuera del catálogo. Descarga, extrae, prepara y audita en segundo plano.
  Se conservan las formaciones originales y aparecen opciones individuales.
  El paquete es una actualización del TGV: sus 198 formaciones auditadas aún
  requieren recursos de la base. Aparecen seleccionables con faltantes, y Jugar
  no los presenta como resueltos. El portátil reproduce la descarga y preparación,
  reutiliza la identidad y muestra las formaciones después de reiniciar.
- **Falta de espacio real:** el paquete original `Jeanprisca/SNCF-TGV-Atlantique`
  llegó a 1164,5 MiB de descarga y fue rechazado antes de descomprimir porque el
  disco de QA no tenía espacio suficiente. Se limpió su staging, no se registró
  una instalación y los paquetes anteriores conservaron sus manifiestos.
- **URLs inválidas:** se rechazan HTTP/local, `file:`, páginas informativas,
  enlaces a un archivo de GitHub y repositorios fuera del catálogo sin confirmar.
  Las pruebas automatizadas cubren también pérdida de red, cancelación, ZIP
  inválido y escapes de extracción sin alterar una edición anterior.

## Blue Pullman preparado

En un Snap sin `--route-root`, el ejemplo de seis estaciones mantiene su servicio
original y sus dos servicios de tráfico. Los ocho modelos del tren se resuelven
desde los recursos compartidos de los ejemplos; inicialmente sólo falta la cabina
`PULLMAN_GR.s`. El detalle identifica el archivo, ofrece el origen de DocMartin y
explica que `/snap` es de sólo lectura.

Se preparó dentro del confinamiento el paquete original Chiltern v4 ya instalado,
con su payload en una vista de sólo lectura: 153 de 188 formaciones con recursos
completos y tracción, 34 actividades. Reiniciar habilita Jugar para el mismo
Blue Pullman preparado y recupera los metadatos originales de infraestructura.
El contador de oráculos sigue siendo independiente de la disponibilidad de los
archivos. Las capturas se conservan como evidencia local de QA, fuera del paquete.

## Repetir a mano

1. Abrí **Biblioteca → Copias y respaldos**. Pegá una carpeta existente y escribible,
   pulsá **Explorar carpeta** y **Usar esta carpeta**. Reiniciá: debe seguir elegida.
   La biblioteca anterior debe continuar visible. Intentá elegir una carpeta sin
   escritura: debe explicar el fallo y conservar la elección anterior.
2. Abrí **Descargas del autor**. Pegá una URL original y pulsá **Comprobar enlace**.
   Revisá origen, fecha/revisión y destino. Un repositorio fuera del catálogo pide
   confirmar que es del autor. No interpreta esa confirmación como paridad.
3. Instalá el paquete y observá descarga, extracción, preparación y auditoría.
   Al terminar buscá su ruta o formación. Si faltan archivos, el detalle debe
   indicar los nombres y destinos escribibles. Una formación incompleta puede
   seleccionarse, pero no debe iniciar como si todos sus recursos estuvieran.
4. Volvé a usar la misma URL: la edición idéntica se reutiliza. Actualizar el Snap
   debe conservar la carpeta y los paquetes en `common`.
5. Para Chiltern, instalá/prepará la edición original de DocMartin. Elegí
   **Chiltern extendido: Northolt Park → Gerrards Cross** y **birmingham_pullman**.
   Jugar debe habilitarse al resolver la cabina original. Probá **1** para cabina
   y **2** para exterior: deben usar el mismo tren y escenario que la auditoría.

## Automatización

```sh
python3 scripts/test_content_url.py
python3 scripts/test_official_content.py
python3 scripts/test_packaging.py
```

El instalador y la GUI conservan estructura, licencias y procedencia del autor;
no ejecutan instaladores ni descargan dependencias desde espejos. `BUILD.json`
registra los hashes de binarios y la versión mínima de glibc. El árbol preparado
por `package_snap.py` transmite commit y hash de fuentes al build sin Git, y el
empaquetado rechaza fuentes cambiadas después de preparar ese árbol.

## Partida después de corregir el presupuesto de publicación

El intento de cabina con espera de escena completa agotó 550 segundos: el
indicador permaneció en 100% (908/908). Se publicaron las 908 formas, la cabina
original tenía 69/69 partes visibles y el ojo estaba dentro de su geometría;
no había shaders fallidos/pendientes ni cargas pendientes a la GPU. Sin embargo,
se registraron cero cuadros de partida y la pantalla de carga seguía cubriendo
el mundo. Esos datos no completan el criterio de captura de cabina y exterior
para #412. La cola ahora publica al menos un trabajo antes de ceder cuando el presupuesto
se consume. La revisión de QA x14 completó la carga y produjo una captura nueva
de cabina, con 160 cuadros de partida, 69/69 piezas visibles, ocho coches,
66 programas SIGSCR y ninguna carga o shader pendiente. Su hash y el del Snap
se registran por separado del primer artefacto de biblioteca. Se ejecutó en
llvmpipe: sus tiempos no representan rendimiento con GPU. El harness tuvo una
carrera con el cierre normal de la ventana; se comprobó la imagen nueva y el
estado Playing independientemente y se corrigió esa espera en el harness.
El intento exterior llegó a 160 cuadros de partida y cero shaders/cargas
pendientes, pero salió tras 2,5 s sin escribir el PNG en el renderizador por
software. El harness rechazó ese intento. La captura espera ahora el evento de
readback de Bevy y una escritura exitosa, con fallo explícito por error de disco
o timeout; un archivo anterior no cuenta como éxito. La entrega final requiere
repetir ambos modos con esa corrección.
No se acepta una pantalla de carga como prueba visual del resultado.

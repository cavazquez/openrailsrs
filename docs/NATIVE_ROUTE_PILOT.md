# Preparar un servicio nativo de tres estaciones

`scripts/prepare_native_pilot.py` conserva TDB, trayectoria PAT, formación CON,
plataformas y programas SIGSCR originales. No cambia ancho de vía, sustituye
edificios ni inventa estaciones. Recorre los enlaces `nextMain` del PAT, valida
continuidad/dirección sobre el grafo importado y usa el extremo de salida real
de cada plataforma. Audita la formación y escribe los hashes de los archivos fuente.

```bash
python3 scripts/prepare_native_pilot.py --route-root /ruta/ROUTES/BelgranoCC --inspect
python3 scripts/prepare_native_pilot.py --route-root /ruta/ROUTES/BelgranoCC --list
# Primero importar el grafo físico:
target/debug/openrailsrs import-msts /ruta/ROUTES/BelgranoCC --out-dir tmp/belgrano-import
python3 scripts/prepare_native_pilot.py \
  --route-root /ruta/ROUTES/BelgranoCC \
  --imported-track tmp/belgrano-import/track.toml \
  --path /ruta/ROUTES/BelgranoCC/PATHS/recorrido.pat \
  --consist /ruta/TRAINS/CONSISTS/formacion.con \
  --traffic-consist /ruta/TRAINS/CONSISTS/otra-formacion.con \
  --out-dir examples/belgrano_cc
target/debug/openrailsrs play-service examples/belgrano_cc/scenario.toml --out-dir tmp/belgrano-service
```

`--inspect` no descarga ni modifica archivos. Informa las carpetas absolutas de
la ruta y de `TRAINS`, el TDB, los PAT/CON disponibles y las ubicaciones de
SIGCFG/SIGSCR ausentes. Cuando no se conoce un nombre real, utiliza un marcador
`<nombre-original>`; no inventa el recurso ni lo busca en fuentes alternativas.
Usá ese diagnóstico para completar la estructura con los archivos del autor.
Una instalación lista para seleccionar archivos todavía debe superar las
auditorías del preparador y la prueba de viaje.

`--origin` elige una estación real del PAT. Se necesitan tres estaciones consecutivas,
un PAT sin inversiones y espacio seguro para el servicio adelantado opcional.
El destino debe ser una carpeta nueva/vacía; el contenido instalado es de solo lectura.
`native-content.json` permite que el menú descubra el piloto y cargue **su propia**
carpeta de escenario y material rodante. No lo mezcla con el terreno de Chiltern.

Estado de Belgrano CC: la ruta publicada por el autor representa Retiro–Villa Rosa
en los años sesenta. [Página de descarga del autor](https://jorgeluisgonzalezlopez.jimdofree.com/descarga-rutas-hist%C3%B3ricas-p2/).
Los enlaces de 4shared devolvieron HTML de acceso, no el RAR; no hay archivos nativos
Belgrano instalados en este equipo. Por eso quedan pendientes la importación real,
auditoría del material argentino y las capturas comparables en OR 1.6.1.
El preparador se valida con el PAT y la formación nativos Chiltern, sin presentarlos
como una ruta argentina. La comparación visual del piloto es un paso posterior
cuando estén disponibles los archivos de la ruta y sus complementos originales.

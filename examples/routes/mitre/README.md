# Línea Mitre — Buenos Aires, Argentina

Topología ferroviaria importada automáticamente desde OpenStreetMap con:

```bash
# Descarga de Overpass API (bbox: Retiro → ramal Tigre y Bartolomé Mitre)
curl "https://overpass-api.de/api/interpreter" \
  --data-urlencode 'data=[out:json][timeout:30];
    (way[railway~"^(rail|light_rail)$"](-34.60,-58.60,-34.43,-58.36);node(w););out body;' \
  -o mitre.json

# Importación
openrailsrs import-osm mitre.json \
  --out examples/routes/mitre/track.toml \
  --route-id mitre \
  --default-speed 90
```

**Resultado:** 1 757 nodos · 2 028 aristas · 172 estaciones  
Incluye: Retiro, Palermo, Belgrano R/C, Núñez, Olivos, Martínez, San Isidro, Tigre, Florida, etc.

## Nota

`consists/caf_6000.con` y sus ENG/WAG son datos sintéticos para pruebas de
física. No incluyen modelos, texturas ni cabina: sus referencias CAF son
marcadores y el menú impide iniciar si esos recursos obligatorios faltan.
No representan el material rodante real del Mitre. El menú muestra el escenario,
los archivos faltantes y sus destinos absolutos; no ofrece un modelo sustitutivo
porque no se identificó un repositorio original para esas referencias.
Para material real obtenido de su origen, conservá los nombres y carpetas del
autor y elegí su `.con` original desde la
[biblioteca del usuario](../../../docs/OFFICIAL_CONTENT.md#recursos-faltantes-y-caf-6000).

La Línea Mitre tiene una topología muy granular en OSM (cada aguja y bypass es un way
separado). Para armar un escenario de simulación útil conviene:

1. Identificar los nodos de origen/destino con `grep "name = " track.toml`.
2. Usar `openrailsrs graph examples/routes/mitre --out mitre.dot` para ver el grafo.
3. Definir un `scenario.toml` con `start` y `destination` que sean IDs de nodos
   (p. ej. `"n2848475776"` para Retiro y el nodo correspondiente a Tigre).

El simulador navega el grafo con BFS; la red completa funciona, aunque un `scenario.toml`
apuntando a un subgrafo simplificado dará resultados más claros.

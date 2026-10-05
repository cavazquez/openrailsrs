# Formación física original de Chiltern

Estos archivos son fixtures numéricos extraídos de la formación Birmingham
Pullman del autor: ocho vehículos, 440906,4 kg y 166,164 m. No contienen
texturas, modelos, cabinas ni sonidos descargados. Las instalaciones completas
permanecen fuera de Git y se gestionan desde la biblioteca del jugador.

`provenance.json` registra SHA-256 de cada fuente original y de su extracción.
Para reproducirla desde el paquete original:

```bash
python3 scripts/prepare_chiltern_physics.py --content /ruta/al/Content/Chiltern
```

El servicio completo usa estos datos. Los cuatro oráculos cortos conservan sus
formaciones históricas y sus referencias. La simulación reconoce EP por vehículo,
gobernadores separados de los dos motores, regulador con escalones, corte de
tracción por presión real, patinaje Pacha, resistencia con temperatura de
rodamientos y conexiones rígidas declaradas por el material rodante.

`environment.json` documenta entradas del ensayo congelado: Open Rails quedó
con `Paused = true` aunque el cliente avanzaba su simulación. Por eso su factor
inicial de adherencia es 0,5 y cambia a 0,8 al primer sector resbaladizo; el clima
visible del jugador no se selecciona con estos datos. El modelo usa la media del
ruido de adherencia, no una réplica de toda la secuencia aleatoria original.
La comparación sigue exigiendo RMS ≤0,75 m/s, pico ≤2 m/s y distancia ≤45 m.

Para vehículos originales que declaran estos subsistemas se activa el modelo
nativo automáticamente. Sin una entrada explícita de entorno, parte de 20 °C y
adherencia de vía despejada; la nieve visual todavía no simula adhesión por hielo.
Pasar este ensayo no certifica todos los vehículos, climas, scripts ni servicios.

# Tres estaciones, cabina y exterior — OR 1.6.1

Seis capturas nativas a las 09:55 de verano, ventana 1280×720 y distancia
de dibujo de 450 m. La órbita exterior tiene yaw 1,6, pitch 0,6 y radio 160 m;
la cabina conserva sus posición, dirección e instrumentos originales, FOV 45°.
Los JSON registran matrices, posición del tren, sol, tiles WORLD y SHA-256.
Los binarios conservan los hashes de `oracles/openrails-reference.toml`.

`SceneryCapture.cs` congela la actividad y desplaza su traveller nativo 0,
3868,22559 y 7058,02269 m. Así la cabina y toda la formación están en la estación
observada. No se usa esta reubicación como comparación física. El cliente
deshabilita únicamente los contadores Host que Wine no implementa.

Las DLL originales y las referencias anteriores no se modificaron. El Content
instalado carece de tres nombres de modelos Pullman y de algunas texturas de
palancas; el render original registra esos faltantes. No deben copiarse como
defectos obligatorios de Bevy.

Para generar las seis vistas Bevy correspondientes, una por vez:

```bash
python3 scripts/capture_route_views.py --route-root "$CHILTERN_ROUTE" \
  --with-cab --cab-fov-deg 45 --software --out-dir tmp/station-materials
```

Se comparan andenes, edificios, cercos, árboles y contornos transparentes. La
posición de estaciones conserva la aceptación espacial nativa de 3 m;
composición y LOD de casas conservan su oráculo exacto y ±1 mm. La silueta de
instancias GPU mantiene ≥98 %. Las diferencias de sol, tonemapping, trenes
ausentes y encuadre impiden usar un umbral global de píxeles entre motores.
Las imágenes Bevy son evidencia de inspección, no reemplazos de estas referencias.

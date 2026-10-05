# Seis estaciones, cabina y exterior — OR 1.6.1

Doce capturas nativas a las 09:55 de verano, ventana 1280×720 y distancia
de dibujo de 450 m. La órbita exterior tiene yaw 1,6, pitch 0,6 y radio 160 m;
la cabina conserva sus posición, dirección e instrumentos originales, FOV 45°.
Los JSON registran matrices, posición del tren, sol, tiles WORLD y SHA-256.
Los binarios conservan los hashes de `oracles/openrails-reference.toml`.

`SceneryCapture.cs` conserva las seis vistas anteriores: congela la actividad y
desplaza su traveller nativo 0, 3868,22559 y 7058,02269 m. Su archivo y hash
permanecen intactos. Esa actividad termina en West Ruislip; avanzar más allá de
su PAT no coloca correctamente el tren en las estaciones extendidas.

El nuevo cliente `ExtendedSceneryCapture.cs` sitúa toda la formación con el
traveller nativo sobre los vectores anfitriones de las plataformas **1864**
(Denham), **1299** (Denham Golf Course) y **1303** (Gerrards Cross), en sentido
Backward. Espera a que el loader publique los WORLD de la nueva cámara. Es una
reubicación para inspección visual; no constituye una comparación física ni de
reservas/señales fuera del PAT. Ambos clientes deshabilitan únicamente los
contadores Host que Wine no implementa.

Las DLL originales y las referencias anteriores no se modificaron. El Content
instalado carece de tres nombres de modelos Pullman y de algunas texturas de
palancas; el render original registra esos faltantes. No deben copiarse como
defectos obligatorios de Bevy.

Para generar las doce vistas Bevy correspondientes, una por vez:

```bash
python3 scripts/capture_route_views.py --route-root "$CHILTERN_ROUTE" \
  --with-cab --capture-or-focus --cab-fov-deg 45 --headless-wayland --require-hardware --out-dir tmp/station-materials
```

Se comparan andenes, edificios, cercos, árboles y contornos transparentes. La
posición de estaciones conserva la aceptación espacial nativa de 3 m;
composición y LOD de casas conservan su oráculo exacto y ±1 mm. La silueta de
instancias GPU mantiene ≥98 %. Las diferencias de sol, tonemapping, trenes
ausentes y encuadre impiden usar un umbral global de píxeles entre motores.
Las imágenes Bevy son evidencia de inspección, no reemplazos de estas referencias.


Las seis vistas adicionales se capturaron el 4 de octubre de 2026 con las mismas
DLL verificadas antes y después. El pico de RSS fue de **4047–4090 MiB**, con
límite de 6144 MiB y un visor a la vez. Cada JSON incluye plataforma, hash del
PNG y hash del nuevo cliente. Las seis vistas anteriores se conservaron sin
cambios. El recorte negro/terreno distante de algunos exteriores pertenece al
render nativo con alcance de 450 m; no se exige reproducirlo en Bevy.

La captura nativa es reproducible con
`scripts/capture_openrails_station.py --prefix ... --runtime ... --activity ...
--label denham-exterior --platform-id 1864 --view exterior`; `--view cab` toma la
cabina original. El script verifica los hashes antes y después, compila solamente
el nuevo cliente y usa un escritorio privado. Necesita una instalación/prefix y
actividad ya disponibles; no instala ni descarga el programa.

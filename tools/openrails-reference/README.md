# Consumidores de Open Rails 1.6.1

Estos programas C# usan las DLL originales de la instalación fijada en
`oracles/openrails-reference.toml`; no se compilan cambios en Open Rails.
`HeadlessCapture.cs` se ejecuta mediante `scripts/capture_chiltern_service_or.py`.

`InspectShapes.cs` serializa el objeto `ShapeFile` original para generar la
referencia de composición. Compilar en una copia privada de la instalación:

```text
csc /nologo /out:InspectShapes.exe /r:Orts.Formats.Msts.dll /r:Newtonsoft.Json.dll InspectShapes.cs
InspectShapes.exe <salida.json> <ruta-a-shape.s> [otras-shapes.s]
```

La salida JSON original permite extraer primitivas, estados y distancias LOD
sin usar nuestro importador como fuente del resultado esperado. Antes de
recapturar, verificar los hashes de DLL y de los modelos.

`SceneryCapture.cs` fija una órbita exterior o la cabina 3D nativa, pausa la
actividad y escribe la pose de la cámara al completar 30 frames. Compilar junto
a la copia de DLL, incluyendo las referencias:

```text
csc /nologo /out:SceneryCapture.exe /r:RunActivity.exe /r:Orts.Settings.dll /r:Orts.Simulation.dll /r:Orts.Common.dll /r:MonoGame.Framework.dll /r:Newtonsoft.Json.dll /r:System.Windows.Forms.dll SceneryCapture.cs
```

Definir `OPENRAILS_REFERENCE_VIEW=exterior` o `cab`, y
`OPENRAILS_REFERENCE_METADATA` con un archivo nuevo. Ejecutar con
`-start <actividad.act> -skip-user-settings -FullScreen=true
-WindowSize=1280x720 -ViewingDistance=450 -DistantMountains=false
-AntiAliasing=1 -SoundVolumePercent=0 -PauseOnFocusLost=false`.
Guardar la imagen una vez registrado `Native scenery capture ready`. El
consumidor espera la ventana WORLD del tile de cámara antes de contar los
30 frames y registra los tiles cargados y la dirección solar nativa.

Para una vista exterior de otra estación, definir opcionalmente
`OPENRAILS_REFERENCE_TARGET=tile_x,tile_z,x,y,z` con coordenadas locales MSTS
y separador decimal punto. El consumidor registra ese objetivo junto con
las matrices de cámara; omitirlo mantiene el foco en el tren de la actividad.
Este cambio mueve la cámara y la ventana de escenario; el tren permanece en
la estación inicial. Las vistas remotas sirven para escenario y edificios.

La órbita admite `OPENRAILS_REFERENCE_CAM_YAW`,
`OPENRAILS_REFERENCE_CAM_PITCH` (radianes) y `OPENRAILS_REFERENCE_CAM_DIST`
(metros), con valores predeterminados 1,6 / 0,6 / 160. Los valores resueltos
quedan en los metadatos. La referencia baja de Northolt utiliza pitch −0,15
para comprobar los reversos abiertos del contenido original.

En Wine, usar el prefijo privado generado por el capturador de servicio y
`WINEDLLOVERRIDES=d3d11,dxgi=b` si DXVK no puede abrir el adaptador. El Host de
estadísticas se detiene para evitar la función PDH ausente en Wine; las tareas
de simulación, carga y render siguen siendo las originales. Conservar la
captura y sus hashes y revisarla antes de modificar el pin de referencia.

`SolarReference.cs` ejecuta `WorldLatLon.ConvertWTC`, `SkyDate` y
`SunMoonPos.SolarAngle` de las DLL originales, incluyendo el lector `.env`.
No inicia un render ni un bucle de simulación. Compilar y ejecutar en la misma
copia privada de la instalación:

```text
csc /nologo /out:SolarReference.exe /r:Orts.Common.dll /r:Orts.Formats.Msts.dll /r:MonoGame.Framework.dll /r:Newtonsoft.Json.dll SolarReference.cs
SolarReference.exe <solar-samples.json> <salida-nueva.json> <ruta-original-Chiltern>
```

`solar-samples.json` incluye las tres estaciones, hemisferio sur, Norteamérica
y discontinuidades de la proyección, con tres estaciones del año y cinco horas.
La referencia `oracles/solar-or161.json` conserva 75 vectores válidos y 15
discontinuidades, hashes del consumidor, entradas, DLL y `SummerClear.env`.
Los tests Rust exigen error geográfico ≤1e−11 rad y error L2 solar <1e−5.
La fixture congela las salidas de las DLL; no se genera desde nuestras funciones.
El visor evalúa directamente la ecuación cada segundo de simulación,
conservando esa mejora respecto de la tabla nativa, que interpola muestras
tomadas cada 20 minutos.

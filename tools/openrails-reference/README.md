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

`SignalReference.cs`, `CabReference.cs` y `ParserReference.cs` consumen las mismas
DLL originales sin recompilar OR. Sus resultados están congelados en
`oracles/openrails-signals.json`, `openrails-cab.json` y `openrails-parser.json`,
con manifiestos de DLL, consumidor y salida. La captura de cabina fija los
valores predeterminados de UnitsScale y Exponent en 1, como el lector original;
un amperímetro de referencia nulo ante esfuerzo positivo se rechaza.

```bash
python3 scripts/capture_signal_reference.py --out-dir tmp/senales-or-a
python3 scripts/capture_cab_reference.py --out-dir tmp/cabina-or-a
python3 scripts/capture_parser_reference.py --out-dir tmp/parser-or-a
```

Repetir cada captura en un segundo directorio nuevo y comparar SHA-256 antes de
promoverla. Cada ejecución copia las DLL a una carpeta privada y utiliza un
prefijo Wine separado; no escribe la instalación, el prefijo ni el contenido
originales. Las referencias numéricas no incluyen rutas locales del usuario.
El lector STF contrasta Includes, cierre de bloques al EOF, comillas tardías y
banderas PAT hexadecimales. Los contextos sintéticos de señal/cabina validan las
funciones aisladas; no certifican una actividad entera ni un modelo completo.

`ActivityParameters.cs` carga un ACT original con el simulador y lector de OR,
usando el cargador de `HeadlessCapture.cs`. Registra vehículos, potencia, freno
máximo, controlador, horarios y orientación nativa de carteles. La referencia
LIRR conserva 778 carteles, 776 con velocidad positiva. No es una traza del viaje
ni una certificación de respuesta neumática de todas las formaciones.

```bash
python3 scripts/capture_activity_parameters.py \
  --activity /ruta/ROUTES/LIRR/ACTIVITIES/01Brooklyn-Hempstead.act \
  --out-dir tmp/lirr-or-a
```

Repetir en otro directorio nuevo y comparar hashes. En esa actividad OR conserva
Duration = 1560 s como estimación y llegada a Hempstead = 1575 s. El servicio puede
registrar retrasos; Duration no impone un fallo automático.

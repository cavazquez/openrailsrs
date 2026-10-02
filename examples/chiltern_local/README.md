# Servicio Chiltern local

Partida completa de Northolt Park a West Ruislip, con parada en South Ruislip,
la formación Birmingham Pullman de ocho coches y el mapa original Chiltern.
La cabina CVF/ORTS y los modelos exteriores se resuelven desde la formación
compartida en `../chiltern/consists`, sin duplicar el Content.

El recorrido utiliza `RS_Maryleb-WRuislip0955.pat`, cuyos primeros puntos están
en Northolt Park aunque el nombre mencione Marylebone. Sus puntos 0, 3 y 6
definen las tres estaciones. La longitud de servicio es 6890,28 m; el odómetro
arranca en cero y la geometría conserva 264,48 m de vía antes de la cabeza.
`provenance.json` identifica PAT, TDB y catálogos tsection mediante SHA-256.

El horario de esta partida se ha creado aquí: salida de Northolt después de
20 s, llegada a South Ruislip a los 360 s y a West Ruislip a los 720 s, con
30 s de embarque en ambas. Se cuentan únicamente las paradas servidas.
Pasar de largo más de 10 m o rebasar una señal de parada termina el servicio
como fallido. El resultado final muestra las paradas y penalizaciones.

```bash
./scripts/run_chiltern_service.sh
./scripts/run_chiltern_service.sh --autodrive --cab
target/debug/openrailsrs play-service examples/chiltern_local/scenario.toml --out-dir tmp/service
```

Controles: `1` cabina, `Alt+1` 2D/3D, `2` exterior, `W/S` inversor, `A/D`
regulador, `;/'` freno de tren, `Backspace` emergencia, `Q` puertas,
`Space` bocina, `V` limpiaparabrisas, `Pause` pausa, `R` reiniciar.
`F5` muestra conducción y presiones; `F4` muestra próxima estación y señal;
`F3` habilita información de depuración; `C` abre el instrumental digital
opcional sin ocultar por defecto los instrumentos de la cabina original.
El audio conserva la configuración
silenciosa existente del proyecto.

Para regenerar el escenario desde la misma instalación:

```bash
python3 scripts/prepare_chiltern_service.py --route-root "$CHILTERN_ROUTE"
```

El generador utiliza las longitudes físicas de tsection y evita interpretar
postes kilométricos, advertencias y señales de fin de restricción como límites
de velocidad. Las velocidades se limitan a 65 km/h para este servicio local.
Las señales usan un script explícito de ocupación de tres aspectos para una
formación; todavía no se ha traducido el SIGSCR completo de Chiltern.
El selector de marcha y el corte en neutro funcionan; las maniobras de
retroceso quedan pendientes en el modelo de física de recorrido dirigido.

Las capturas OR congeladas verifican aceleración, frenado y costa; todavía
no existe una captura OR de esta partida completa ni un certificado de
paridad visual de todas sus estaciones. La prueba de servicio valida su
ejecución completa y su independencia de la frecuencia de renderizado.

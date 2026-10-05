# Hora y clima actuales del lugar

El jugador elige **Hora visual** y **Origen del clima** por separado, en el menú
inicial o en F10 durante la partida. Ambos empiezan en **Elegido por el jugador**.
El modo **Actual del lugar** es opcional. F10 → **Elegir clima** vuelve al modo
manual y permite despejado, lluvia, niebla, nieve, nublado o tormenta.

## Hora visual y horario del servicio

La hora real corresponde a la ubicación de la ruta, con fecha actual y zona IANA
(incluido el horario de verano). Chiltern usa Europe/London: no toma la hora de
Argentina por estar ejecutándose allí. UTC procede del reloj del equipo; debe
estar correctamente sincronizado. El sol usa fecha, latitud y longitud reales,
sin aplicar los amaneceres estacionales `.env` del modo manual. Cabinas,
iluminación y estrellas siguen esa altura solar.

El reloj del servicio conserva la hora de salida y el tiempo de simulación.
Cambiar la hora visual no cambia pasajeros, salidas, tráfico, puntuación ni
física. Pausar o acelerar la partida no pausa ni acelera el reloj real. La
estación del año y sus texturas continúan siendo una elección del jugador.

## Datos meteorológicos

[Open-Meteo](https://open-meteo.com/) proporciona **condiciones actuales estimadas
por modelos**, basadas en intervalos de 15 minutos. No son una medición en el
tren ni una transmisión de rayos observados. Se consulta la ubicación inicial
de la ruta cada diez minutos; el viaje corto de Chiltern utiliza ese mismo punto.

Los códigos WMO determinan despejado/nublado, niebla, lluvia, nieve o tormenta.
La cobertura nubosa controla el cielo; precipitación y nieve ajustan la cantidad
de partículas; el viento modifica su deriva sin desplazamientos bruscos al
actualizarse. La temperatura se informa en F8, sin modelar hielo ni adherencia.
Los códigos con granizo se representan como tormenta con lluvia, sin granizo
físico. Los rayos de la partida son eventos procedurales de esa tormenta.

Se envían únicamente las coordenadas públicas de la ruta al proveedor. La zona
horaria se resuelve allí una vez y se conserva en caché. Hora real sin clima real
puede necesitar una primera consulta para resolver la zona; luego funciona sin
conexión con esa zona. Con ambos selectores manuales no se inicia ninguna consulta.

La consulta HTTPS se ejecuta fuera del cuadro Bevy, con límite total de diez
segundos, respuesta de hasta 64 KiB, una petición pendiente y reintentos de
60/120/240/480 segundos (luego mantiene 480 s). No bloquea carga, cámara ni física. Se rechazan datos
incompletos, códigos desconocidos, valores inválidos, otras ubicaciones y fechas
vencidas o futuras. La caché está separada por coordenadas en
`player-data/environment/` (o `OPENRAILSRS_PLAYER_DIR`).

Si falla la conexión, conserva el último clima válido hasta dos horas desde la
fecha del dato y muestra su antigüedad. Después usa **Clima manual / respaldo**.
La hora real continúa si ya conoce la zona. Un aviso en pantalla y **F8 → Clima**
informan consulta, error, zona, coordenadas, fecha del dato y fuente. Elegir
manual impide que una respuesta pendiente cambie el clima del jugador.

Datos de Open-Meteo, adaptados a efectos visuales de openrailsrs, bajo
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
La API gratuita está destinada a [uso no comercial](https://open-meteo.com/en/terms);
una distribución comercial necesita revisar el servicio contratado.
[Variables, códigos WMO y zona automática](https://open-meteo.com/en/docs).
[Cálculo solar UTC de NOAA](https://gml.noaa.gov/grad/solcalc/solareqns.PDF).

## Tormentas

**Tormenta** está disponible incluso sin internet. Incluye lluvia, superficies
mojadas, parabrisas, cielo cubierto, rayos ramificados con destellos breves y
truenos procedurales. Los rayos se fijan en el mundo, no siguen la cámara ni
proyectan sombras. Un primer evento llega unos cuatro segundos de simulación
después de activar la tormenta; los siguientes se espacian entre 25 y 55 s.

El trueno llega después de la luz según distancia / 343 m/s, se atenúa con la
distancia y dentro de la cabina, y respeta volumen, silencio y pausa. Rayos y
truenos usan el reloj de simulación. F10 → **Rayos y destellos** apaga el efecto
visual; el sonido se controla con los ajustes de audio. Se mantiene como máximo
un rayo visible, dos eventos de trueno pendientes y dos voces de audio.

## Contenido sin ubicación

Las rutas nativas usan sus coordenadas WORLD/MSTS. Un circuito sintético sin
ubicación no se geolocaliza por su origen: informa la limitación y mantiene el
respaldo manual. Para dar ubicación a una ruta importada, su directorio de
escenografía (ROUTES/... en contenido nativo) puede contener `route-location.json`:

```json
{"latitude": -34.6037, "longitude": -58.3816, "timezone": "America/Argentina/Buenos_Aires"}
```

Son coordenadas WGS84 en grados. `timezone` es opcional; con una zona explícita
válida, la hora real no necesita resolverla por internet. La ubicación elegida
se guarda con la caché; no modifica vías ni el trazado. En el menú, preferencias
se conservan con **Guardar ajustes**. Las partidas guardadas conservan sus dos
selectores y clima manual; los guardados antiguos siguen siendo legibles.

## CLI y verificación

Las ejecuciones directas ignoran preferencias en vivo guardadas para conservar
capturas y oráculos reproducibles. Requieren estas opciones explícitas:

```bash
OPENRAILSRS_REAL_TIME=1 OPENRAILSRS_REAL_WEATHER=1 \
  ./scripts/run_chiltern_service.sh --direct

# Tormenta manual, sin consultas meteorológicas.
OPENRAILSRS_WEATHER=storm ./scripts/run_chiltern_service.sh --direct
```

`check.sh` verifica los casos offline, zonas/DST, guardado antiguo/nuevo,
selección manual, códigos WMO, geometría y audio. La prueba opcional del proveedor
requiere internet:

```bash
cargo test --locked --workspace --all-features public_provider_returns_timestamp_and_iana_timezone \
  -- --ignored --nocapture --test-threads=1
```

La prueba visual opcional ejecuta un renderer por vez y exige escenarios/shaders completos:

```bash
python3 scripts/check_live_environment.py --route-root "$CHILTERN_ROUTE" --online
```

`--online` agrega consultas reales; sin él verifica solo modos manuales y tormenta.
`--software` utiliza lavapipe en lugar de una GPU de hardware.

Las capturas `.stream.json` incluyen selección, ubicación, zona, hora UTC/local,
clima efectivo, dato meteorológico y contadores de tormenta. Para capturar un
rayo real del renderer se puede añadir `OPENRAILSRS_SCREENSHOT_DURING_LIGHTNING=1`
a una captura de tormenta: pausa ese evento mientras se preparan los recursos GPU,
sin insertar geometría falsa. Las [pruebas manuales](PLAYER_MANUAL_TESTS.md#27-hora-real-clima-del-lugar-y-tormentas)
indican qué observar.

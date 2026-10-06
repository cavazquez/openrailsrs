<div align="center">

# openrailsrs

**Simulador ferroviario en Rust y Bevy** — conducción desde cabina y exterior, escenarios originales y servicios con estaciones, pasajeros y tráfico; núcleo de simulación independiente de los gráficos.

[![CI](https://github.com/cavazquez/openrailsrs/actions/workflows/ci.yml/badge.svg)](https://github.com/cavazquez/openrailsrs/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/cavazquez/openrailsrs/graph/badge.svg)](https://codecov.io/gh/cavazquez/openrailsrs)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)

</div>

## Qué es

Núcleo de simulación **sin gráficos** (Linux-first, Rust ≥1.95). CSV para series temporales; TOML para escenarios. Viewer 2D (`minifb`) y 3D (Bevy 0.19.1) en crates aparte. Referencia de paridad fijada en **Open Rails 1.6.1**; commit y hashes en [`oracles/openrails-reference.toml`](oracles/openrails-reference.toml).

Fases y prioridades: [`ROADMAP.md`](ROADMAP.md). Docs: [`docs/README.md`](docs/README.md).

La [web del proyecto](https://cavazquez.github.io/openrailsrs/) presenta el recorrido,
capturas reales, la experiencia, instalación y estado de compatibilidad. Su fuente
está en [`website/`](website/README.md), con layout y contenido separados y páginas de experiencia, contenido, instalación y compatibilidad.

## CI local

```bash
./check.sh   # fmt → clippy → tests → build
```

GitHub Actions: mismo `check.sh` + cobertura Codecov + visual smoke (xvfb/lavapipe).

## Inicio rápido

```bash
cargo build
cargo test
cargo run -p openrailsrs-cli -- sim examples/smoke/scenario.toml
```

```bash
# Servicio completo: seis estaciones Northolt Park → Gerrards Cross (15,32 km).
# Necesita el Content Chiltern instalado; CHILTERN_ROUTE permite cambiar su ubicación.
./scripts/run_chiltern_service.sh  # menú de inicio
./scripts/run_chiltern_service.sh --direct  # servicio predeterminado
./scripts/run_chiltern_service.sh --autodrive --cab

# La misma partida y conductor automático sin ventana.
target/debug/openrailsrs play-service examples/chiltern_extended/scenario.toml --out-dir tmp/service

# Verifica versión/hashes y compara física con las capturas OR congeladas.
python3 scripts/run_oracles.py
```

`1` cabina, `Alt+1` alterna 2D/3D, `2` exterior; `W/S` inversor, `A/D` regulador, `;/'` freno, `Q` puertas, `P`/`Esc` pausa. `F5` información de conducción, `F4` monitor de vía. En cada estación: detenerse a ≤0,1 m/s dentro de ±10 m, abrir puertas, completar el embarque y cerrar puertas; el HUD separa pasajeros y horario. **F10 → Práctica de estaciones** permite probar paradas en 5 s. El menú conserva también el servicio corto de tres estaciones. Detalles y límites de paridad: [`examples/chiltern_local/README.md`](examples/chiltern_local/README.md).

Pruebas de menús, guardado, monitor, libreta, formación, despachador, HUD, clima, sonido y ratón en cabina: [`docs/PLAYER_MANUAL_TESTS.md`](docs/PLAYER_MANUAL_TESTS.md). Cada sección indica qué hacer y qué observar en la partida.

Guías: [`docs/CHILTERN.md`](docs/CHILTERN.md) · [`docs/VIEWER3D_TESTING.md`](docs/VIEWER3D_TESTING.md) · [`docs/BEVY.md`](docs/BEVY.md).

## Funciones implementadas

### Partida, contenido y controles

- Menú de rutas, servicios, formaciones, recorridos, hora, estación del año y clima; auditoría de recursos antes de iniciar.
- **Contenido oficial** desde el menú: descarga, progreso, cancelación y auditoría de actividades. Actualizaciones del autor junto a copias anteriores, identificadas por fecha y commit/hash; recursos fuera de Git en datos del usuario, con almacenamiento persistente para Snap. Instalador y catálogo embebidos, sin necesitar el checkout; requiere Python 3. Biblioteca con **Abrir carpeta** y **Copiar diagnóstico** de referencias y destinos absolutos. Búsqueda y actualización solo en el repositorio original identificado del catálogo. [Catálogo web de rutas y autores](https://cavazquez.github.io/openrailsrs/contenido.html) · [Uso, licencias y límites](docs/OFFICIAL_CONTENT.md).
- **Vehículos con `Include` nativo**: expande `.inc` anidados antes de leer física, cabinas, luces y efectos, con límites y confinamiento al paquete. Demo Model 1 permite la formación original de siete vehículos, cabina 3D y servicio nativo de Edimburgo a Linlithgow con sus tres paradas; la auditoría acepta una cabina válida y avisa si la alternativa falta.
- Las actividades nativas iniciadas desde el menú convierten el punto inicial de la cola de Open Rails a la cabeza del tren usando la longitud de la formación elegida. Los escenarios TOML ya preparados conservan sus posiciones.
- Hora visual y clima actuales del lugar, opcionales e independientes: fecha/zona IANA y horario de verano de la ruta; Open-Meteo en segundo plano con caché y respaldo manual. Tormentas elegibles con rayos ramificados, destellos y truenos demorados por distancia; F10 permite elegir manual y desactivar destellos. [Guía y límites](docs/LIVE_ENVIRONMENT.md).
- Servicio Chiltern de **seis estaciones / 15,32 km**: Northolt Park → South Ruislip → West Ruislip → Denham → Denham Golf Course → Gerrards Cross. También está disponible el servicio corto de tres estaciones.
- El conductor automático aplica la precaución de la próxima señal normal, evitando mantener restricciones de señales ya superadas o situadas después de una verde. El horario propio de la extensión conserva el límite nativo de 15 mph y permite completar sus seis paradas a tiempo.
- Conducción manual y automática, embarque/desembarque, puertas, horario, puntuación y resumen final. Pasajeros y espera de horario tienen contadores separados; F10 permite práctica de cinco segundos sin espera de horario. Una salida anticipada con puertas cerradas se registra y permite continuar.
- Pausa, guardado/carga del jugador y tráfico, libreta F7, formación F9, mapa/despachador M, ajustes F10 y controles con ratón en cabina.
- Operaciones F9: frenos de mano, mangueras y llaves de freno, batería, tracción y mando múltiple; desacoplar una sección posterior asegurada y volver a acoplarla sobre el mismo recorrido. Retroceso con el tren detenido y protección contra operaciones inválidas.
- Mapa M con red, itinerario, estaciones, jugador y tráfico; órdenes de señales, cambios libres y recálculo del recorrido. Las órdenes sobre vías ocupadas se rechazan.
- HUD de conducción F5, monitor gráfico de vía F4, ayuda F6 y diez páginas de HUD avanzado F8: formación, locomotora, potencia distribuida, alimentación, frenos, fuerzas, despachador, clima y diagnóstico, además de la vista general. El botón **km/h ↔ mph** de la barra inferior y la tecla **U** alternan las unidades de velocidad y límites durante la partida; F10 permite reasignar la tecla y guardar la preferencia. Los instrumentos originales conservan la escala indicada por su modelo.
- Ajustes persistentes de distancia, campo visual, escala de interfaz, unidades, sombras, niebla, volumen, puesto de conducción y controles. Las asignaciones rechazan conflictos; Q opera puertas sin mover la cámara. Texto con tildes y caracteres españoles.

### Cabina, escenario y clima

- Cabinas originales **2D y 3D**, vistas de pasajero y cámaras exteriores. Instrumentos CVF, palancas, iluminación nocturna, **luz interior con I** y perfiles de asiento/barrido por cabina. El manómetro de cilindro lee el vehículo principal.
- Preparación de cabina en segundo plano, texturas únicas por archivo y reutilización de mallas e instrumentos al volver de la cámara exterior. La carga espera también al interior de la locomotora.
- Terreno, vías, edificios, árboles, cercos, carreteras, vehículos y señales de los recursos MSTS/Open Rails. Streaming de sectores durante el viaje, origen flotante, agrupación de objetos en GPU y cambios de detalle con transición para geometría rígida.
- Paquetes independientes como **Demo Model 1** resuelven su propia carpeta `global/`, aunque el nombre del paquete difiera del de la ruta. Los buffers de alturas y huecos del terreno admiten los nombres Windows con mayúsculas distintas, también en la carga asíncrona de Bevy, conservando los archivos originales.
- Cámara exterior con altura mínima sobre el terreno; ruedas, bogies y señales semafóricas animados con la sesión. Cielo y efectos excluidos de las sombras; edificios y vegetación conservan las transparencias de los materiales originales. La carga de terreno se coordina con el origen flotante para mantenerlo alineado con la vía.
- Sol según ubicación/hora/estación y entorno original, faros sobre la vía, cielo nocturno con estrellas en tiempo despejado, nubes y niebla atmosférica y de suelo.
- **Lluvia y nieve seleccionables**: precipitación exterior, gotas/copos sobre el vidrio y barrido del limpiaparabrisas. El tablero y el HUD conservan su legibilidad. Nieve tiene visibilidad de 500 m y cobertura del terreno y exterior de trenes. **Los edificios conservan sus materiales y texturas, incluidos los techos**; no se sustituyen por variantes Snow ni reciben cobertura blanca. El mojado por lluvia alcanza terreno, materiales originales opacos, PBR e instancias GPU.
- Copos irregulares con tamaños, rotación, viento y caída variados; campo estable en el mundo, máscara conservadora bajo techos y cobertura irregular sobre superficies superiores. Nubes con ruido 3D sin bandas de proyección. Cálculo de precipitación **GPU/CPU/Mixto/Auto** con adaptación por tiempo de cuadro y presión de VRAM.
- Renderizador seleccionable al iniciar: GPU, automático o software CPU. Telemetría separada de RSS, VRAM del proceso, GTT y memoria global; percentiles de partida separados de la carga inicial. [Uso, mediciones y límites](docs/WEATHER_EXECUTION.md).
- Preparador de pilotos nativos de tres estaciones desde PAT/TDB/CON y SIGSCR originales, formación auditada, tráfico opcional y procedencia. El menú conserva el escenario y material rodante propios de cada ruta. [Belgrano CC: archivos originales todavía pendientes](docs/NATIVE_ROUTE_PILOT.md).
- Menú con descubrimiento de escenarios completos en todo `examples/`, también en subdirectorios y variantes de ensayo. Reportes, overlays, campañas y horarios se operan con sus herramientas propias. Los ejemplos Mitre utilizan el recorrido importado; necesitan los modelos del CAF 6000 para jugar en 3D y no incluyen el paisaje MSTS argentino original. El menú informa las formaciones incompletas.
- **Texturas ACE/DDS/KTX2 y caché persistente**: se conservan los bloques DXT1/3/5 originales, mipmaps, sRGB y transparencia, con alternativa RGBA para dispositivos sin BC. KTX2 nativo y derivados sin pérdida de ACE/DDS, comprimidos con Zstd en datos del usuario; se invalidan al cambiar el original y se reconstruyen si se dañan. Conversores `textures-dds` y `textures-ktx2`, sin reemplazar recursos del autor. [Uso y límites](docs/GPU_TEXTURES.md).
- Escape diésel y vapor desde emisores ENG originales; partículas limitadas, sin sombras y coherentes con el reloj de simulación.
- Sonido original SMS/WAV por vehículo: RPM y frenos propios, disparadores y atenuación por distancia de OR 1.6.1, referencias `Include`, interiores por coche y paso de sonido exterior declarado por el autor. Oráculo capturado de las DLL originales y comprobador WAV con lista de recursos ausentes. [Pruebas y límites](docs/NATIVE_AUDIO.md).

### Simulación y señales

- Dos servicios de tráfico vivo con paradas, reloj compartido, ocupación de la formación completa y restauración de partidas.
- Los servicios nativos incorporan **SIGSCR y dirección TDB originales**: 66 cabezas de Chiltern y 678 de Demo Model 1. El intérprete acotado admite las funciones normal, distante, INFO, repetidor y maniobras probadas de estas rutas. Conserva los ocho aspectos nativos; verifica todas las ramas y propaga las órdenes del despachador. El coordinador comparte reservas y bloqueos de agujas, protege la cola y busca un desvío hacia las estaciones pendientes cuando hay una ruta libre. F8 muestra esperas, bloqueos y cambios de itinerario; guardar conserva las concesiones. Las señales declarativas siguen disponibles.
- Tracción, resistencia Davis, pendientes y límites por posición/sentido; un aumento de velocidad espera a que pase la cola. Frenos por vehículo, parámetros originales EP/aire, diésel, vapor básico y dinámica opcional de acopladores.
- Pantalla ETCS/DMI y `BasicEtcsTcs` en Rust, con estado, planificación y controles. Host C# opcional en proceso .NET separado, API OR 1.6.1 acotado, ocho aspectos y hasta 32 señales/postes por delante, máxima del tren, ACK/menú y freno conectado a la sesión; errores y timeouts provocan intervención. **Save/Restore** para scripts que implementan ambos hooks, identidad SHA-256 y restauración preparada antes de modificar la partida. [Contrato, límites y prueba Linux](docs/TCS_CSHARP_HOST.md).
- Auditoría de formaciones por vehículo: recursos gráficos y SMS/WAV, subsistemas/scripts declarados, avisos de compatibilidad en menú, trocha y déficit de peralte. Diagnóstico de confort en curvas nativas en F8 → Locomotora, con diez casos de la fórmula original C# de OR 1.6.1 congelados y probados en Rust. [Alcance y comprobaciones](docs/SIGNALS_AND_CONTENT_SCOPE.md).
- **Peralte automático** en curvas compatibles: estándares de ruta y transiciones del algoritmo fijado de OR 1.6.1; perfil compartido por mallas originales de vía, tren y cámara. Conserva UV y peralte escrito; excluye agujas, vías múltiples y tablas antiguas sin validar. F10 permite desactivarlo para la próxima partida. Diez perfiles del código C# original forman un segundo oráculo independiente.
- **Class 47 + seis Mk2 de Demo Model 1**: lectura de parámetros `Include` originales, mínimo de 450 RPM para alimentación, freno de servicio neumático con carrera de pistón y deslizamiento de ruedas. Cabina y manómetros contrastados con OR 1.6.1; segundo oráculo físico fijo de 250 s. Depósitos, emergencia y otros sistemas de freno conservan un alcance parcial.
- Validación de consistencias, importación MSTS, escenarios TOML, simulación sin ventana, CSV/JSON, comparación de trazas y oráculos fijados en Open Rails 1.6.1.
- Presupuesto flexible de subida a GPU de **8 MiB por cuadro**, con reintento de las mallas preparadas tarde; trabajo progresivo del escenario y telemetría de RAM/P50/P95/P99. La pantalla de carga espera recursos y shaders de GPU; el reloj de la partida empieza después.
- Siete vistas visuales fijas, máscaras sobre píxeles de cabina/formación, comparación con capturas OR 1.6.1 y pruebas de fallos reales del renderer. [Goldens y comando de validación](docs/fixtures/visual/player_goldens/README.md).

### Alcance y pruebas

La compatibilidad se valida por función y contenido: **no se afirma paridad completa con Open Rails**. SIGSCR cubre las funciones probadas de Chiltern y Demo Model 1. El despachador mantiene seguridad y busca alternativas hacia delante; reversas automáticas, enlaces y horarios avanzados de OR requieren trabajo adicional. El peralte generado usa la velocidad de diseño de la ruta; falta validar límites locales por categoría y tablas antiguas. El API C# sigue acotado. El amperímetro estima carga, vapor y frenos tienen subsistemas parciales, y la paridad física se certifica por ensayo: el servicio nativo completo de tres estaciones pasa sus tolerancias originales, sin afirmar equivalencia de todas las rutas. La nieve es visual, sin termodinámica de deshielo ni adhesión por hielo.

La tracción eléctrica requiere tensión y un captador compatible: catenaria, tercer o cuarto riel. Bajar el pantógrafo, abrir el disyuntor o entrar en un sector sin tensión corta el esfuerzo; el tren sigue por inercia. `O` acciona el pantógrafo y `J` el disyuntor; F8 muestra cada coche motor. Los ejemplos `electric_supply` permiten comprobarlo en un recorrido corto.

Los diésel tienen arranque y parada con RPM, depósitos finitos y consumo también en ralentí. `K` acciona el primer motor; `B` permite controlar cada motor. En vapor, `B` abre el fogonero automático o manual: corte, pala, tiro, dos inyectores, soplador y purgas. El ténder, la caldera y el fuego conservan reservas distintas; agotar agua o carbón tiene consecuencias. Los instrumentos, humo y eventos SMS siguen esos estados. `examples/traction_operation` contiene recorridos de 1 km y pruebas de agotamiento en segundos, disponibles desde el menú. [Cómo probarlos](docs/PLAYER_MANUAL_TESTS.md#38-vapor-y-diésel-reservas-arranque-y-fogonero). La termodinámica del vapor sigue simplificada y la cremallera no está implementada. [Estado de diésel, vapor y electricidad](docs/TRACTION_SUPPORT.md).

La última prueba del 5 de octubre completa Chiltern con formación original,
dos servicios de tráfico y seis paradas a horario. En RX 7600/Vulkan,
1280×720 y radio 450 m, la preparación asíncrona de cabina reduce el peor
cuadro inicial de **4,37 a 1,48 s**. En partida quedan **dos cuadros mayores a
100 ms**, máximo **122,52 ms**, P50/P95/P99 **25/25/51 ms**, pico RAM **2089 MiB**
y VRAM **1644 MiB**. Las doce cámaras de las seis estaciones pasan sus límites;
esto no certifica iluminación idéntica por píxel.

El oráculo físico Pullman conserva RMS **0,2081 m/s**, pico **1,0921 m/s** y
odómetro máximo **39,61 m**. El nuevo ensayo Class 47 de 250 s obtiene
**0,2382 m/s / 1,9415 m/s / 25,79 m**. Ambos pasan **0,75 m/s / 2 m/s / 45 m**
con referencias y tolerancias intactas. El ensayo corto de la Class 47 y los
viajes gráficos completos tienen alcances distintos.

Pruebas manuales y resultados verificables: [PLAYER_MANUAL_TESTS.md](docs/PLAYER_MANUAL_TESTS.md), [PLAYER_POLISH_QA.md](docs/PLAYER_POLISH_QA.md) y [OR_PARITY.md](docs/OR_PARITY.md). Las capturas nativas y sus poses están en [las referencias de estaciones](docs/fixtures/visual/or_reference/chiltern_station_views/README.md). No se ajustan tolerancias para hacer pasar diferencias conocidas.

Para probar nieve, elegí **Origen del clima → Elegido por el jugador** y **Clima manual / respaldo → Nieve** en el menú, o iniciá directamente:

```bash
OPENRAILSRS_WEATHER=snow ./scripts/run_chiltern_service.sh --direct
```

Desde el menú también podés elegir noche, lluvia, niebla, nublado o tormenta, y optar por hora y/o clima actuales del lugar. F10 permite cambiar su origen y elegir clima durante la partida; el reloj del servicio sigue independiente. **V** activa el limpiaparabrisas; las teclas de faros, luces y demás acciones se consultan en **F6** y se reasignan en **F10**.

## CLI

El [paquete Linux y la receta Snap](docs/DISTRIBUTION.md) incluyen los binarios,
shaders y ejemplos necesarios para ejecutar desde otra carpeta. El contenido
original se instala por separado y queda en los datos del usuario; no se
incorpora al paquete ni al repositorio. El paquete agrega `Jugar.sh --check`,
`Jugar.sh`, `LEEME.txt` y metadatos/hashes de compilación. No requiere Rust ni
Cargo para jugar. La construcción de QA actual requiere **glibc 2.43**; todavía
no es un binario general para distribuciones anteriores.

```bash
cargo install --path crates/openrailsrs-cli   # binario `openrailsrs`

openrailsrs inspect path/file.eng
openrailsrs sim examples/smoke/scenario.toml
openrailsrs play-headless examples/smoke/scenario.toml
openrailsrs compare run1.csv run2.csv --max-velocity-rms 0.5
openrailsrs audit-vehicle examples/chiltern/trains/RF_Blue_Pullman/RF_WP_DMBSA.eng
```

También hay importación de rutas MSTS y OSM, inspección de formatos, exportación GeoJSON, replay 2D/3D, despachador sin ventana, horarios con varios trenes y campañas con progreso guardado. Consultá `graph`, `export-geojson`, `replay --watch`, `cab`, `dispatch`, `timetable`, `campaign`, `import-msts`, `import-osm`, `compare-or` y `oracle-suite` mediante `openrailsrs --help`.

## Crates (resumen)

| Crate | Rol |
|-------|-----|
| `formats` / `scenarios` / `route` / `track` / `train` | Datos MSTS + grafo + consists |
| `sim` / `game` / `validate` / `export` / `cli` | Headless + CLI |
| `viewer` | Replay 2D |
| `or-shader` / `bevy-scenery` / `viewer3d` / `render3d` | Capa 3D Bevy — [`docs/BEVY.md`](docs/BEVY.md) |

## Notas de paridad visual (OR)

Bugs reales ya cubiertos por tests en `check.sh` (no “arreglar” a ojo):

- Terreno: patches 16×16 / 17×17; diagonal OR; UV `W/B/C/H`; no sumar `CenterX/Z` encima del placement local.
- Shapes: `prim_state_idx` intercalado con trilists; alpha por ShaderName/`AlphaTestMode`/ACE (no solo nombre “glass”).
- Tren live: no forzar LOD lejano cerca de cámara; forests: `TreeSize` del WORLD.

Física vs OR: [`docs/OR_PARITY.md`](docs/OR_PARITY.md). Trazas: [`docs/OR_TRACE_COMPARISON.md`](docs/OR_TRACE_COMPARISON.md).

## Licencia

GPL-3.0 — ver `LICENSE`.

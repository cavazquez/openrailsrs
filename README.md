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
está en [`website/`](website/README.md), con layout y contenido separados y seis páginas.

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
- Servicio Chiltern de **seis estaciones / 15,32 km**: Northolt Park → South Ruislip → West Ruislip → Denham → Denham Golf Course → Gerrards Cross. También está disponible el servicio corto de tres estaciones.
- Conducción manual y automática, embarque/desembarque, puertas, horario, puntuación y resumen final. Pasajeros y espera de horario tienen contadores separados; F10 permite práctica de cinco segundos sin espera de horario. Una salida anticipada con puertas cerradas se registra y permite continuar.
- Pausa, guardado/carga del jugador y tráfico, libreta F7, formación F9, mapa/despachador M, ajustes F10 y controles con ratón en cabina.
- Operaciones F9: frenos de mano, mangueras y llaves de freno, batería, tracción y mando múltiple; desacoplar una sección posterior asegurada y volver a acoplarla sobre el mismo recorrido. Retroceso con el tren detenido y protección contra operaciones inválidas.
- Mapa M con red, itinerario, estaciones, jugador y tráfico; órdenes de señales, cambios libres y recálculo del recorrido. Las órdenes sobre vías ocupadas se rechazan.
- HUD de conducción F5, monitor gráfico de vía F4, ayuda F6 y diez páginas de HUD avanzado F8: formación, locomotora, potencia distribuida, alimentación, frenos, fuerzas, despachador, clima y diagnóstico, además de la vista general.
- Ajustes persistentes de distancia, campo visual, escala de interfaz, unidades, sombras, niebla, volumen, puesto de conducción y controles. Las asignaciones rechazan conflictos; Q opera puertas sin mover la cámara. Texto con tildes y caracteres españoles.

### Cabina, escenario y clima

- Cabinas originales **2D y 3D**, vistas de pasajero y cámaras exteriores. Instrumentos CVF, palancas, iluminación nocturna y perfiles de asiento/barrido por cabina. El manómetro de cilindro lee el vehículo principal.
- Terreno, vías, edificios, árboles, cercos, carreteras, vehículos y señales de los recursos MSTS/Open Rails. Streaming de sectores durante el viaje, origen flotante, agrupación de objetos en GPU y cambios de detalle con transición para geometría rígida.
- Cámara exterior con altura mínima sobre el terreno; ruedas, bogies y señales semafóricas animados con la sesión. Cielo y efectos excluidos de las sombras; edificios y vegetación conservan las transparencias de los materiales originales. La carga de terreno se coordina con el origen flotante para mantenerlo alineado con la vía.
- Sol según ubicación/hora/estación y entorno original, faros sobre la vía, cielo nocturno con estrellas en tiempo despejado, nubes y niebla atmosférica y de suelo.
- **Lluvia y nieve seleccionables**: precipitación exterior, gotas/copos sobre el vidrio y barrido del limpiaparabrisas. El tablero y el HUD conservan su legibilidad. Nieve utiliza las variantes de textura originales disponibles según la estación, visibilidad de 500 m y cobertura de terreno/superficies superiores. El mojado alcanza terreno, materiales originales opacos, PBR e instancias GPU.
- Copos irregulares con tamaños, rotación, viento y caída variados; campo estable en el mundo, máscara conservadora bajo techos y cobertura irregular sobre superficies superiores. Nubes con ruido 3D sin bandas de proyección. Cálculo de precipitación **GPU/CPU/Mixto/Auto** con adaptación por tiempo de cuadro y presión de VRAM.
- Renderizador seleccionable al iniciar: GPU, automático o software CPU. Telemetría separada de RSS, VRAM del proceso, GTT y memoria global; percentiles de partida separados de la carga inicial. [Uso, mediciones y límites](docs/WEATHER_EXECUTION.md).
- Preparador de pilotos nativos de tres estaciones desde PAT/TDB/CON y SIGSCR originales, formación auditada, tráfico opcional y procedencia. El menú conserva el escenario y material rodante propios de cada ruta. [Belgrano CC: archivos originales todavía pendientes](docs/NATIVE_ROUTE_PILOT.md).
- Escape diésel y vapor desde emisores ENG originales; partículas limitadas, sin sombras y coherentes con el reloj de simulación. Motor de sonido original SMS/WAV con eventos y separación interior/exterior.

### Simulación y señales

- Dos servicios de tráfico vivo con paradas, reloj compartido, ocupación de la formación completa y restauración de partidas.
- El servicio extendido incorpora los **scripts SIGSCR originales de Chiltern** y su dirección TDB: intérprete acotado para condiciones, variables y funciones normal/distante utilizadas por esta ruta. Conserva los ocho aspectos nativos en las lámparas; ocupación y órdenes del despachador restringen la autoridad. Los escenarios anteriores conservan sus reglas declarativas.
- Tracción, resistencia Davis, pendientes y límites por posición/sentido; un aumento de velocidad espera a que pase la cola. Frenos por vehículo, parámetros originales EP/aire, diésel, vapor básico y dinámica opcional de acopladores.
- Pantalla ETCS/DMI y `BasicEtcsTcs` en Rust, con estado, planificación y controles. Host C# opcional en proceso .NET separado, API OR 1.6.1 acotado, ACK/menú y freno conectado a la sesión; errores y timeouts provocan intervención. [Contrato y prueba Linux](docs/TCS_CSHARP_HOST.md). No implementa todo el API C# original ni Save/Restore del script.
- Validación de consistencias, importación MSTS, escenarios TOML, simulación sin ventana, CSV/JSON, comparación de trazas y oráculos fijados en Open Rails 1.6.1.
- Presupuesto flexible de subida a GPU de **8 MiB por cuadro**, con reintento de las mallas preparadas tarde; trabajo progresivo del escenario y telemetría de RAM/P50/P95/P99. La pantalla de carga espera recursos y shaders de GPU; el reloj de la partida empieza después.
- Siete vistas visuales fijas, máscaras sobre píxeles de cabina/formación, comparación con capturas OR 1.6.1 y pruebas de fallos reales del renderer. [Goldens y comando de validación](docs/fixtures/visual/player_goldens/README.md).

### Alcance y pruebas

La compatibilidad se valida por función y contenido: **no se afirma paridad completa con Open Rails**. SIGSCR cubre las funciones usadas por el corredor Chiltern; enlaces avanzados, reservas generales, otros scripts y C# requieren trabajo adicional. El amperímetro sigue estimando carga, el vapor y los frenos no reproducen todos los subsistemas originales, y la comparación física histórica del servicio completo sigue fuera de tolerancia. La nieve es una cobertura visual, sin termodinámica de deshielo ni física de adhesión por hielo.

Pruebas manuales y resultados verificables: [PLAYER_MANUAL_TESTS.md](docs/PLAYER_MANUAL_TESTS.md), [PLAYER_POLISH_QA.md](docs/PLAYER_POLISH_QA.md) y [OR_PARITY.md](docs/OR_PARITY.md). Las capturas nativas y sus poses están en [las referencias de estaciones](docs/fixtures/visual/or_reference/chiltern_station_views/README.md). No se ajustan tolerancias para hacer pasar diferencias conocidas.

Para probar nieve, elegí **Clima → Nieve** en el menú, o iniciá directamente:

```bash
OPENRAILSRS_WEATHER=snow ./scripts/run_chiltern_service.sh --direct
```

Desde el menú también podés elegir noche, lluvia o niebla. **V** activa el limpiaparabrisas; las teclas de faros, luces y demás acciones se consultan en **F6** y se reasignan en **F10**.

## CLI

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

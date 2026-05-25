# Open Rails: cómo genera geometría 3D (terreno, shapes, trenes, escenario)

Este documento resume el **flujo real del código** del simulador [Open Rails](https://github.com/openrails/openrails), analizado a partir de un clon shallow en `/tmp/openrails` (rama `master` al momento del estudio). Sirve de referencia para [issue #8](https://github.com/cavazquez/openrailsrs/issues/8) (viewer 3D con Bevy en openrailsrs): **no hay que portar MonoGame**, pero sí entender **qué datos produce cada subsistema** y **cómo llegan a la GPU**.

---

## 1. Arquitectura general (dónde vive el “3D”)

- El ejecutable principal de juego es **RunActivity**; el subsistema 3D está bajo  
  `Source/RunActivity/Viewer3D/`.
- La documentación de alto nivel está en  
  [Docs/Architecture.md](https://github.com/openrails/openrails/blob/master/Docs/Architecture.md):  
  `Player → Game → UI → Viewer → Simulation` y hilos **Render / Updater / Loader / Sound**.
- El mundo visual se agrega en **`World`** (`Viewer3D/World.cs`): instancia y orquesta  
  `TerrainViewer`, `SceneryDrawer`, `TrainDrawer`, cielo, precipitación, etc.

**Idea clave:** casi todo lo “dibujable” implementa el patrón **PrepareFrame → RenderFrame → Draw** sobre **MonoGame** (`GraphicsDevice`, `VertexBuffer`, `IndexBuffer`).

---

## 2. Lista de render y primitivas (`RenderFrame`)

Archivo: `Viewer3D/RenderFrame.cs`.

- **`RenderPrimitive`** (abstracta): cada cosa dibujable expone `Draw(GraphicsDevice)`.
- Las primitivas se clasifican en **`RenderPrimitiveGroup`** (Cab, Sky, World, Interior, …) y se ordenan en **`RenderPrimitiveSequence`** (opaco vs alpha, cielo, mundo, cabina, overlay).
- En cada frame del **Updater**, los “drawers” llaman a `PrepareFrame` y registran primitivas con métodos del estilo **`frame.AddAutoPrimitive(...)`** (distancia, material, matriz mundo, grupo, flags).

Esto es el análogo conceptual a **“systems” Bevy que encolan meshes con material + transform”**.

---

## 3. Terreno (heightfield por parche)

Archivo principal: **`Viewer3D/Terrain.cs`**.

### 3.1 Carga de tiles (`TileManager` en `Tiles.cs`)

- **`TileManager`** mantiene una **caché MRU** de tiles (`MaximumCachedTiles = 8*8`).
- Resuelve archivos de terreno MSTS por **TileX / TileZ** y “zoom” (alta resolución vs montañas lejanas).
- **`LoadAndGetElevation`**: normaliza coordenadas dentro del tile (bucles ±1024 / 2048) y **interpola** altura entre muestras del tile cargado.

### 3.2 De tile a malla: `TerrainTile` → `TerrainPrimitive`

- **`TerrainViewer.Load()`** (hilo **Loader**): según `ViewingDistance`, pide tiles alrededor de la cámara; construye **`TerrainTile`** por cada `Tile` único.
- Cada **`TerrainTile`** crea una matriz **`TerrainPrimitive[patchCount, patchCount]`** solo donde `patch.DrawingEnabled`.
- **`TerrainPrimitive`** hereda de **`RenderPrimitive`**.

### 3.3 Geometría del parche (polígonos)

En el constructor de **`TerrainPrimitive`**:

1. **`GetVertexBuffer`**  
   - Rejilla **17×17** vértices (`VertexPositionNormalTexture`).  
   - Posición XZ en metros relativos al centro del parche (`Patch.RadiusM`, `Tile.SampleSize`).  
   - **Y** = elevación vía `Elevation(x,z)` → `TileManager.GetElevation(Tile, ...)`.  
   - **UV**: transformación afín 2×3 desde el bloque MSTS `terrain_patchset_patch` (`W,B,X` y `C,H,Y` en el código).  
   - **Normal**: `TerrainNormal` / `SpecificTerrainNormal` — **derivada de la malla de elevaciones** (productos cruzados entre vecinos); comentario en código: decodificar desde `_N.RAW` sigue como TODO.

2. **`GetIndexBuffer`**  
   - Cuadrícula **16×16** celdas; cada celda = **2 triángulos** (6 índices).  
   - Patrón de diagonal **alternado** (`(z & 1) == (x & 1)`) para evitar “pliegues” en T-junctions.  
   - Si un vértice está “oculto” (`IsVertexHidden`, p. ej. túneles), **no se emiten** triángulos que lo usan → lista de índices variable.  
   - Si el patch es “completo” (sin agujeros), **`PatchIndexBuffer == null`** y se reusa un **`SharedPatchIndexBuffer`** estático (optimización).

3. **`PrepareFrame`**  
   - Traslada el parche al espacio relativo a la cámara (delta de tile × 2048 m).  
   - Convierte a coordenadas XNA: `Matrix.CreateTranslation(..., -Z)` (convención Z).  
   - **`frame.AddAutoPrimitive`** con material de terreno (`TerrainShared`, `TerrainSharedDistantMountain`, o `Terrain` si hay túnel / índice propio).

4. **`Draw`**  
   - `SetVertexBuffers` (incluye un **dummy** segundo stream para shaders de terreno).  
   - `DrawIndexedPrimitives(TriangleList, ...)`.

**Resumen:** el terreno es un **heightfield regular** por parche, texturizado por shaders que mezclan capas según datos del tile; los “huecos” son solo **índices omitidos**.

---

## 4. Shapes MSTS (`.s`): de archivo a `VertexBuffer`

### 4.1 Parseo (no está en Viewer3D)

- **`Orts.Formats.Msts.ShapeFile`** — `Source/Orts.Formats.Msts/ShapeFile.cs`: lee el `.s` y construye el modelo lógico MSTS (`shape`, `lod_controls`, `sub_objects`, `indexed_trilist`, etc.).

### 4.2 Instancia compartida en GPU

Archivo: **`Viewer3D/Shapes.cs`**.

- **`SharedShapeManager`**: diccionario **`path → SharedShape`**. **Una sola** carga por archivo; muchas instancias reutilizan vértices.
- **`SharedShape`**: en `LoadContent()`:
  - Instancia **`ShapeFile`** con la ruta del `.s`.
  - Opcionalmente lee **`.sd`** (`ShapeDescriptorFile`) para flags de textura (nieve, noche, sonido, FPS de animación).
  - Convierte matrices MSTS a **XNA** (`XNAMatrixFromMSTS` — ojo al **flip de Z** en varias filas).
  - Construye array **`LodControl[]` → `DistanceLevel[]` → `SubObject[]` → `ShapePrimitive[]`**.

- **`ShapePrimitive`**: contiene **material**, referencia a **vértices** (`VertexBufferSet` con `VertexPositionNormalTexture`), **jerarquía** de huesos (`Hierarchy`), índices, etc.
- Los vértices se generan en **`VertexBufferSet.CreateVertexData`**: por cada vértice MSTS, **`XNAVertexPositionNormalTextureFromMSTS`** ajusta posición/normal/UV al espacio XNA (de nuevo **Z negada** donde corresponde).

### 4.3 Instancias en el mundo: `StaticShape` / `PoseableShape` / `AnimatedShape`

- **`StaticShape`**: posición fija + `SharedShape.PrepareFrame(...)`.
- **`PoseableShape`**: copia **`SharedShape.Matrices`** a **`XNAMatrices[]`** y permite animación por nodo (`AnimateMatrix` recorre `Hierarchy`; soporta `slerp_rot`, `linear_key`, `tcb_key` leídos del `.s`).
- **`AnimatedShape`**: avanza **`AnimationKey`** con el reloj y llama `AnimateMatrix` para todos los nodos antes de preparar el frame.

### 4.4 Cómo se “emiten” los triángulos por frame

**`SharedShape.PrepareFrame(..., Matrix[] animatedXNAMatrices, ...)`** (mismo archivo, ~línea 2537):

1. Ajusta posición por **delta de tile** respecto a la cámara (2048 m por tile).
2. Para cada **`LodControl`**: test **FOV** con esferas de LOD; elige **nivel de detalle** según distancia a cámara y **`LODBias`** / extensión de distancia.
3. Por cada **`SubObject`** visible (día/noche con `HasNightSubObj`):
4. Por cada **`ShapePrimitive`**: multiplica matrices siguiendo **`Hierarchy`** desde el índice del primitivo hasta la raíz (`Matrix.Multiply` en bucle).
5. Multiplica por la **matriz mundo** del objeto (`xnaDTileTranslation`).
6. **`frame.AddAutoPrimitive(..., shapePrimitive.Material, shapePrimitive, ...)`** — la primitiva **ya sabe** cómo hacer `Draw` sobre su `VertexBuffer`.

**Resumen:** los polígonos del tren/vía/objeto vienen del **`.s` parseado una vez**; por frame solo cambian **matrices** (animación / bogies / posición del vagón).

---

## 5. Escenario estático (`.w`)

Archivo: **`Viewer3D/Scenery.cs`** (`SceneryDrawer`).

- Comentario de cabecera (resumen oficial en código): los **`.w`** están en `WORLD` del route; **cada archivo ≈ 2048 m** de región.
- **`Load()`** (Loader): para un rango de tiles alrededor de la cámara, **`LoadWorldFile`** → instancias de **`WorldFile`** (formato MSTS en `Orts.Formats.Msts`).
- Al crear **`WorldFile`**, por cada ítem de escenario se instancian **`StaticShape`** (u otras variantes) que llaman **`Viewer.ShapeManager.Get(path)`** → **`SharedShape`** del `.s` correspondiente.

**Resumen:** el `.w` **no genera vértices propios** de forma especial en el viewer; **delega en shapes** (mismo pipeline de la sección 4). Posición/orientación vienen del world file + transformadas a **`WorldPosition`**.

---

## 6. Trenes (rolling stock)

Archivo: **`Viewer3D/Trains.cs`** (`TrainDrawer`).

- Diccionario **`TrainCar → TrainCarViewer`**. Los coches **visibles** se cargan en el **Loader** (`LoadCar(car)`).
- **`TrainCarViewer`** (abstracto, `RollingStock/TrainCarViewer.cs`): define **`PrepareFrame(RenderFrame, ElapsedTime)`** ejecutado en el **Updater**.

Implementación principal: **`RollingStock/MSTSWagonViewer.cs`**:

- Carga **`PoseableShape TrainCarShape`** (y más `AnimatedShape` para enganches, mangueras, interior, etc.).
- En **`PrepareFrame`**: actualiza **partes animadas** (`AnimatedPart`) según estado del simulador (puertas, pantógrafo, frenos, partículas, etc.) y llama **`UpdateAnimation`**.
- La geometría del coche es, de nuevo, **la misma cadena `SharedShape` + `ShapePrimitive`**.

**Resumen:** el simulador (`Orts.Simulation`) actualiza **`TrainCar` / `WorldPosition`**; el viewer solo **refleja** estado en matrices y animaciones, luego encola primitivas.

---

## 7. Vía dinámica (`DynamicTrack`)

Archivo: **`Viewer3D/DynamicTrack.cs`** (`DynamicTrackViewer`, `DynamicTrackPrimitive`).

- Construye mallas a partir de **perfiles de raíl** (XML / datos de perfil) y la **pose** del tramo (`WorldPosition`, yaw, etc.).
- **`PrepareFrame`**: calcula centro para LOD, comprueba **FOV** y **cutoff por LOD** del perfil, luego añade primitivas al `RenderFrame` (mismo patrón).

---

## 8. Otros generadores de geometría (puntero rápido)

| Archivo / tipo        | Rol aproximado |
|-----------------------|----------------|
| `Forest.cs`           | **`ForestPrimitive`**: por cada `ForestObj`, **`CalculateTrees`** coloca `Population` árboles con RNG **sembrado por posición**; cada árbol son **2 triángulos** (6 vértices `VertexPositionNormalTexture`) en cruz (billboard-like); altura desde **`TileManager.LoadAndGetElevation`**; opcionalmente evita vías/carreteras usando secciones de track cercanas. |
| `Water.cs`            | Superficie de agua por tile. |
| `RoadCars.cs`         | Tráfico rodado. |
| `Signals.cs`          | Señales como objetos/shapes o primitivas dedicadas. |
| `Sky.cs`, `MSTSSky.cs`| Cielo / dome. |
| `Precipitation.cs`    | Partículas. |
| `ParticleEmitter.cs`  | Efectos en vagones. |

Cada uno sigue el mismo esquema: **cargar en Loader**, **PrepareFrame en Updater**, **Draw en Render**.

---

## 9. Prioridad para openrailsrs (issue #8): facilidad × resultado visible

Criterio: **primero** lo que sea relativamente fácil en Rust/Bevy y **se note en pantalla** en poco tiempo; **después** lo que depende de formatos MSTS pesados o de mucha semántica de Open Rails.

Leyenda: **F** = facilidad (5 = muy fácil), **V** = impacto visual inmediato (5 = muy evidente).

| Orden | Tarea | F | V | Por qué va en ese lugar |
|------:|-------|--:|--:|---------------------------|
| **1** | App Bevy + ventana + **cámara libre** (orbit / fly) sobre un plano o grilla — **✅ hecho** (`openrailsrs-viewer3d`) | 5 | 4 | Infraestructura mínima; valida el crate 3D y el bucle sin depender del sim. |
| **2** | **Grafo 3D desde `track.toml`**: aristas como tubos o líneas gruesas 3D, nodos como esferas; colorear switches / estaciones — **✅ hecho** | 4 | 5 | Ya tenéis topología 2D→3D (X,Y del grafo, Z=0 o elevación constante); se ve “la ruta” al instante. |
| **3** | **Marcador del tren** (cubo / flecha) posicionado con el mismo criterio que `openrailsrs-viewer` 2D (`edge_id` + `pos_on_edge_m` desde CSV o snapshot del sim) — **✅ hecho** | 4 | 5 | Prueba end-to-end “sim → posición → mundo 3D” sin parsear `.s`. |
| **4** | **HUD mínimo** (texto Bevy / egui): tiempo, velocidad, nombre de escenario | 4 | 3 | Poco riesgo; mejora la demo y copia el espíritu del HUD 2D actual. **✅ hecho** — franja Bevy UI inferior (título, replay, barra, leyenda trenes, atajos). |
| **5** | Objetos **`.w` como cajas** en posición local (`WorldFile` + `WorldItem` → cubo + etiqueta `kind`) sin mesh MSTS | 4 | 4 | Usa el parser ya hecho; el resultado se ve en rutas con tiles world reales. **✅ hecho** — escaneo `WORLD/`/`world/`, coords globales tile×2048, cubos por tipo. |
| **6** | **Un shape `.s` ASCII** con una sola primitiva LOD → **un `Mesh` Bevy** (vértices/índices desde `ShapeFile`) | 3 | 4 | Hit tangible “MSTS en 3D”; evitad binario tokenized al principio. **✅ hecho** — `build_mesh_from_shape`, LOD más cercano, mesh en objetos `Static` con `.s` en `SHAPES/`. |
| **7** | **Textura `.ace`** en material (mip 0 vía `openrailsrs-ace` → imagen GPU) sobre un cubo o el mesh del paso 6 | 3 | 3 | Cierra el pipeline visual de material; menos espectacular que la geometría pero útil. **✅ hecho** — `prim_state` → `TEXTURES/*.ace`, mip 0 RGBA8 en `StandardMaterial.base_color_texture`; fallback magenta si falta textura. |
| **8** | **Terreno estilo OR**: parche 16×16 + elevación (necesita **tiles MSTS** + `.y` / RAW / mismas convenciones que `TerrainPrimitive`) | 2 | 5 | Muy visible pero **dificultad y datos** suben mucho (shaders, vecinos, agujeros). **✅ hecho** — `.y` + `_Y.RAW`, parches 17×17 / diagonal alternada, mesh por tile en `TERRAIN/`; sin texturas multi-capa ni agujeros aún. |
| **9** | **Vía dinámica** (perfiles TSection / mallas como `DynamicTrackPrimitive`) | 2 | 4 | Mucha lógica y datos; mejor cuando el grafo y la cámara ya estén sólidos. **✅ hecho (PR1)** — segmentos orientados desde `Dyntrack` en `.w` (durmientes + 2 rieles, local +Z); sin perfiles TSection ni enlace `.tdb`. |
| **10** | **Rolling stock** — consist del escenario → cadena de meshes `.s` (fallback cubo); sin animaciones/LOD/bogies aún | 2 | 5 | PR1 ✅; PR2 (10b/10c) ✅; **PR3 (10d) ✅** — consist por tren desde `scenario.toml` (`[train]` + `[[extra_trains]]`); CSV solo trayectoria. |
| **11** | **Bosque** (población, RNG, colisión con vía) / **agua** / **cielo** / partículas | 2–3 | 3–4 | Bonificación visual; depende menos del sim y más de tuning y assets. |

**Regla práctica:** hasta el **orden 5** podés mostrar una demo creíble **sin** parser binario de `.s` ni archivos de terreno MSTS; del **6** en adelante es “contenido MSTS de verdad”.

### Pulido D (post-orden 3, sin reordenar la tabla)

Mejoras acotadas ya implementadas en `openrailsrs-viewer3d`:

- **Cámara follow (`T`)** — con replay activo: ciclo off → orbit follow (foco en el tren) → chase cam (detrás del tren); solo en modo orbit; pan con botón medio desactiva follow.
- **Señales 3D** — marcadores coloreados por `SignalAspect` (misma paleta que el viewer 2D), posicionados en arista con `position_m`.
- **Modo compact** — rutas con más de 800 aristas: aristas como líneas gizmo (no cilindros mesh), nodos Plain omitidos; log `render=compact` al arrancar.
- **HUD en pantalla** — franja inferior Bevy UI (~60 px): título y modo cámara; con replay: estado PLAY/PAUSED, tiempo, km/h, velocidad, barra de progreso, leyenda por tren y atajos (`Space`, `R`, `+/-`, `T`, `F1/F2`, `Esc`).
- **Objetos `.w` como cajas** — tiles en `WORLD/` parseados con `WorldFile`; posición global MSTS → Bevy; cubos coloreados por `Static` / `Forest` / `TrackObj` / `Signal` / `Dyntrack` / `Other`.
- **Shape `.s` → mesh** — `ShapeFile` ASCII al LOD más cercano; objetos world con `FileName` `.s` resuelven `SHAPES/` y dibujan malla real (fallback a cubo).
- **Textura `.ace` en material** — mip 0 vía `openrailsrs-ace` → `Image` Bevy; resuelve `TEXTURES/` (o `textures/`); UV con flip V; fallback magenta si falta el `.ace`.
- **Terreno heightfield** — tile `.y` + `_Y.RAW` en `TERRAIN/`; malla estilo OR (16×16 parches, 17×17 vértices, diagonal alternada); reemplaza el plano gris cuando hay datos.
- **Vía dinámica (básica)** — objetos `Dyntrack` en `.w` → segmento recto orientado (durmientes repetidos + dos rieles a lo largo de local +Z); longitud por defecto según extensión de la ruta; sin perfiles TSection ni `.tdb`.
- **Rolling stock (PR1)** — consist del `scenario.toml` → locomotora + vagones encadenados con offset longitudinal; mesh `.s` desde `SHAPES/` (fallback cubo coloreado).
- **Rolling stock (PR2)** — rotación MSTS (+Z forward → +X tren) y escala uniforme desde bbox del mesh a `length_m` del vehículo; frente alineado al offset del consist.
- **Rolling stock (PR3 / 10d)** — cada tren del replay (`primary` + `[[extra_trains]]`) carga su consist desde el TOML; el CSV aporta solo posición/velocidad.
- **Teletransporte (`G`)** — diálogo x,y,z para saltar la cámara; coords `pos`/`focus` en HUD.

---

## 10. Equivalencia sugerida para openrailsrs (issue #8)

| Open Rails | openrailsrs (dirección) |
|------------|-------------------------|
| `Orts.Simulation` | `openrailsrs-sim` (autoritativo) |
| `PrepareFrame` + snapshot | canal / component Bevy que recibe **estado mínimo** (trenes, tiempo) |
| `SharedShape` + VB | **Asset pipeline**: mesh Bevy desde `ShapeFile` / glTF intermedio |
| `TerrainPrimitive` | mesh de terreno desde elevación + mismo layout 17×17 / índices |
| `RenderFrame` ordenado | `Transparent3d` / `Opaque3d` + materiales Bevy |
| `TileManager` / `World` Mark/Sweep | `AssetServer` + unload por distancia |

---

## 11. Referencias de lectura en el repo clonado

Rutas bajo `/tmp/openrails/Source/`:

- `RunActivity/Viewer3D/Terrain.cs` — terreno.
- `RunActivity/Viewer3D/Shapes.cs` — shapes, LOD, animación, `AddAutoPrimitive`.
- `RunActivity/Viewer3D/Scenery.cs` — `.w` → `StaticShape`.
- `RunActivity/Viewer3D/Trains.cs` + `Viewer3D/RollingStock/MSTSWagonViewer.cs` — trenes.
- `RunActivity/Viewer3D/RenderFrame.cs` — pipeline de render.
- `RunActivity/Viewer3D/World.cs` — composición del mundo.
- `Orts.Formats.Msts/ShapeFile.cs` — parseo `.s`.

---

## 12. Nota legal

Open Rails es **GPL v3**. Este documento es **análisis arquitectónico**; cualquier reutilización de código fuente de Open Rails en otro proyecto debe respetar la licencia y la atribución correspondientes.

# MSTS / Open Rails Binary Shape Parser

Estado del parser binario de shapes `.s` y plan para llevarlo a paridad razonable con Open Rails.

## Referencia

El comportamiento base se toma del codigo fuente oficial de Open Rails:

- `Source/Orts.Parsers.Msts/SBR.cs`: `SIMISA@F` se descomprime antes de leer el sub-header; el bloque binario usa `u16 token + u16 flags + u32 remaining`, luego `u8 label_len` y label UTF-16 opcional.
- `Source/Orts.Parsers.Msts/TokenID.cs`: tabla oficial de tokens core MSTS; shape usa offset `0`, world usa offset `300`.
- `Source/Orts.Formats.Msts/ShapeFile.cs`: estructura esperada de `shape`, colecciones con `count`, `prim_states`, `lod_controls`, `sub_objects`, `vertices`, `vertex_sets`, `primitives` y `indexed_trilist`.

## Fixtures Binarios

Regla para agregar fixtures al repo:

- Aceptar solo contenido con licencia clara y compatible con el repo, o contenido generado por nosotros.
- No commitear assets MSTS/payware/freeware comunitarios sin permiso explicito.
- Preferir fixtures chicos y dedicados, no rutas completas.
- Guardar la fuente y licencia del fixture en el mismo directorio o en este documento.

Fixtures reales actuales en el repo:

| Fixture | Formato | Cobertura |
|---|---|---|
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_BP_PCFfwd.s` | `SIMISA@F` + `JINX0s1b` | puntos, normales, UVs, texturas, matrices, prim_states, LOD, primitivas |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_BP_PCFrear.s` | `SIMISA@F` + `JINX0s1b` | idem |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s` | `SIMISA@F` + `JINX0s1b` | fixture principal de regresion, 4869 triangulos |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSH.s` | `SIMISA@F` + `JINX0s1b` | idem |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_KFC.s` | `SIMISA@F` + `JINX0s1b` | idem |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_KFF.s` | `SIMISA@F` + `JINX0s1b` | idem |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_PSB.s` | `SIMISA@F` + `JINX0s1b` | idem |
| `examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_PSG.s` | `SIMISA@F` + `JINX0s1b` | idem |

Fixtures que conviene conseguir o generar:

| Tipo | Motivo |
|---|---|
| Binario sin comprimir `SIMISA@@` + `JINX0s1b` | Validar container sin zlib. |
| Shape multi-LOD real | Verificar seleccion por distancia y multiples `distance_level`. |
| Shape con animaciones | Cubrir `animations`, `anim_nodes`, `controllers`, `linear_pos`, `tcb_rot`, `slerp_rot`. |
| Shape con `uv_ops` y light configs | Cubrir `light_model_cfgs`, `uv_ops`, `texture_filter_names`. |
| Shape con alpha/blend/transparencia | Cubrir material flags, `alphatestmode`, `ZBufMode`, textura glass. |
| Shape con labels UTF-16 no ASCII | Confirmar que no rompemos parsing y decidir representacion. |
| Shape con `shape_named_data` / material palette | Cubrir tokens OR/MSTS menos comunes. |
| Binarios sinteticos por bloque | Tests unitarios chicos para cada block parser sin depender de assets grandes. |

Fuentes candidatas:

- `https://github.com/openrails/content.git`, usado por Open Rails para rutas auto-instalables. Ese repo es un catalogo de metadata (`routes.json`) y no contiene `.s`; sirve para encontrar rutas candidatas, pero antes de copiar fixtures hay que descargar la ruta concreta y revisar licencia de cada asset.
- Assets generados localmente por un encoder propio de tests.
- Fixtures aportados por usuarios con permiso explicito para redistribuirlos bajo la licencia del repo.

## Faltantes Del Parser

El estado actual parsea binarios reales comprimidos hasta obtener buffers, texturas, matrices, LODs, primitivas y triangulos. Aun asi, el camino interno sigue siendo `binario -> S-expression sintetica -> ShapeFile`, con heuristicas. Faltantes principales:

1. Reemplazar el dumper generico por un lector estructural estilo `SBR`.
2. Modelar `Vertex` completo: `ipoint`, `inormal`, `Color1`, `Color2`, `vertex_uvs`.
3. Corregir el armado de malla: `vertex_idxs` indexa la tabla de `vertices` del `sub_object`, no directamente `shape.points`.
4. Exponer `vertex_sets` y `geometry_info`, aunque sea de forma minima, para material/LOD/rendering correcto.
5. Completar `prim_state`: flags, shader index, `tex_idxs`, `ZBias`, `ivtx_state`, alpha test, light config, z-buffer mode.
6. Parsear `vtx_states`, `textures`, `texture_filter_names`, `shader_names` y `light_model_cfgs`.
7. Parsear `uv_ops` o degradarlos de forma explicita cuando no afecten el primer UV set.
8. Parsear animaciones: `animations`, `animation`, `anim_nodes`, `controllers`, `linear_pos`, `linear_key`, `tcb_rot`, `tcb_key`, `slerp_rot`.
9. Completar la tabla `TokenID` para todos los tokens de shape usados por Open Rails, no solo los actuales.
10. Mejorar errores: incluir token, offset absoluto, parent block y bytes restantes.
11. Agregar soporte de fixtures binarios por bloque para tests chicos y deterministas.
12. Evaluar si conviene mantener la conversion a AST para compatibilidad o migrar `ShapeFile::from_path` a un parser binario tipado directo.

## Plan De Implementacion

1. **Congelar regresiones actuales**
   - Mantener tests para todos los fixtures Chiltern actuales.
   - Agregar test unitario para `SIMISA@F`, `SIMISA@@`, `JINX0s1t` y `JINX0s1b`.

2. **Crear `BinaryBlockReader` interno**
   - API minima: `read_sub_block`, `read_i32`, `read_u32`, `read_f32`, `read_string`, `skip`, `end_of_block`.
   - Misma semantica que Open Rails: cada bloque consume sus bytes y valida `remaining`.

3. **Parsear shape binario directo a modelo intermedio**
   - Implementar lectores para: `shape`, `points`, `uv_points`, `normals`, `matrices`, `images`, `textures`, `prim_states`, `lod_controls`.
   - Mantener fallback textual para ASCII.

4. **Completar geometria real**
   - Agregar `Vertex` a `ShapeFile` o a `SubObject`.
   - Cambiar `Primitive.vertex_indices` para representar indices de vertices, y resolverlos a punto/normal/UV en el viewer.
   - Agregar tests de malla sobre un fixture binario real: vertices Bevy > 0, UVs validos, textura primaria esperada.

5. **Materiales y transparencia**
   - Mapear `prim_state.texture_idx` desde `tex_idxs[0]`.
   - Guardar flags de alpha/z-buffer para que el viewer pueda crear materiales transparentes cuando corresponda.

6. **Animaciones**
   - Primero parsearlas y conservarlas sin render.
   - Luego conectar bogies, ruedas, puertas/pantografos/cabina segun nombres de matrices.

7. **Fixture intake**
   - Revisar `openrails/content.git` y seleccionar 2-4 shapes con licencia compatible.
   - Copiar solo los `.s` minimos y documentar origen/licencia.
   - Si no hay licencia clara, generar fixtures sinteticos con un encoder de test.

8. **Paridad y limpieza**
   - Comparar conteos contra Open Rails o contra dumps conocidos.
   - Eliminar heuristicas de `shape_binary_to_ascii` cuando el parser directo cubra los mismos casos.
   - Documentar cualquier degradacion aceptada.

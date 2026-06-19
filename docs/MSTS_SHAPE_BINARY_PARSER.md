# MSTS / Open Rails Binary Shape Parser

Estado del parser binario de shapes `.s` y plan para llevarlo a paridad razonable con Open Rails.

## Referencia

El comportamiento base se toma del codigo fuente oficial de Open Rails:

- `Source/Orts.Parsers.Msts/SBR.cs`: `SIMISA@F` se descomprime antes de leer el sub-header; el bloque binario usa `u16 token + u16 flags + u32 remaining`, luego `u8 label_len` y label UTF-16 opcional.
- `Source/Orts.Parsers.Msts/TokenID.cs`: tabla oficial de tokens core MSTS; shape usa offset `0`, world usa offset `300`.
- `Source/Orts.Formats.Msts/ShapeFile.cs`: estructura esperada de `shape`, colecciones con `count`, `prim_states`, `lod_controls`, `sub_objects`, `vertices`, `vertex_sets`, `primitives` y `indexed_trilist`.

## Referencias secundarias

- OpenBVE [`Object.Msts/ShapeParser.cs`](../../OpenBVE/source/Plugins/Object.Msts/ShapeParser.cs): edge cases de shapes Kuju (secundario frente a OR). Ver [`OPENBVE_REFERENCE.md`](OPENBVE_REFERENCE.md).

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
2. Completar `Vertex` con `Color1`, `Color2` y flags. Ya se modelan `ipoint`, `inormal` y `vertex_uvs`.
3. Exponer `vertex_sets` y `geometry_info`, aunque sea de forma minima, para material/LOD/rendering correcto.
4. Completar `prim_state`: alpha test, light config, z-buffer mode y validacion fuerte de campos binarios tipados. Ya se modelan `flags`, `shader_idx`, `tex_idxs`, `ZBias` e `ivtx_state` cuando aparecen en la forma parseada.
5. Parsear `vtx_states`, `textures`, `texture_filter_names` y `light_model_cfgs`. Ya se exponen `shader_names`.
6. Parsear `uv_ops` o degradarlos de forma explicita cuando no afecten el primer UV set.
7. Parsear animaciones: `animations`, `animation`, `anim_nodes`, `controllers`, `linear_pos`, `linear_key`, `tcb_rot`, `tcb_key`, `slerp_rot`.
8. Completar la tabla `TokenID` para todos los tokens de shape usados por Open Rails, no solo los actuales.
9. Mejorar errores: incluir token, offset absoluto, parent block y bytes restantes.
10. Agregar soporte de fixtures binarios por bloque para tests chicos y deterministas.
11. Evaluar si conviene mantener la conversion a AST para compatibilidad o migrar `ShapeFile::from_path` a un parser binario tipado directo.

Ya resuelto:

- `vertex_idxs` ahora se interpreta como indice a la tabla `vertices` del `sub_object`.
- El viewer resuelve cada vertex a `point`, `normal` y primer UV antes de construir el mesh Bevy.
- Hay tests sobre fixtures binarios reales para validar tablas de vertices y conteo final de triangulos renderizables.
- `PrimState` conserva `tex_idxs`, `shader_idx`, `flags`, `ivtx_state` y `ZBias`; `ShapeFile` expone `shader_names`.
- El viewer puede construir partes de mesh agrupadas por `prim_state_idx`, con textura resuelta por parte.
- `train.rs`, `live.rs` y `world.rs` instancian esas partes como children con su material/textura propia.
- Los materiales usan `AlphaMode::Blend` cuando el `.ace` tiene alpha real o el nombre de textura sugiere vidrio/transparencia.

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
   - Hecho: `SubObject` conserva `vertices` con `point_idx`, `normal_idx` y `uv_indices`.
   - Hecho: `Primitive.vertex_indices` representa indices de vertices y el viewer los resuelve a punto/normal/UV.
   - Hecho: tests de malla sobre fixture binario real con conteo de vertices Bevy.
   - Pendiente: guardar flags/colores del vertex cuando el renderer los necesite.

5. **Materiales y transparencia**
   - Hecho: mapear `prim_state.texture_idx` desde `tex_idxs[0]`.
   - Hecho: exponer `shader_names` y conservar los `tex_idxs` completos por `PrimState`.
   - Hecho: agregar API de viewer para `LoadedShapePart` / mesh por `prim_state_idx`.
   - Hecho: conectar los spawners de escena para instanciar cada parte con su material.
   - Hecho: detectar alpha en `ACE.mip0` y aplicar `AlphaMode::Blend` de forma conservadora.
   - Pendiente: mapear flags reales de alpha/z-buffer desde `prim_state` / `vtx_state` / `light_model_cfgs`.

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

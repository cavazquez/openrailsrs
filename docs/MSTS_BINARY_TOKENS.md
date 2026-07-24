# Tokens binarios MSTS / Open Rails

Inventario del lector binario de `openrailsrs-formats`. La referencia de nombres e
IDs es `openrails/Source/Orts.Parsers.Msts/TokenID.cs`; el comportamiento efectivo
está en `crates/openrailsrs-formats/src/shape_binary.rs`.

## Qué significa cada estado

- **Admitido**: `is_known_binary_token` acepta el ID como cabecera de bloque y
  `token_name` puede emitir un nombre estable. Esto no implica que todas las
  extensiones del payload se conserven en el modelo tipado.
- **Especializado**: además de ser admitido, tiene lectura explícita de esquema o
  de tipos mixtos (`int` + `float`).
- **Genérico**: se admite estructuralmente, pero se emite como `_world` o
  `_unknown`; la capa tipada normalmente lo ignora.
- **No soportado**: el nombre existe en la tabla de Open Rails, pero el lector no
  reconoce el ID como subbloque. No debe confundirse con un token inexistente.
- **Desconocido**: no está en ninguno de los conjuntos admitidos. Un ID nuevo de
  Open Rails también cae en esta categoría hasta incorporarlo.

Un token no soportado dentro de un padre conocido no tiene compatibilidad
forward automática: puede consumirse como escalares residuales o descartarse al
cerrar el bloque. Por eso un token desconocido de geometría, material o animación
puede producir datos incompletos sin que el archivo entero falle.

## Offset

| Archivo | Offset | ID usado por el lector |
|---|---:|---|
| Shape binario `JINX0s*b` | 0 | ID core directamente |
| World binario `JINX0w*b` | 300 | `ID efectivo = ID raw + 300` |

Ejemplo: `Static` aparece como raw `3` en un `.w` y como ID efectivo `303`.

## Shape: tokens admitidos

El lector admite **93 IDs core**. Las tablas siguientes son exhaustivas para el
conjunto actual.

| IDs | Tokens |
|---|---|
| 1–4 | `comment`, `point`, `vector`, `quat` |
| 5–9 | `normals`, `normal_idxs`, `points`, `uv_point`, `uv_points` |
| 10–18 | `colour`, `colours`, `packed_colour`, `image`, `images`, `texture`, `textures`, `light_material`, `light_materials` |
| 19–24 | `linear_key`, `tcb_key`, `linear_pos`, `tcb_pos`, `slerp_rot`, `tcb_rot` |
| 25–30 | `controllers`, `anim_node`, `anim_nodes`, `animation`, `animations`, `anim` |
| 31–39 | `lod_controls`, `lod_control`, `distance_levels_header`, `distance_level_header`, `dlevel_selection`, `distance_levels`, `distance_level`, `sub_objects`, `sub_object` |
| 40–49 | `sub_object_header`, `geometry_info`, `geometry_nodes`, `geometry_node`, `geometry_node_map`, `cullable_prims`, `vtx_state`, `vtx_states`, `vertex`, `vertex_uvs` |
| 50–56 | `vertices`, `vertex_set`, `vertex_sets`, `primitives`, `prim_state`, `prim_states`, `prim_state_idx` |
| 60–61 | `indexed_trilist`, `tex_idxs` |
| 63–71 | `vertex_idxs`, `flags`, `matrix`, `matrices`, `hierarchy`, `volumes`, `vol_sphere`, `shape_header`, `shape` |
| 72–76 | `shader_names`, `shader_name`, `texture_filter_names`, `texture_filter_name`, `sort_vectors` |
| 79–89 | `light_model_cfgs`, `light_model_cfg`, `uv_ops`, `uvop_copy`, `uv_op_share`, `uv_op_copy`, `uv_op_uniformscale`, `uv_op_user_uninformscale`, `uv_op_nonuniformscale`, `uv_op_user_nonuninformscale`, `uv_op_transform` |
| 90–97 | `uv_op_user_transform`, `uv_op_reflectxy`, `uv_op_reflectmap`, `uv_op_reflectmapfull`, `uv_op_spheremap`, `uv_op_spheremapfull`, `uv_op_specularmap`, `uv_op_embossbump` |
| 125 | `named_filter_mode` |
| 129 | `named_shader` |

### Shape: lectura especializada

| Token | Tratamiento |
|---|---|
| `texture` (15) | Índices enteros, `MipMapLODBias` float y color opcional |
| `linear_key` (19) | `Frame:int` + 3 floats |
| `tcb_key` (20) | `Frame:int` + 9 floats |
| `slerp_rot` (23) | `Frame:int` + quaternion de 4 floats |
| `prim_state` (54) | Payload mixto, `tex_idxs`, Z-bias float y flags |
| `indexed_trilist` (60) | `vertex_idxs`, `normal_idxs` y `flags` |
| `shape` (71) | 17 hijos obligatorios + `animations` (29) opcional |
| Colecciones | Conteo y relación padre→hijo validados para arrays, LOD, geometría, materiales y animaciones |

Los keyframes correctos son `linear_key=19`, `tcb_key=20` y
`slerp_rot=23`. Los IDs `99`, `101` y `103` **no** son aliases de
animación.

## Shape: tokens core no soportados

Son **39 IDs** de la tabla core 0–131. Se conocen sus nombres oficiales, pero
`is_known_binary_token(..., 0)` los rechaza.

| IDs | Tokens |
|---|---|
| 0 | `error` (sentinela) |
| 57–59 | `indexed_point_list`, `point_list`, `indexed_line_list` |
| 62 | `tri` |
| 77–78 | `uvop_arg_sets`, `uvop_arg_set` |
| 98–103 | `user_uv_args`, `io_dev`, `io_map`, `sguid`, `dlev_cfg_table`, `dlev_cfg` |
| 104–110 | `subobject_shaders`, `subobject_light_cfgs`, `shape_named_data`, `shape_named_data_header`, `shape_named_geometry`, `shape_geom_ref`, `material_palette` |
| 111–119 | `blend_config`, `blend_config_header`, `filtermode_cfgs`, `filter_mode_cfg`, `blend_mode_cfgs`, `blend_mode_cfg`, `texture_stage_progs`, `texture_stage_prog`, `blend_mode_cfg_refs` |
| 120–124 | `shader_cfgs`, `shader_cfg`, `texture_slots`, `texture_slot`, `named_filter_modes` |
| 126–128 | `filtermode_cfg_refs`, `filtermode_cfg_ref`, `named_shaders` |
| 130–131 | `shader_cfg_refs`, `shader_cfg_ref` |

Cualquier shape ID fuera de 0–131 también es desconocido para este lector,
salvo que se agregue explícitamente.

## World: tokens admitidos con nombre

En WORLD se admiten estructuralmente **175 IDs efectivos**. De ellos, los
siguientes **68** tienen nombre específico:

| IDs efectivos | Tokens |
|---|---|
| 303, 305, 308, 311, 317 | `Static`, `TrackObj`, `Forest`, `CollideObject`, `Signal` |
| 360, 362, 364, 365 | `Platform`, `LevelCr`, `Speedpost`, `Hazard` |
| 375–376 | `Tr_Worldfile`, `Tr_Watermark` |
| 395–401 | `FileName`, `FileNames`, `Position`, `Direction`, `MaxVisDistance`, `Quality`, `StaticDetailLevel` |
| 404–405 | `StaticFlags`, `CollideFlags` |
| 408–410 | `UiD`, `TrackSections`, `TrackSection` |
| 419–420 | `SectionIdx`, `SectionCurve` |
| 424 | `JNodePosn` |
| 458 | `SignalSubObj` |
| 486–487 | `SignalUnits`, `SignalUnit` |
| 493 | `Elevation` |
| 500–501, 503 | `Population`, `Area`, `ScaleRange` |
| 579–580 | `ViewDbSphere`, `Radius` |
| 583–584 | `VDbId`, `VDbIdCount` |
| 598 | `Matrix3x3` |
| 922 | `TrItemId` |
| 945 | `QDirection` |
| 1107 | `PlatformData` |
| 1111–1114 | `SpeedRange`, `PickupType`, `PickupAnimData`, `PickupCapacity` |
| 1116–1117 | `CarFrequency`, `CarAvSpeed` |
| 1120 | `SidingData` |
| 1122–1124 | `LevelCrParameters`, `LevelCrData`, `LevelCrTiming` |
| 1131, 1134, 1139 | `Speed_Sign_Shape`, `Speed_Digit_Tex`, `Speed_Text_Size` |
| 1152–1155 | `Width`, `Height`, `TreeTexture`, `TreeSize` |
| 1531 | `CrashProbability` |
| 1540–1545 | `CarSpawner`, `Siding`, `Dyntrack`, `Transfer`, `Gantry`, `Pickup` |
| 1561–1563 | `Length`, `Flipped`, `Ruler` |

## World: admitidos pero genéricos/desconocidos

Todo ID efectivo **300–430** se acepta como posible subbloque WORLD. Los que no
figuran con nombre en la tabla anterior se emiten como `_world`:

`300–302`, `304`, `306–307`, `309–310`, `312–316`, `318–359`, `361`,
`363`, `366–374`, `377–394`, `402–403`, `406–407`, `411–418`,
`421–423`, `425–430`.

Además, `1115` y `1121` son admitidos por los rangos estructurales actuales,
pero `token_name` los emite como `_unknown`.

Estos **107 IDs** son compatibilidad estructural, no soporte semántico. La capa
tipada no debe depender de sus datos.

## World: IDs rechazados

Fuera de 300–430, sólo se admiten los IDs enumerados en la tabla WORLD con
nombre más `1115` y `1121`. Por lo tanto, los rangos rechazados son:

`431–457`, `459–485`, `488–492`, `494–499`, `502`, `504–578`,
`581–582`, `585–597`, `599–921`, `923–944`, `946–1106`,
`1108–1110`, `1118–1119`, `1125–1130`, `1132–1133`, `1135–1138`,
`1140–1151`, `1156–1530`, `1532–1539`, `1546–1560` y todo ID
mayor que `1563`.

## Cómo incorporar un token

1. Confirmar el ID y el layout en `TokenID.cs` y en el lector de formato de Open
   Rails; el nombre por sí solo no describe los tipos del payload.
2. Agregar el nombre a `token_name`.
3. Agregar el ID a `is_known_binary_token` sólo cuando se conozcan sus límites.
4. Si es colección, declarar la relación en
   `is_schema_collection_parent`/`is_expected_collection_child`.
5. Si mezcla enteros, floats o strings, crear un lector especializado; no usar la
   inferencia escalar genérica.
6. Modelarlo en `typed/*` si el dato debe llegar al simulador o al viewer.
7. Añadir una fixture binaria mínima y, cuando exista contenido local, una
   regresión sobre un archivo real.

Pruebas relevantes:

```bash
cargo test -p openrailsrs-formats shape_binary
cargo test -p openrailsrs-formats shapes_world
```

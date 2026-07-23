# Transformaciones: DirectX / MSTS → Bevy (glam)

Mapa para quien viene de C++ (DirectXMath / XNA / Open Rails) y trabaja poses en `viewer3d`.

Espacios world/render/view: [`MSTS_COORDINATES.md`](MSTS_COORDINATES.md). Shear Affine (#139): [`VIEWER3D.md`](VIEWER3D.md).

## Tipos

| Concepto | C++ (DirectXMath) | Rust (Bevy / glam) |
|----------|-------------------|--------------------|
| Vector 3 | `XMVECTOR` / `XMFLOAT3` | `Vec3` |
| Cuaternión | `XMVECTOR` / `XMFLOAT4` | `Quat` |
| Matriz 4×4 | `XMMATRIX` / `XMFLOAT4X4` | `Mat4` (también en `GlobalTransform`) |
| Matriz 3×3 lineal | `XMFLOAT3X3` / filas MSTS | `Mat3` (`WorldObject.linear`, GPU instance) |

## Declarativo vs matriz a mano

En DirectX solías armar el world matrix cada frame (escala → rotación → traslación) y subir un `XMMATRIX`.

En Bevy declarás `Transform` en la entidad; el pipeline arma `GlobalTransform` (jerarquía de padres incluida):

```rust
commands.spawn((
    Mesh3d(mesh),
    MeshMaterial3d(material),
    Transform {
        translation: Vec3::new(10.0, 0.0, 0.0),
        rotation: Quat::from_rotation_y(45_f32.to_radians()),
        scale: Vec3::splat(2.0),
    },
));
```

Para offsets locales / cámara, `Transform::mul_transform`, `GlobalTransform::compute_matrix()`, o `Mat4` a mano.

## Orden de multiplicación (vectores columna)

Bevy/glam usan **vectores columna**:

- `p' = M * p` (no `p * M` como el layout habitual de DirectX row-vector).
- Componer a mano (derecha → izquierda): traslación × rotación × escala:

```rust
let world = Mat4::from_translation(t) * Mat4::from_quat(r) * Mat4::from_scale(s);
```

Equivale al TRS que Bevy guarda en `Transform` y luego convierte a matriz.

## MSTS / Open Rails en este repo

1. **`.w` `Matrix3x3`:** layout fila MSTS/XNA (ejes locales). Conversión en `matrix3x3_to_rotation_scale` / `coordinates` — no asumir que el `Mat3` glam “tal cual” es el array del archivo sin pasar por esos helpers.
2. **`Transform` es solo TRS:** escala no uniforme + rotación sí; **shear se pierde**. Detección: `linear_requires_affine` (TRS no round-trip) — **no** usar `linear.is_some()` (casi todo Matrix3x3 lo tiene; forzar GPU rompió pilares/puentes, #174).
3. **Shear real:** GPU instance `Mat4` cuando N≥4; si no, **bake** del `Mat3` en el mesh + `Transform` solo traslación (#139).
4. **QDirection** sin `Matrix3x3` → cuaternión + escala 1 (o la del patch); no hay `linear`.
5. **Hijos (`with_children`):** pose del coche/cabina en el padre; partes/meshes en hijos con `Transform::default()` salvo animación local.

## Checklist rápido

- ¿Solo rotación/escala/traslación? → `Transform` / `GlobalTransform`.
- ¿Shear del `.w`? → affine / instance `Mat4`, no confiar en TRS entity.
- ¿Espacio de cámara vs tile? → restar floating origin ([`MSTS_COORDINATES.md`](MSTS_COORDINATES.md)); no mezclar MSTS absoluto con view.

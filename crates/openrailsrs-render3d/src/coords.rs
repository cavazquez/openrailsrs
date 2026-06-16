//! Conversiones de coordenadas MSTS/XNA → Bevy para shapes `.s`.
//!
//! Replicadas (mínimas) de la convención de Open Rails: las posiciones de shape
//! niegan Z, y la jerarquía de matrices `Matrix43` se aplica con la convención
//! XNA (términos en Z negados de forma consistente).

use bevy::math::Vec3;
use openrailsrs_formats::{Matrix43, Vec3 as ShapeVec3};

/// Punto de shape MSTS (`.s`) → espacio de malla Bevy: Z negada.
#[inline]
pub fn shape_point_to_bevy(v: ShapeVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, -(v.z as f32))
}

/// Transforma un punto por un nivel de la jerarquía `Matrix43` (convención XNA
/// de Open Rails `Shapes.cs`). Con `zero_translation` se ignora la 4ª fila.
pub fn matrix43_transform_point_xna(m: &Matrix43, p: Vec3, zero_translation: bool) -> Vec3 {
    let r = &m.rows;
    let d = if zero_translation {
        [0.0, 0.0, 0.0]
    } else {
        r[3]
    };
    Vec3::new(
        p.x * r[0][0] as f32 + p.y * r[1][0] as f32 - p.z * r[2][0] as f32 + d[0] as f32,
        p.x * r[0][1] as f32 + p.y * r[1][1] as f32 - p.z * r[2][1] as f32 + d[1] as f32,
        -p.x * r[0][2] as f32 - p.y * r[1][2] as f32 + p.z * r[2][2] as f32 - d[2] as f32,
    )
}

/// Transforma un vector dirección (sin traslación) por un nivel `Matrix43`.
pub fn matrix43_transform_vector_xna(m: &Matrix43, p: Vec3) -> Vec3 {
    let r = &m.rows;
    Vec3::new(
        p.x * r[0][0] as f32 + p.y * r[1][0] as f32 - p.z * r[2][0] as f32,
        p.x * r[0][1] as f32 + p.y * r[1][1] as f32 - p.z * r[2][1] as f32,
        -p.x * r[0][2] as f32 - p.y * r[1][2] as f32 + p.z * r[2][2] as f32,
    )
}

/// Offset local MSTS (+Z adelante) → vector Bevy (Z negada).
#[inline]
pub fn msts_local_offset_to_bevy(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, -z)
}

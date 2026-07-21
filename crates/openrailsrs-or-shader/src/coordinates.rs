//! Single source of truth for all MSTS/OpenRails → Bevy coordinate conversions.
//!
//! ## MSTS coordinate system
//!
//! MSTS uses a right-handed, Y-up coordinate system where:
//! - **X** = east (positive east)
//! - **Y** = up (positive up)
//! - **Z** = away from the viewer in MSTS camera convention (positive south in some places,
//!   but tile-local Z uses "screen-forward" conventions per Microsoft XNA)
//!
//! **Tile layout**: the world is divided into tiles of 2048 m × 2048 m.  Tile numbers are
//! signed ("internal") values, negative X for UK routes (e.g. tile_x = -6084).  The signs are
//! written into `.w` filenames too (`w-006084+014923.w` → tile (-6084, 14923), exactly as
//! Open Rails parses them in `WorldFile.cs`).
//!
//! Tile-local positions are centred: (0, 0) = tile centre, range roughly ±1024 m in X and Z.
//!
//! ## Bevy coordinate system
//!
//! Bevy uses a right-handed, Y-up system where the default camera looks toward **−Z**.
//!
//! ## MSTS → Bevy world-space conversion
//!
//! Follows Open Rails XNA convention (`Scenery.cs` / `Shapes.cs`): the **whole-world** MSTS Z
//! is negated:
//!
//! ```text
//! bevy_x = tile_x * 2048 + local_x            (signed internal tile X)
//! bevy_y = local_y                            (Y up, unchanged)
//! bevy_z = -(tile_z * 2048 + local_z)         (whole-world Z negation, same as XNA)
//! ```
//!
//! Negating only the local part (or using positive "display" tile numbers) would mirror the
//! tile grid and break continuity at every tile border.
//!
//! ## Shape-local coordinates
//!
//! MSTS `.s` vertices / normals also flip Z:
//! ```text
//! bevy_pos = (x, y, -z)
//! ```
//! Internal `Matrix43` multiplication in the XNA convention negates the Z terms consistently.
//!
//! ## Quaternion / Matrix3×3 from `.w`
//!
//! Open Rails converts `QDirection` by negating the Z component:
//! ```text
//! quat = (qx, qy, -qz, qw)
//! ```
//! `Matrix3×3` uses the same XNA convention; see [`matrix3x3_to_rotation_scale`].

use bevy::math::{Mat3, Quat, Vec3};
use bevy::prelude::Transform;
use openrailsrs_formats::{Matrix43, ShapeFile, Vec3 as ShapeVec3};

/// MSTS tile size (metres), equal in X and Z.
pub const MSTS_TILE_SIZE_M: f64 = 2048.0;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A position in the MSTS world reference frame:
/// signed internal tile numbers plus tile-local offset.
///
/// Created from `.w` world-file items, `.tdb` track nodes, or tsection anchors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MstsWorldPosition {
    /// East tile index (signed internal convention: negative for UK routes).
    pub tile_x: i32,
    /// North tile index (signed internal convention).
    pub tile_z: i32,
    /// East offset from tile centre (metres).
    pub x: f64,
    /// Elevation above MSL (metres).
    pub y: f64,
    /// Offset along tile-local Z axis, **before** the Bevy Z-flip (metres).
    pub z: f64,
}

/// A position in Bevy world space, derived from [`MstsWorldPosition`] via [`msts_to_bevy`].
///
/// Large absolute values (millions of metres) are typical for real routes; callers
/// should subtract [`crate::world::RouteFocus::center`] before spawning Bevy entities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BevyWorldPosition(pub Vec3);

impl BevyWorldPosition {
    /// Raw `Vec3` in Bevy world space.
    #[inline]
    pub fn as_vec3(self) -> Vec3 {
        self.0
    }
}

// ── World-space conversion ────────────────────────────────────────────────────

/// Convert an MSTS tile-local position to Bevy absolute world space.
///
/// Tile X / Z must use the **signed internal** convention (negative X for UK routes,
/// exactly as parsed from `.w` filenames and `.tdb` nodes).
///
/// Matches the Open Rails XNA convention (whole-world Z negation):
/// ```text
/// bevy_x = tile_x * 2048 + x
/// bevy_y = y
/// bevy_z = -(tile_z * 2048 + z)   ← whole-world Z flip
/// ```
#[inline]
pub fn msts_to_bevy(pos: MstsWorldPosition) -> BevyWorldPosition {
    BevyWorldPosition(Vec3::new(
        (pos.tile_x as f64 * MSTS_TILE_SIZE_M + pos.x) as f32,
        pos.y as f32,
        (-(pos.tile_z as f64 * MSTS_TILE_SIZE_M + pos.z)) as f32,
    ))
}

/// Thin wrapper accepting the raw tile + [`openrailsrs_formats::Vec3`] fields used
/// across `world.rs` and `tdb_track.rs`.
#[inline]
pub fn msts_tile_local_to_bevy(tile_x: i32, tile_z: i32, local: openrailsrs_formats::Vec3) -> Vec3 {
    msts_to_bevy(MstsWorldPosition {
        tile_x,
        tile_z,
        x: local.x,
        y: local.y,
        z: local.z,
    })
    .as_vec3()
}

// ── Shape-local coordinates ───────────────────────────────────────────────────

/// Convert an MSTS shape-local point (`.s` vertex) to Bevy mesh space.
///
/// Matches Open Rails `XNAVertexPositionNormalTextureFromMSTS` (`Shapes.cs`):
/// Z is negated; X and Y are unchanged.
#[inline]
pub fn shape_point_to_bevy(v: ShapeVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, -(v.z as f32))
}

/// Convert an MSTS shape-local vector (normal / direction) to Bevy mesh space.
///
/// Same Z flip as [`shape_point_to_bevy`]; no translation component.
#[inline]
pub fn shape_vec_to_bevy(v: ShapeVec3) -> Vec3 {
    shape_point_to_bevy(v)
}

/// Convert an already-loaded MSTS `Vec3` (XYZ floats read from `.eng` / `ORTS3DCabHeadPos`
/// etc.) to Bevy mesh space.  Use when the raw `ShapeVec3` is not available.
#[inline]
pub fn msts_shape_vec3_to_bevy(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, -v.z)
}

/// A local-space vector in MSTS tsection / shape space that needs a Z flip
/// before being used as a Bevy world-space offset.
///
/// Use [`msts_local_offset_to_bevy`] instead of constructing a raw `Vec3` from
/// `shape_local_offset[*]` fields, which live in MSTS-convention (+Z forward).
#[inline]
pub fn msts_local_offset_to_bevy(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, -z)
}

// ── Rotation conversions ──────────────────────────────────────────────────────

/// Convert an MSTS `QDirection` `[qx, qy, qz, qw]` to a Bevy `Quat`.
///
/// Follows Open Rails XNA convention (`Scenery.cs`): negate the Z component.
pub fn qdir_to_quat(qdir: &[f64; 4]) -> Quat {
    Quat::from_xyzw(
        qdir[0] as f32,
        qdir[1] as f32,
        -(qdir[2] as f32),
        qdir[3] as f32,
    )
}

/// Decompose an MSTS `Matrix3×3` into a Bevy `Quat` and a non-uniform scale `Vec3`.
///
/// Follows Open Rails XNA convention (`Scenery.cs`): each column's Z component is negated,
/// and the third column's X/Y components are negated.
///
/// Scale components are **signed**: when the XNA matrix has `det < 0` (reflection / mirrored
/// placement), the reflection is absorbed into the Z scale axis so
/// `Mat3::from_quat(rot) * Mat3::from_diagonal(scale)` round-trips the affine linear part
/// within ~`1e-4` (see unit tests).
///
/// Row-major storage: `m[0..3]` = first row, etc.
pub fn matrix3x3_to_rotation_scale(m: &[f64; 9]) -> (Quat, Vec3) {
    let raw = matrix3x3_to_xna_mat3(m);
    let sx = raw.x_axis.length().max(1e-6);
    let sy = raw.y_axis.length().max(1e-6);
    let mut sz = raw.z_axis.length().max(1e-6);
    let x = raw.x_axis / sx;
    let y = raw.y_axis / sy;
    let mut z = raw.z_axis / sz;
    // Column lengths alone lose reflections (det < 0). Flip one axis and negate its scale
    // so the rotation stays proper (det ≈ +1) while scale carries the mirror.
    if Mat3::from_cols(x, y, z).determinant() < 0.0 {
        z = -z;
        sz = -sz;
    }
    let rot = Quat::from_mat3(&Mat3::from_cols(x, y, z));
    (rot, Vec3::new(sx, sy, sz))
}

/// Extract only the rotation from an MSTS `Matrix3×3`.
pub fn matrix3x3_to_quat(m: &[f64; 9]) -> Quat {
    matrix3x3_to_rotation_scale(m).0
}

fn matrix3x3_to_xna_mat3(m: &[f64; 9]) -> Mat3 {
    // Open Rails XNA convention: negate Z-component of X/Y cols and negate X/Y of Z col.
    Mat3::from_cols(
        Vec3::new(m[0] as f32, m[1] as f32, -(m[2] as f32)),
        Vec3::new(m[3] as f32, m[4] as f32, -(m[5] as f32)),
        Vec3::new(-(m[6] as f32), -(m[7] as f32), m[8] as f32),
    )
}

// ── Matrix43 (shape hierarchy) ────────────────────────────────────────────────

/// Transform a shape-space point through one level of the MSTS `Matrix43` hierarchy.
///
/// Implements the XNA convention used by Open Rails `Shapes.cs`:
/// - X/Y columns: normal dot product
/// - Z column and translation row: negated Z terms
/// - When `zero_translation` is true the fourth row is ignored (used at the root matrix).
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

/// Transform a direction vector (no translation) through one `Matrix43` level.
pub fn matrix43_transform_vector_xna(m: &Matrix43, p: Vec3) -> Vec3 {
    let r = &m.rows;
    Vec3::new(
        p.x * r[0][0] as f32 + p.y * r[1][0] as f32 - p.z * r[2][0] as f32,
        p.x * r[0][1] as f32 + p.y * r[1][1] as f32 - p.z * r[2][1] as f32,
        -p.x * r[0][2] as f32 - p.y * r[1][2] as f32 + p.z * r[2][2] as f32,
    )
}

/// MSTS `Matrix43` → Bevy `Transform` (Open Rails XNA column basis + Z flip).
pub fn matrix43_to_transform(m: &Matrix43) -> Transform {
    let r = &m.rows;
    let basis = Mat3::from_cols(
        Vec3::new(r[0][0] as f32, r[0][1] as f32, -r[0][2] as f32),
        Vec3::new(r[1][0] as f32, r[1][1] as f32, -r[1][2] as f32),
        Vec3::new(-r[2][0] as f32, -r[2][1] as f32, r[2][2] as f32),
    );
    let translation = Vec3::new(r[3][0] as f32, r[3][1] as f32, -r[3][2] as f32);
    Transform {
        translation,
        rotation: Quat::from_mat3(&basis),
        scale: Vec3::ONE,
    }
}

/// True when LOD0 hierarchy marks matrix 0 as root (`hierarchy[0] == -1`).
///
/// Cab mesh bake uses `zero_translation` on that root so rebased lever poses must match.
fn shape_zero_root_translation(shape: &ShapeFile) -> bool {
    shape
        .lod_controls
        .first()
        .and_then(|lc| lc.distance_levels.first())
        .is_some_and(|level| level.hierarchy.first().copied() == Some(-1))
}

fn matrix43_to_transform_cab(m: &Matrix43, zero_translation: bool) -> Transform {
    if !zero_translation {
        return matrix43_to_transform(m);
    }
    let mut copy = *m;
    copy.rows[3] = [0.0, 0.0, 0.0];
    matrix43_to_transform(&copy)
}

/// Matrix chain from `leaf` to root — same walk order as shape mesh bake.
fn cab_matrix_chain<'a>(
    shape: &'a ShapeFile,
    leaf: i32,
    pose_mats: &'a [Matrix43],
    zero_root_translation: bool,
) -> Vec<(&'a Matrix43, bool)> {
    let Some(level) = shape
        .lod_controls
        .first()
        .and_then(|lc| lc.distance_levels.first())
    else {
        return pose_mats
            .get(leaf as usize)
            .map(|m| (m, false))
            .into_iter()
            .collect();
    };
    let mut out = Vec::new();
    let mut matrix_idx = leaf;
    let mut guard = 0usize;
    while matrix_idx >= 0 && guard < shape.matrices.len() {
        let idx = matrix_idx as usize;
        if let Some(m) = pose_mats.get(idx) {
            out.push((m, zero_root_translation && idx == 0));
        }
        matrix_idx = level.hierarchy.get(idx).copied().unwrap_or(-1);
        guard += 1;
    }
    out
}

fn transform_point_xna_chain(mut point: Vec3, chain: &[(&Matrix43, bool)]) -> Vec3 {
    for (matrix, zero_translation) in chain {
        point = matrix43_transform_point_xna(matrix, point, *zero_translation);
    }
    point
}

/// Build a Bevy transform that matches [`transform_shape_point`] / cab mesh bake.
fn transform_from_xna_matrix_chain(chain: &[(&Matrix43, bool)]) -> Transform {
    if chain.is_empty() {
        return Transform::IDENTITY;
    }
    let origin = transform_point_xna_chain(Vec3::ZERO, chain);
    let x = transform_point_xna_chain(Vec3::X, chain) - origin;
    let y = transform_point_xna_chain(Vec3::Y, chain) - origin;
    let z = transform_point_xna_chain(Vec3::Z, chain) - origin;
    let basis = Mat3::from_cols(
        x.try_normalize().unwrap_or(Vec3::X),
        y.try_normalize().unwrap_or(Vec3::Y),
        z.try_normalize().unwrap_or(Vec3::Z),
    );
    Transform {
        translation: origin,
        rotation: Quat::from_mat3(&basis),
        scale: Vec3::ONE,
    }
}

/// Walk shape hierarchy from `leaf` to root, multiplying pose matrices (OR `PrepareFrame` order).
pub fn hierarchy_chain_transform(
    shape: &ShapeFile,
    leaf: usize,
    pose_mats: &[Matrix43],
) -> Transform {
    hierarchy_chain_transform_inner(shape, leaf, pose_mats, false)
}

/// Cab lever pose: same chain order and XNA math as cab mesh bake.
pub fn hierarchy_chain_transform_cab(
    shape: &ShapeFile,
    leaf: usize,
    pose_mats: &[Matrix43],
) -> Transform {
    let chain = cab_matrix_chain(
        shape,
        leaf as i32,
        pose_mats,
        shape_zero_root_translation(shape),
    );
    transform_from_xna_matrix_chain(&chain)
}

fn hierarchy_chain_transform_inner(
    shape: &ShapeFile,
    leaf: usize,
    pose_mats: &[Matrix43],
    zero_root_translation: bool,
) -> Transform {
    let level = shape
        .lod_controls
        .first()
        .and_then(|lc| lc.distance_levels.first());
    let Some(level) = level else {
        return pose_mats
            .get(leaf)
            .map(|m| matrix43_to_transform_cab(m, false))
            .unwrap_or(Transform::IDENTITY);
    };
    let mut hi = leaf as i32;
    let mut chain = Vec::new();
    let mut guard = 0usize;
    while hi >= 0 && guard < shape.matrices.len() {
        let idx = hi as usize;
        if let Some(m) = pose_mats.get(idx) {
            let zero_trans = zero_root_translation && idx == 0;
            chain.push(matrix43_to_transform_cab(m, zero_trans));
        }
        hi = level.hierarchy.get(idx).copied().unwrap_or(-1);
        guard += 1;
    }
    chain
        .into_iter()
        .reduce(|acc, t| t * acc)
        .unwrap_or(Transform::IDENTITY)
}

/// Static rest pose for a cab bone (shape file matrices, no animation).
pub fn static_hierarchy_chain_transform(shape: &ShapeFile, leaf: usize) -> Transform {
    let mats: Vec<Matrix43> = shape.matrices.iter().map(|m| m.matrix).collect();
    hierarchy_chain_transform(shape, leaf, &mats)
}

/// Static cab bone pose aligned with cab mesh bake (`zero_translation` on matrix 0).
pub fn static_hierarchy_chain_transform_cab(shape: &ShapeFile, leaf: usize) -> Transform {
    let mats: Vec<Matrix43> = shape.matrices.iter().map(|m| m.matrix).collect();
    hierarchy_chain_transform_cab(shape, leaf, &mats)
}

/// Re-express baked cab vertices in bone-local space so entity `Transform` can carry the pose.
pub fn rebase_points_to_bone_local(points: &mut [Vec3], bone_world: Transform) {
    let inv = bone_world.rotation.inverse();
    for p in points {
        *p = inv * (*p - bone_world.translation);
    }
}

pub fn rebase_vectors_to_bone_local(vectors: &mut [Vec3], bone_world: Transform) {
    let inv = bone_world.rotation.inverse();
    for v in vectors {
        *v = (inv * *v).try_normalize().unwrap_or(Vec3::Y);
    }
}

// ── Track / graph coordinates ─────────────────────────────────────────────────

/// Map track-graph planar coordinates to Bevy world space (flat, Y = 0).
///
/// Track-graph nodes are stored in `track.toml` as `x_m` / `y_m` where:
/// - `x_m` = east in Bevy world (already converted from MSTS display-tile convention)
/// - `y_m` = north in Bevy world (already Z-flipped during route import)
///
/// The graph therefore uses Bevy conventions directly; no further flip is needed here.
#[inline]
pub fn graph_to_world(x_m: f64, y_m: f64) -> Vec3 {
    Vec3::new(x_m as f32, 0.0, y_m as f32)
}

/// Yaw angle (radians) for a Bevy `Quat::from_rotation_y(yaw)` transform so that
/// an MSTS train shape faces the direction `(dx, dz)` in Bevy world XZ.
///
/// MSTS shapes face +Z locally; `msts_shape_to_train_rotation` applies a +π/2 base
/// rotation so they effectively face +X in Bevy.  Given that base, the correct yaw is:
/// ```text
/// yaw = atan2(-dz, dx)
/// ```
/// which satisfies `shape_forward = (cos yaw, 0, -sin yaw) = normalize(dx, 0, dz)`.
#[inline]
pub fn train_yaw_from_direction(dx: f32, dz: f32) -> f32 {
    (-dz).atan2(dx)
}

/// Yaw angle (radians) for a procedural track segment whose local forward is **+Z**.
///
/// Segmentos procedurales (Dyntrack, TDB) avanzan en +Z local, así que:
/// ```text
/// yaw = atan2(dx, dz)
/// ```
#[inline]
pub fn track_segment_yaw_from_direction(dx: f32, dz: f32) -> f32 {
    dx.atan2(dz)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::Vec3 as FVec3;

    // ── msts_to_bevy ──────────────────────────────────────────────────────────

    #[test]
    fn msts_to_bevy_tile_zero_uses_local_coords() {
        let p = msts_to_bevy(MstsWorldPosition {
            tile_x: 0,
            tile_z: 0,
            x: 100.0,
            y: 5.0,
            z: -3.0,
        });
        // Z flip: bevy_z = -(0*2048 + (-3)) = 3
        assert_eq!(p.as_vec3(), Vec3::new(100.0, 5.0, 3.0));
    }

    #[test]
    fn msts_to_bevy_tile_offset_scales_by_2048() {
        let p = msts_to_bevy(MstsWorldPosition {
            tile_x: 2,
            tile_z: 1,
            x: 10.0,
            y: 0.0,
            z: 20.0,
        });
        // x = 2*2048+10 = 4106; z = -(1*2048+20) = -2068
        assert_eq!(p.as_vec3(), Vec3::new(4106.0, 0.0, -2068.0));
    }

    #[test]
    fn msts_to_bevy_is_continuous_across_tile_borders() {
        // East edge of tile (-6084, 0) == west edge of tile (-6083, 0).
        let a = msts_to_bevy(MstsWorldPosition {
            tile_x: -6084,
            tile_z: 0,
            x: 1024.0,
            y: 0.0,
            z: 0.0,
        });
        let b = msts_to_bevy(MstsWorldPosition {
            tile_x: -6083,
            tile_z: 0,
            x: -1024.0,
            y: 0.0,
            z: 0.0,
        });
        assert!((a.as_vec3() - b.as_vec3()).length() < 1e-3);
        // North edge of tile (0, 14923) == south edge of tile (0, 14924).
        let c = msts_to_bevy(MstsWorldPosition {
            tile_x: 0,
            tile_z: 14923,
            x: 0.0,
            y: 0.0,
            z: 1024.0,
        });
        let d = msts_to_bevy(MstsWorldPosition {
            tile_x: 0,
            tile_z: 14924,
            x: 0.0,
            y: 0.0,
            z: -1024.0,
        });
        assert!((c.as_vec3() - d.as_vec3()).length() < 1e-3);
    }

    #[test]
    fn msts_to_bevy_y_unchanged() {
        let p = msts_to_bevy(MstsWorldPosition {
            tile_x: 0,
            tile_z: 0,
            x: 0.0,
            y: 42.5,
            z: 0.0,
        });
        assert!((p.as_vec3().y - 42.5).abs() < 1e-4);
    }

    #[test]
    fn msts_to_bevy_positive_local_z_gives_negative_bevy_z_relative_to_tile_origin() {
        // MSTS local z=+100 (north) → Bevy z = -(0*2048 + 100) = -100
        let p = msts_to_bevy(MstsWorldPosition {
            tile_x: 0,
            tile_z: 0,
            x: 0.0,
            y: 0.0,
            z: 100.0,
        });
        assert!((p.as_vec3().z - (-100.0_f32)).abs() < 1e-2);
    }

    #[test]
    fn msts_tile_local_to_bevy_matches_msts_to_bevy() {
        let local = FVec3 {
            x: 10.0,
            y: 5.0,
            z: 20.0,
        };
        let via_struct = msts_to_bevy(MstsWorldPosition {
            tile_x: 2,
            tile_z: 1,
            x: local.x,
            y: local.y,
            z: local.z,
        })
        .as_vec3();
        let via_fn = msts_tile_local_to_bevy(2, 1, local);
        assert_eq!(via_struct, via_fn);
    }

    // ── shape_point_to_bevy ───────────────────────────────────────────────────

    #[test]
    fn shape_point_z_is_negated() {
        let p = shape_point_to_bevy(ShapeVec3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        });
        assert_eq!(p, Vec3::new(1.0, 2.0, -3.0));
    }

    #[test]
    fn shape_point_xy_unchanged() {
        let p = shape_point_to_bevy(ShapeVec3 {
            x: 5.0,
            y: -1.0,
            z: 0.0,
        });
        assert_eq!(p, Vec3::new(5.0, -1.0, 0.0));
    }

    #[test]
    fn msts_local_offset_to_bevy_negates_z() {
        // tsection shape_local_offset: (0, 0, 5) in MSTS → (0, 0, -5) in Bevy
        let v = msts_local_offset_to_bevy(0.0, 0.0, 5.0);
        assert_eq!(v, Vec3::new(0.0, 0.0, -5.0));
    }

    // ── qdir_to_quat ─────────────────────────────────────────────────────────

    #[test]
    fn qdir_identity_stays_identity() {
        let q = qdir_to_quat(&[0.0, 0.0, 0.0, 1.0]);
        assert!((q.x).abs() < 1e-5);
        assert!((q.y).abs() < 1e-5);
        assert!((q.z).abs() < 1e-5);
        assert!((q.w - 1.0).abs() < 1e-5);
    }

    #[test]
    fn qdir_z_component_negated() {
        let q = qdir_to_quat(&[0.1, 0.2, 0.3, 0.9]);
        assert!((q.x - 0.1).abs() < 1e-5);
        assert!((q.y - 0.2).abs() < 1e-5);
        assert!((q.z + 0.3).abs() < 1e-5, "expected z=-0.3, got {}", q.z);
        assert!((q.w - 0.9).abs() < 1e-5);
    }

    // ── matrix3x3 ────────────────────────────────────────────────────────────

    #[test]
    fn matrix3x3_identity_gives_identity_quat() {
        let m = [1.0_f64, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let q = matrix3x3_to_quat(&m);
        assert!(
            (q.w.abs() - 1.0).abs() < 1e-4,
            "expected |w|=1, got {}",
            q.w
        );
    }

    #[test]
    fn matrix3x3_rotation_scale_decomposition_round_trips() {
        // A pure 90° Y rotation in MSTS convention.
        let m = [0.0_f64, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0];
        let (rot, scale) = matrix3x3_to_rotation_scale(&m);
        assert!((scale.x - 1.0).abs() < 1e-4);
        assert!((scale.y - 1.0).abs() < 1e-4);
        assert!((scale.z - 1.0).abs() < 1e-4);
        // The quaternion should be a valid unit quaternion.
        assert!((rot.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn matrix3x3_preserves_non_uniform_scale_2_1_1() {
        // Columns already in MSTS row-major; XNA path will flip Z terms.
        // Build a Bevy linear map with scale (2,1,1), convert back through XNA inverse
        // so the public API sees an MSTS Matrix3×3.
        let bevy = Mat3::from_diagonal(Vec3::new(2.0, 1.0, 1.0));
        let m = bevy_mat3_to_msts_matrix3x3(bevy);
        let (rot, scale) = matrix3x3_to_rotation_scale(&m);
        assert!(
            (scale.x.abs() - 2.0).abs() < 1e-3 && (scale.y.abs() - 1.0).abs() < 1e-3,
            "scale={scale:?}"
        );
        assert!((scale.z.abs() - 1.0).abs() < 1e-3, "scale={scale:?}");
        let rebuilt = Mat3::from_quat(rot) * Mat3::from_diagonal(scale);
        assert_mat3_close(bevy, rebuilt, 1e-3);
    }

    #[test]
    fn matrix3x3_preserves_negative_determinant_reflection() {
        // Mirror in X (det < 0) with unit lengths — must keep a signed scale axis.
        let bevy = Mat3::from_cols(
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        );
        assert!(bevy.determinant() < 0.0);
        let m = bevy_mat3_to_msts_matrix3x3(bevy);
        let (rot, scale) = matrix3x3_to_rotation_scale(&m);
        assert!(
            scale.x * scale.y * scale.z < 0.0,
            "reflection must yield signed scale product < 0, got {scale:?}"
        );
        let rebuilt = Mat3::from_quat(rot) * Mat3::from_diagonal(scale);
        assert_mat3_close(bevy, rebuilt, 1e-3);
    }

    /// Inverse of [`matrix3x3_to_xna_mat3`] for unit tests (row-major MSTS Matrix3×3).
    fn bevy_mat3_to_msts_matrix3x3(bevy: Mat3) -> [f64; 9] {
        // matrix3x3_to_xna_mat3:
        //   X col: (m0, m1, -m2)  Y col: (m3, m4, -m5)  Z col: (-m6, -m7, m8)
        let x = bevy.x_axis;
        let y = bevy.y_axis;
        let z = bevy.z_axis;
        [
            f64::from(x.x),
            f64::from(x.y),
            f64::from(-x.z),
            f64::from(y.x),
            f64::from(y.y),
            f64::from(-y.z),
            f64::from(-z.x),
            f64::from(-z.y),
            f64::from(z.z),
        ]
    }

    fn assert_mat3_close(a: Mat3, b: Mat3, eps: f32) {
        for (i, (ca, cb)) in [a.x_axis, a.y_axis, a.z_axis]
            .into_iter()
            .zip([b.x_axis, b.y_axis, b.z_axis])
            .enumerate()
        {
            let d = (ca - cb).length();
            assert!(
                d < eps,
                "col {i} diff {d} (eps {eps}): {ca:?} vs {cb:?}"
            );
        }
    }

    // ── train / track yaw ────────────────────────────────────────────────────

    #[test]
    fn train_yaw_facing_plus_x() {
        // Moving east (+X): shape (base +π/2) should face +X.
        let yaw = train_yaw_from_direction(1.0, 0.0);
        // forward = (cos yaw, 0, -sin yaw) = (cos 0, 0, 0) = (1,0,0) ✓
        let fwd_x = yaw.cos();
        let fwd_z = -yaw.sin();
        assert!((fwd_x - 1.0).abs() < 1e-5, "fwd_x={fwd_x}");
        assert!(fwd_z.abs() < 1e-5, "fwd_z={fwd_z}");
    }

    #[test]
    fn train_yaw_facing_plus_z() {
        // Moving north (Bevy +Z): yaw should make shape face +Z.
        let yaw = train_yaw_from_direction(0.0, 1.0);
        let fwd_x = yaw.cos();
        let fwd_z = -yaw.sin();
        assert!(fwd_x.abs() < 1e-5, "fwd_x={fwd_x}");
        assert!((fwd_z - 1.0).abs() < 1e-5, "fwd_z={fwd_z}");
    }

    #[test]
    fn track_segment_yaw_facing_plus_z() {
        // Segment moving in +Z: yaw=0 gives forward (sin 0, 0, cos 0)=(0,0,1)=+Z ✓
        let yaw = track_segment_yaw_from_direction(0.0, 1.0);
        assert!(yaw.abs() < 1e-5, "yaw={yaw}");
    }

    #[test]
    fn track_segment_yaw_facing_plus_x() {
        // Segment moving in +X: yaw=π/2 gives forward (sin π/2, 0, cos π/2)=(1,0,0)=+X ✓
        let yaw = track_segment_yaw_from_direction(1.0, 0.0);
        assert!(
            (yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
            "yaw={yaw}"
        );
    }

    // ── graph_to_world ────────────────────────────────────────────────────────

    #[test]
    fn graph_to_world_maps_x_m_to_x_y_m_to_z() {
        let p = graph_to_world(10.0, -3.0);
        assert_eq!(p, Vec3::new(10.0, 0.0, -3.0));
    }
}

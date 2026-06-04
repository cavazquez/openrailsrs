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
//! **Tile layout**: the world is divided into tiles of 2048 m × 2048 m.  Tile numbers are stored
//! as "internal" values in `.tdb` (negative X for UK: e.g. tile_x = -6084) and as "display"
//! values in `.w` filenames (positive X for UK: e.g. `w-006084+014923.w`).
//!
//! Tile-local positions are centred: (0, 0) = tile centre, range roughly ±1024 m in X and Z.
//!
//! ## Bevy coordinate system
//!
//! Bevy uses a right-handed, Y-up system where the default camera looks toward **−Z**.
//!
//! ## MSTS → Bevy world-space conversion
//!
//! Follows Open Rails `WorldPositionFromMSTSLocation` in `Scenery.cs` and
//! `XNAVertexPositionNormalTextureFromMSTS` in `Shapes.cs`:
//!
//! ```text
//! bevy_x = tile_x_display * 2048 + local_x
//! bevy_y = local_y                            (Y up, unchanged)
//! bevy_z = tile_z * 2048 - local_z            (Z flipped, same as XNA / Open Rails)
//! ```
//!
//! This negation of local Z is the **only** axis change; X and Y are identity.
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
use openrailsrs_formats::{Matrix43, Vec3 as ShapeVec3};

/// MSTS tile size (metres), equal in X and Z.
pub const MSTS_TILE_SIZE_M: f64 = 2048.0;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A position in the MSTS world reference frame:
/// tile numbers (display convention, positive for UK east) plus tile-local offset.
///
/// Created from `.w` world-file items, `.tdb` track nodes, or tsection anchors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MstsWorldPosition {
    /// East tile index (display convention: positive east).
    pub tile_x: i32,
    /// South tile index (same sign convention in both `.w` and `.tdb`).
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
/// Tile X must use the **display** convention (positive east, as in `.w` filenames).
/// Tile Z uses the same sign in both `.w` and `.tdb` conventions.
///
/// Matches Open Rails `WorldPositionFromMSTSLocation` (`Scenery.cs`):
/// ```text
/// bevy_x = tile_x * 2048 + x
/// bevy_y = y
/// bevy_z = tile_z * 2048 - z   ← Z flip
/// ```
#[inline]
pub fn msts_to_bevy(pos: MstsWorldPosition) -> BevyWorldPosition {
    BevyWorldPosition(Vec3::new(
        (pos.tile_x as f64 * MSTS_TILE_SIZE_M + pos.x) as f32,
        pos.y as f32,
        (pos.tile_z as f64 * MSTS_TILE_SIZE_M - pos.z) as f32,
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

/// Convert an MSTS display tile X (positive east) to the internal sign convention
/// used in `.tdb` files (negative X for UK routes: internal = -display).
#[inline]
pub fn display_tile_x_to_internal(display: i32) -> i32 {
    -display
}

/// Convert an MSTS `.tdb` internal tile X to the display convention.
#[inline]
pub fn internal_tile_x_to_display(internal: i32) -> i32 {
    if internal < 0 { -internal } else { internal }
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
/// Row-major storage: `m[0..3]` = first row, etc.
pub fn matrix3x3_to_rotation_scale(m: &[f64; 9]) -> (Quat, Vec3) {
    let raw = matrix3x3_to_xna_mat3(m);
    let sx = raw.x_axis.length().max(1e-6);
    let sy = raw.y_axis.length().max(1e-6);
    let sz = raw.z_axis.length().max(1e-6);
    let scale = Vec3::new(sx, sy, sz);
    let normalized = Mat3::from_cols(raw.x_axis / sx, raw.y_axis / sy, raw.z_axis / sz);
    let rot = Quat::from_mat3(&normalized);
    (rot, scale)
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
        // Z flip: bevy_z = 0*2048 - (-3) = 3
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
        // x = 2*2048+10 = 4106; z = 1*2048-20 = 2028
        assert_eq!(p.as_vec3(), Vec3::new(4106.0, 0.0, 2028.0));
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
        // MSTS local z=+100 (away in MSTS conv) → Bevy z = 0*2048 - 100 = -100
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

    // ── display / internal tile ───────────────────────────────────────────────

    #[test]
    fn display_internal_tile_x_round_trip() {
        let display = 6084_i32;
        let internal = display_tile_x_to_internal(display);
        assert_eq!(internal, -6084);
        assert_eq!(internal_tile_x_to_display(internal), display);
    }

    #[test]
    fn internal_tile_x_to_display_handles_already_positive() {
        // Some routes have positive internal tile X; display should be same value.
        assert_eq!(internal_tile_x_to_display(100), 100);
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

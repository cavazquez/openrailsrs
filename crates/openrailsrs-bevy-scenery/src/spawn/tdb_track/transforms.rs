//! Metric tile / scene transforms for TDB geometry (world Bevy ↔ tile local).

use bevy::prelude::Vec3;

/// MSTS tile edge length in metres.
pub const MSTS_TILE_SIZE_M: f32 = 2048.0;

/// Bevy world → tile-local XZ (origin at SW corner of the tile).
pub fn world_to_tile_local(world: Vec3, tile_x: i32, tile_z: i32, tile_size_m: f32) -> (f32, f32) {
    let cx = tile_x as f32 * tile_size_m;
    let cz = -(tile_z as f32 * tile_size_m);
    (world.x - cx, world.z - cz)
}

/// Bevy world → tile-centred local XZ (terrain / object space).
pub fn world_to_tile_local_centered(
    world: Vec3,
    tile_x: i32,
    tile_z: i32,
    tile_size_m: f32,
) -> (f32, f32) {
    let half = tile_size_m * 0.5;
    let (lx, lz) = world_to_tile_local(world, tile_x, tile_z, tile_size_m);
    (lx - half, lz - half)
}

/// World Bevy → scene XZ (origin = centre of the focus tile).
pub fn world_to_scene_xz(
    world: Vec3,
    center_tile_x: i32,
    center_tile_z: i32,
    tile_size_m: f32,
) -> (f32, f32) {
    world_to_tile_local_centered(world, center_tile_x, center_tile_z, tile_size_m)
}

/// Scene XZ → world Bevy (inverse of [`world_to_scene_xz`]).
pub fn scene_xz_to_world(
    x: f32,
    z: f32,
    center_tile_x: i32,
    center_tile_z: i32,
    tile_size_m: f32,
) -> (f32, f32) {
    let half = tile_size_m * 0.5;
    let cx = center_tile_x as f32 * tile_size_m;
    let cz = -(center_tile_z as f32 * tile_size_m);
    (x + cx + half, z + cz + half)
}

/// Liang–Barsky clip of segment `a→b` against axis-aligned box `[-bound, bound]²` in XZ.
pub fn clip_segment_to_box(
    ax: f32,
    az: f32,
    bx: f32,
    bz: f32,
    bound: f32,
) -> Option<(f32, f32, f32, f32)> {
    let dx = bx - ax;
    let dz = bz - az;
    let mut t0 = 0.0f32;
    let mut t1 = 1.0f32;
    let edges = [
        (-dx, ax + bound),
        (dx, bound - ax),
        (-dz, az + bound),
        (dz, bound - az),
    ];
    for (p, q) in edges {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                if r > t1 {
                    return None;
                }
                if r > t0 {
                    t0 = r;
                }
            } else {
                if r < t0 {
                    return None;
                }
                if r < t1 {
                    t1 = r;
                }
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    Some((ax + t0 * dx, az + t0 * dz, ax + t1 * dx, az + t1 * dz))
}

/// Clipped ribbon endpoints in scene XZ for one world-space chord.
pub fn ribbon_scene_segment(
    start: Vec3,
    end: Vec3,
    center_tile_x: i32,
    center_tile_z: i32,
    tile_size_m: f32,
    bound_m: f32,
) -> Option<(f32, f32, f32, f32)> {
    let (ax, az) = world_to_scene_xz(start, center_tile_x, center_tile_z, tile_size_m);
    let (bx, bz) = world_to_scene_xz(end, center_tile_x, center_tile_z, tile_size_m);
    if ax.abs() > bound_m && az.abs() > bound_m && bx.abs() > bound_m && bz.abs() > bound_m {
        return None;
    }
    clip_segment_to_box(ax, az, bx, bz, bound_m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_local_roundtrip_scene() {
        let world = Vec3::new(100.0, 0.0, -50.0);
        let (sx, sz) = world_to_scene_xz(world, 0, 0, MSTS_TILE_SIZE_M);
        let (wx, wz) = scene_xz_to_world(sx, sz, 0, 0, MSTS_TILE_SIZE_M);
        assert!((wx - world.x).abs() < 1e-3);
        assert!((wz - world.z).abs() < 1e-3);
    }

    #[test]
    fn clip_keeps_interior_segment() {
        let clipped = clip_segment_to_box(-10.0, 0.0, 10.0, 0.0, 5.0).unwrap();
        assert!((clipped.0 - (-5.0)).abs() < 1e-4);
        assert!((clipped.2 - 5.0).abs() < 1e-4);
    }
}

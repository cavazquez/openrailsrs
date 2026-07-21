//! Shared Forest RNG, scatter helpers and OR-style billboard mesh (#117 / #38).

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

pub const DEFAULT_TREE_WIDTH_M: f32 = 5.0;
pub const DEFAULT_TREE_HEIGHT_M: f32 = 12.0;
pub const DEFAULT_PATCH_HALF_M: f32 = 128.0;
pub const MAX_SCATTER_ATTEMPTS: u32 = 12;

/// Tree height/width baseline in metres.
pub fn forest_tree_size(width: f32, height: f32) -> (f32, f32) {
    let w = if width > 0.0 {
        width.clamp(0.5, 50.0)
    } else {
        DEFAULT_TREE_WIDTH_M
    };
    let h = if height > 0.0 {
        height.clamp(1.0, 80.0)
    } else {
        DEFAULT_TREE_HEIGHT_M
    };
    (w, h)
}

/// Deterministic [0, 1) sample for tree placement (Open Rails-style seeded RNG).
pub fn forest_rng01(tile_x: i32, tile_z: i32, uid: u32, tree_index: u32, channel: u32) -> f32 {
    let mut x = (tile_x as u32)
        ^ (tile_z as u32).rotate_left(7)
        ^ uid.rotate_left(13)
        ^ tree_index.rotate_left(3)
        ^ channel.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 16;
    (x as f32) / (u32::MAX as f32)
}

/// One tree placement in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreePlacement {
    pub position: Vec3,
    pub scale: f32,
}

/// Optional clearance predicate: return `true` when `(x, z)` is too close to track.
pub type TrackClearanceFn<'a> = dyn Fn(f32, f32, f32) -> bool + 'a;

/// Scatter trees inside a rectangular patch around `anchor`.
///
/// `sample_y(x, z)` supplies terrain height. When `track_blocked` is provided and
/// returns true for a candidate with the current clearance, the sample is retried.
#[allow(clippy::too_many_arguments)]
pub fn scatter_trees_in_patch(
    anchor: Vec3,
    patch_half_x: f32,
    patch_half_z: f32,
    population: u32,
    scale_min: f32,
    scale_max: f32,
    tile_x: i32,
    tile_z: i32,
    uid: u32,
    sample_y: impl Fn(f32, f32) -> f32,
    track_blocked: Option<&TrackClearanceFn<'_>>,
    track_clearance_m: f32,
) -> Vec<TreePlacement> {
    let mut trees = Vec::with_capacity(population as usize);
    for i in 0..population {
        let mut placed = None;
        for attempt in 0..MAX_SCATTER_ATTEMPTS {
            let ch = attempt * 4;
            let rx = forest_rng01(tile_x, tile_z, uid, i, ch) * 2.0 - 1.0;
            let rz = forest_rng01(tile_x, tile_z, uid, i, ch + 1) * 2.0 - 1.0;
            let x = anchor.x + rx * patch_half_x;
            let z = anchor.z + rz * patch_half_z;
            let clearance = if attempt + 1 == MAX_SCATTER_ATTEMPTS {
                0.0
            } else {
                track_clearance_m
            };
            if clearance > 0.0 && track_blocked.is_some_and(|blocked| blocked(x, z, clearance)) {
                continue;
            }
            let t = forest_rng01(tile_x, tile_z, uid, i, ch + 2);
            let scale = scale_min + (scale_max - scale_min) * t;
            let y = sample_y(x, z);
            placed = Some(TreePlacement {
                position: Vec3::new(x, y, z),
                scale,
            });
            break;
        }
        if let Some(tree) = placed {
            trees.push(tree);
        }
    }
    trees
}

/// Append one OR-style forest billboard (single quad).
///
/// All four corners share the tree base in `POSITION`; `NORMAL.xy` stores
/// width/height so [`crate::OrForestMaterial`] can expand toward the camera in VS.
pub fn append_tree_billboard(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    width: f32,
    height: f32,
) {
    let base = positions.len() as u32;
    let size = [width, height, 0.0];
    for _ in 0..4 {
        positions.push([origin.x, origin.y, origin.z]);
        normals.push(size);
    }
    uvs.extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Merge camera-facing billboards for all trees into one mesh (OR `ForestPrimitive`).
pub fn build_forest_patch_mesh(trees: &[TreePlacement], base_width: f32, base_height: f32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    for tree in trees {
        append_tree_billboard(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            tree.position,
            base_width * tree.scale,
            base_height * tree.scale,
        );
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic() {
        let a = forest_rng01(1, 2, 3, 4, 5);
        let b = forest_rng01(1, 2, 3, 4, 5);
        assert_eq!(a, b);
        assert!((0.0..1.0).contains(&a));
    }

    #[test]
    fn scatter_respects_population() {
        let trees = scatter_trees_in_patch(
            Vec3::ZERO,
            10.0,
            10.0,
            8,
            0.9,
            1.1,
            0,
            0,
            1,
            |_, _| 0.0,
            None,
            0.0,
        );
        assert_eq!(trees.len(), 8);
    }

    #[test]
    fn billboard_mesh_has_one_quad_per_tree() {
        let trees = [TreePlacement {
            position: Vec3::ZERO,
            scale: 1.0,
        }];
        let mesh = build_forest_patch_mesh(&trees, 4.0, 12.0);
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("positions");
        assert_eq!(positions.len(), 4);
    }
}

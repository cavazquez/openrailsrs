//! Shared MSTS `Transfer` ground-decal mesh (#116).
//!
//! Geometry matches Open Rails `TransferPrimitive`: 8 m grid draped on terrain,
//! MSTS +Z south indexing, and UV from inverse object rotation.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

const GRID_M: f32 = 8.0;
const NORMAL_SAMPLE_M: f32 = 4.0;

/// Open Rails `TransferMaterial.ReferenceAlpha = 10` (0–255).
pub const TRANSFER_ALPHA_CUTOFF: f32 = 10.0 / 255.0;

/// Height sample in the same world frame as `center` (Bevy XZ, Y up).
pub trait TransferHeightSampler {
    fn sample_y(&self, x: f32, z: f32) -> f32;
}

impl<F> TransferHeightSampler for F
where
    F: Fn(f32, f32) -> f32,
{
    fn sample_y(&self, x: f32, z: f32) -> f32 {
        self(x, z)
    }
}

/// Terrain-following transfer mesh (parity with OR `TransferPrimitive`).
///
/// `center` is world XZ with Y as the patch origin height. Vertex positions are
/// relative to `center` so the entity transform can place the patch.
pub fn build_transfer_mesh(
    center: Vec3,
    width: f32,
    height: f32,
    inv_rot: Quat,
    height_field: &impl TransferHeightSampler,
) -> Option<Mesh> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let radius = (width * width + height * height).sqrt() * 0.5;
    let min_ix = ((center.x - radius) / GRID_M).floor() as i32;
    let max_ix = ((center.x + radius) / GRID_M).ceil() as i32;
    // OR indexes Z in MSTS (+Z south); Bevy scenery uses `-z`.
    let center_msts_z = -center.z;
    let min_iz = ((center_msts_z - radius) / GRID_M).floor() as i32;
    let max_iz = ((center_msts_z + radius) / GRID_M).ceil() as i32;
    if min_ix >= max_ix || min_iz >= max_iz {
        return None;
    }

    let nx = (max_ix - min_ix + 1) as usize;
    let nz = (max_iz - min_iz + 1) as usize;
    let mut positions = Vec::with_capacity(nx * nz);
    let mut normals = Vec::with_capacity(nx * nz);
    let mut uvs = Vec::with_capacity(nx * nz);

    for ix in min_ix..=max_ix {
        for iz in min_iz..=max_iz {
            let wx = ix as f32 * GRID_M;
            let wz = -(iz as f32 * GRID_M);
            let rel_x = wx - center.x;
            let rel_z = wz - center.z;
            let y = height_field.sample_y(wx, wz) - center.y;
            positions.push([rel_x, y, rel_z]);

            let y_dx0 = height_field.sample_y(wx - NORMAL_SAMPLE_M, wz);
            let y_dx1 = height_field.sample_y(wx + NORMAL_SAMPLE_M, wz);
            let y_dz0 = height_field.sample_y(wx, wz - NORMAL_SAMPLE_M);
            let y_dz1 = height_field.sample_y(wx, wz + NORMAL_SAMPLE_M);
            let n =
                Vec3::new(y_dx0 - y_dx1, 2.0 * NORMAL_SAMPLE_M, y_dz0 - y_dz1).normalize_or_zero();
            normals.push([n.x, n.y, n.z]);

            let tc = inv_rot * Vec3::new(rel_x, 0.0, rel_z);
            uvs.push([tc.x / width + 0.5, tc.z / height + 0.5]);
        }
    }

    let mut indices = Vec::new();
    let cols = nz;
    let dx = (max_ix - min_ix) as usize;
    let dz = (max_iz - min_iz) as usize;
    for x in 0..dx {
        for z in 0..dz {
            let i00 = (x * cols + z) as u32;
            let i10 = ((x + 1) * cols + z) as u32;
            let i01 = (x * cols + z + 1) as u32;
            let i11 = ((x + 1) * cols + z + 1) as u32;
            if (x as i32 + min_ix) & 1 == (z as i32 + min_iz) & 1 {
                indices.extend([i00, i11, i10, i00, i01, i11]);
            } else {
                indices.extend([i01, i11, i10, i01, i00, i10]);
            }
        }
    }

    if indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_mesh_has_positions_and_indices() {
        let mesh = build_transfer_mesh(
            Vec3::new(0.0, 10.0, 0.0),
            16.0,
            16.0,
            Quat::IDENTITY,
            &|_, _| 10.0,
        )
        .expect("mesh");
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("positions");
        assert!(positions.len() >= 4);
        assert!(mesh.indices().is_some());
    }
}

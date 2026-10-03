//! Shared MSTS `Transfer` ground-decal mesh (#116).
//!
//! Geometry matches Open Rails `TransferPrimitive`: 8 m grid draped on terrain,
//! MSTS +Z south indexing, and UV from inverse object rotation.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

const GRID_M: f32 = 8.0;
const NORMAL_SAMPLE_M: f32 = 4.0;

#[derive(Clone, Copy)]
struct TransferVertex {
    position: Vec3,
    normal: Vec3,
    uv: Vec2,
}

impl TransferVertex {
    fn interpolate(self, other: Self, t: f32) -> Self {
        Self {
            position: self.position.lerp(other.position, t),
            normal: self.normal.lerp(other.normal, t).normalize_or_zero(),
            uv: self.uv.lerp(other.uv, t),
        }
    }
}

/// OR PSImage9_3Clamp discards transfer fragments outside UV [0, 1].
/// Clip the existing terrain triangles to that rectangle before upload so both
/// Bevy renderers enforce the same footprint without a special sampler feature.
fn clip_transfer_triangle(triangle: [TransferVertex; 3]) -> Vec<TransferVertex> {
    let mut polygon = triangle.to_vec();
    for (axis, boundary, keep_above) in [
        (0, 0.0, true),
        (0, 1.0, false),
        (1, 0.0, true),
        (1, 1.0, false),
    ] {
        let mut clipped = Vec::with_capacity(polygon.len() + 1);
        let Some(&last) = polygon.last() else { break };
        let inside = |v: TransferVertex| {
            if keep_above {
                v.uv[axis] >= boundary
            } else {
                v.uv[axis] <= boundary
            }
        };
        let mut previous = last;
        for current in polygon {
            if inside(previous) != inside(current) {
                let t = (boundary - previous.uv[axis]) / (current.uv[axis] - previous.uv[axis]);
                let mut intersection = previous.interpolate(current, t);
                intersection.uv[axis] = boundary;
                clipped.push(intersection);
            }
            if inside(current) {
                clipped.push(current);
            }
            previous = current;
        }
        polygon = clipped;
    }
    polygon
}

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

    let mut clipped_positions = Vec::new();
    let mut clipped_normals = Vec::new();
    let mut clipped_uvs = Vec::new();
    let mut clipped_indices = Vec::new();
    for triangle in indices.as_chunks::<3>().0 {
        let polygon = clip_transfer_triangle(triangle.map(|i| TransferVertex {
            position: Vec3::from(positions[i as usize]),
            normal: Vec3::from(normals[i as usize]),
            uv: Vec2::from(uvs[i as usize]),
        }));
        for j in 1..polygon.len().saturating_sub(1) {
            let triangle = [polygon[0], polygon[j], polygon[j + 1]];
            if (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position)
                .length_squared()
                < 1e-12
            {
                continue;
            }
            for vertex in triangle {
                clipped_indices.push(clipped_positions.len() as u32);
                clipped_positions.push(vertex.position.to_array());
                clipped_normals.push(vertex.normal.to_array());
                clipped_uvs.push(vertex.uv.to_array());
            }
        }
    }
    if clipped_indices.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, clipped_positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, clipped_normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, clipped_uvs);
    mesh.insert_indices(Indices::U32(clipped_indices));
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

    #[test]
    fn rotated_transfer_covers_only_its_texture_rectangle_and_keeps_surface_height() {
        let center = Vec3::new(3.25, 12.0, -2.75);
        let mesh = build_transfer_mesh(center, 10.0, 40.0, Quat::from_rotation_y(0.7), &|x, z| {
            12.0 + x * 0.1 - z * 0.2
        })
        .unwrap();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap();
        let bevy::mesh::VertexAttributeValues::Float32x2(uvs) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap()
        else {
            panic!("UVs");
        };
        for (position, uv) in positions.iter().zip(uvs) {
            assert!(
                uv.iter().all(|v| (0.0..=1.0).contains(v)),
                "outside footprint: {uv:?}"
            );
            let expected = (position[0] + center.x) * 0.1 - (position[2] + center.z) * 0.2;
            assert!((position[1] - expected).abs() < 1e-4);
        }
        let area: f32 = positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                let a = Vec3::from(triangle[1]) - Vec3::from(triangle[0]);
                let b = Vec3::from(triangle[2]) - Vec3::from(triangle[0]);
                a.cross(b).y.abs() * 0.5
            })
            .sum();
        assert!((area - 400.0).abs() < 0.01, "rectangle area: {area}");
    }
}

//! Conservative WORLD visibility: exact mesh bounds and actual opaque triangles.
//! An uncertain/near-plane case stays visible. Shadow submissions are independent.
use super::*;
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::mesh::VertexAttributeValues;

#[derive(Resource, Clone, ExtractResource)]
pub(super) struct VisibilityOptions {
    pub gpu: bool,
    pub force_gpu: bool,
    pub occlusion: bool,
}
impl Default for VisibilityOptions {
    fn default() -> Self {
        let enabled =
            |name| std::env::var(name).map_or(true, |v| !matches!(v.trim(), "0" | "off" | "false"));
        Self {
            gpu: enabled("OPENRAILSRS_WORLD_GPU_CULLING"),
            force_gpu: std::env::var("OPENRAILSRS_WORLD_GPU_CULLING")
                .is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "on")),
            occlusion: enabled("OPENRAILSRS_WORLD_OCCLUSION"),
        }
    }
}
#[derive(Component, Clone, ExtractComponent)]
pub(super) struct InstanceBounds {
    pub center: Vec4,
    pub half: Vec4,
    pub group: Aabb,
    mesh: AssetId<Mesh>,
    source: Arc<[WorldInstanceData]>,
}
#[derive(Clone, PartialEq)]
pub(super) struct Triangle {
    entity: Entity,
    points: [Vec3; 3],
}
#[derive(Resource, Clone, Default, ExtractResource)]
pub(super) struct OcclusionGeometry(pub Arc<[Triangle]>);
#[derive(Component)]
pub struct WorldOccluder;

pub(super) fn update_bounds(
    mut commands: Commands,
    meshes: Option<Res<Assets<Mesh>>>,
    query: Query<(
        Entity,
        &Mesh3d,
        &WorldInstanceBuffer,
        Option<&InstanceBounds>,
    )>,
) {
    let Some(meshes) = meshes else {
        return;
    };
    for (entity, mesh, data, existing) in &query {
        if existing.is_some_and(|b| b.mesh == mesh.id() && Arc::ptr_eq(&b.source, &data.0)) {
            continue;
        }
        let Some(local) = meshes.get(mesh).and_then(MeshAabb::compute_aabb) else {
            commands.entity(entity).remove::<InstanceBounds>();
            continue;
        };
        if data.is_empty()
            || !Vec3::from(local.center).is_finite()
            || !Vec3::from(local.half_extents).is_finite()
        {
            commands.entity(entity).remove::<InstanceBounds>();
            continue;
        }
        commands.entity(entity).insert(InstanceBounds {
            center: Vec3::from(local.center).extend(1.),
            half: Vec3::from(local.half_extents).extend(1.),
            group: instances_aabb(data, Some(&local)),
            mesh: mesh.id(),
            source: data.0.clone(),
        });
    }
}

pub(super) fn collect_occluders(
    options: Res<VisibilityOptions>,
    meshes: Option<Res<Assets<Mesh>>>,
    materials: Option<Res<Assets<StandardMaterial>>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    sources: Query<
        (
            Entity,
            &Mesh3d,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            Option<&WorldInstanceBuffer>,
            Option<&WorldInstanceAppearance>,
        ),
        Or<(
            With<WorldInstanceBuffer>,
            With<openrailsrs_bevy_scenery::TileBound>,
            With<WorldOccluder>,
        )>,
    >,
    mut geometry: ResMut<OcclusionGeometry>,
) {
    let (Some(meshes), Some(materials)) = (meshes, materials) else {
        return;
    };
    let Some((_, camera)) = cameras
        .iter()
        .filter(|(c, _)| c.is_active)
        .min_by_key(|(c, _)| c.order)
    else {
        geometry.0 = Arc::from([]);
        return;
    };
    if !options.occlusion {
        geometry.0 = Arc::from([]);
        return;
    }
    let eye = camera.translation();
    let mut nearby = sources
        .iter()
        .filter_map(|(e, m, t, material, instances, appearance)| {
            // A back-facing triangle may be absent from the actual draw. Only
            // two-sided, unbiased opaque surfaces can certify coverage here.
            if !materials.get(material).is_some_and(|m| {
                matches!(m.alpha_mode, AlphaMode::Opaque)
                    && m.cull_mode.is_none()
                    && m.depth_bias == 0.
            }) || appearance.is_some_and(|a| {
                a.alpha_cutoff > 0.
                    || a.lod_fade != 0.
                    || a.vegetation.x > 0.
                    || a.surface_weather != Vec2::ZERO
            }) {
                return None;
            }
            let p = instances
                .and_then(|b| b.first())
                .map_or(t.translation(), |b| t.transform_point(b.translation()));
            Some((p.distance_squared(eye), e, m, t, instances))
        })
        .collect::<Vec<_>>();
    nearby.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut triangles = Vec::new();
    'sources: for (_, entity, handle, transform, instances) in nearby.into_iter().take(48) {
        let Some(mesh) = meshes.get(handle) else {
            continue;
        };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            continue;
        }
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let indices = mesh
            .indices()
            .map(|i| i.iter().take(288).collect::<Vec<_>>())
            .unwrap_or_else(|| (0..positions.len().min(288)).collect());
        let matrices = instances.map_or_else(
            || vec![Mat4::IDENTITY],
            |b| {
                b.iter()
                    .take(8)
                    .map(|i| Mat4::from_cols_array_2d(&[i.col0, i.col1, i.col2, i.col3]))
                    .collect()
            },
        );
        for matrix in matrices {
            let world = transform.to_matrix() * matrix;
            for index in indices.as_chunks::<3>().0.iter().take(96) {
                let [Some(a), Some(b), Some(c)] = index.map(|i| positions.get(i)) else {
                    continue;
                };
                let points = [*a, *b, *c].map(|p| world.transform_point3(Vec3::from(p)));
                if points.iter().all(|p| p.is_finite()) {
                    triangles.push(Triangle { entity, points });
                }
                if triangles.len() >= 1024 {
                    break 'sources;
                }
            }
        }
    }
    if geometry.0.as_ref() != triangles.as_slice() {
        geometry.0 = triangles.into();
    }
}

pub(super) struct ProjectedTriangle {
    points: [Vec2; 3],
    far: f32,
    area: f32,
    altitudes: Vec3,
    inset: f32,
}
impl ProjectedTriangle {
    fn new(points: [Vec2; 3], far: f32, inset: f32) -> Option<Self> {
        let [a, b, c] = points;
        let area = (b - a).perp_dot(c - a);
        if area.abs() <= 0.001 {
            return None;
        }
        let altitudes = Vec3::new(
            area.abs() / (b - c).length(),
            area.abs() / (c - a).length(),
            area.abs() / (a - b).length(),
        );
        Some(Self {
            points,
            far,
            area,
            altitudes,
            inset,
        })
    }
}
pub(super) fn project_occluders(
    geometry: &OcclusionGeometry,
    clip: Mat4,
    visible: &HashSet<MainEntity>,
    inset: f32,
) -> Vec<ProjectedTriangle> {
    let mut result = geometry
        .0
        .iter()
        .filter(|t| visible.contains(&MainEntity::from(t.entity)))
        .filter_map(|t| {
            let points = t.points.map(|p| clip * p.extend(1.));
            if points
                .iter()
                .any(|p| !p.is_finite() || p.w <= 1. || p.z < 0. || p.z > p.w)
            {
                return None;
            }
            let far = points.iter().map(|p| p.w).fold(0., f32::max);
            let points = points.map(|p| p.truncate().truncate() / p.w);
            ProjectedTriangle::new(points, far, inset)
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| b.area.abs().total_cmp(&a.area.abs()));
    result.truncate(64);
    result
}

pub(super) fn occluded(
    bounds: &Aabb,
    world: Mat4,
    clip: Mat4,
    triangles: &[ProjectedTriangle],
) -> bool {
    let center = Vec3::from(bounds.center);
    let half = Vec3::from(bounds.half_extents) + Vec3::splat(0.25);
    let corners = std::array::from_fn::<_, 8, _>(|i| {
        clip * world
            .transform_point3(
                center
                    + half
                        * Vec3::new(
                            if i & 1 == 0 { -1. } else { 1. },
                            if i & 2 == 0 { -1. } else { 1. },
                            if i & 4 == 0 { -1. } else { 1. },
                        ),
            )
            .extend(1.)
    });
    if corners
        .iter()
        .any(|p| !p.is_finite() || p.w <= 0.1 || p.z < 0. || p.z > p.w)
    {
        return false;
    }
    let near = corners.iter().map(|p| p.w).fold(f32::INFINITY, f32::min);
    triangles.iter().any(|triangle| {
        near > triangle.far + 0.25
            && corners.iter().all(|p| {
                let p = p.truncate().truncate() / p.w;
                let [a, b, c] = triangle.points;
                let u = (b - p).perp_dot(c - p) / triangle.area;
                let v = (c - p).perp_dot(a - p) / triangle.area;
                let w = 1. - u - v;
                u > 0.01
                    && v > 0.01
                    && w > 0.01
                    && (Vec3::new(u, v, w) * triangle.altitudes).min_element() > triangle.inset
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_instances_release_cached_bounds_and_the_previous_source() {
        let mut app = App::new();
        let mut meshes = Assets::<Mesh>::default();
        let mesh = meshes.add(Cuboid::default());
        app.insert_resource(meshes)
            .add_systems(Update, update_bounds);
        let data: Arc<[WorldInstanceData]> =
            Arc::from([WorldInstanceData::from_mat4(Mat4::IDENTITY)]);
        let weak = Arc::downgrade(&data);
        let entity = app
            .world_mut()
            .spawn((Mesh3d(mesh), WorldInstanceBuffer(data)))
            .id();
        app.update();
        assert!(app.world().get::<InstanceBounds>(entity).is_some());
        app.world_mut()
            .entity_mut(entity)
            .insert(WorldInstanceBuffer(Arc::from([])));
        app.update();
        assert!(app.world().get::<InstanceBounds>(entity).is_none());
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn occlusion_requires_full_coverage_front_depth_and_valid_projection() {
        let projection = Mat4::perspective_infinite_reverse_rh(1.0, 1., 0.1);
        let wall = ProjectedTriangle::new(
            [Vec2::new(-2., -2.), Vec2::new(2., -2.), Vec2::new(0., 2.)],
            2.,
            0.01,
        )
        .unwrap();
        let bounds = Aabb::from_min_max(-Vec3::splat(0.1), Vec3::splat(0.1));
        assert!(occluded(
            &bounds,
            Mat4::from_translation(Vec3::new(0., 0., -10.)),
            projection,
            &[wall]
        ));
        let empty: Vec<ProjectedTriangle> = vec![];
        assert!(!occluded(&bounds, Mat4::IDENTITY, projection, &empty));
        let crossing = ProjectedTriangle::new(
            [
                Vec2::new(-0.01, -2.),
                Vec2::new(0.01, -2.),
                Vec2::new(0., 2.),
            ],
            2.,
            0.01,
        )
        .unwrap();
        assert!(!occluded(
            &bounds,
            Mat4::from_translation(Vec3::new(0., 0., -10.)),
            projection,
            &[crossing]
        ));
    }
}

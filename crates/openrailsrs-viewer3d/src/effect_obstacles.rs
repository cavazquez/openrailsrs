//! Bounded near-field geometry for plumes. Opaque part bounds are conservative;
//! alpha-tested foliage and transparent glass never become solid box walls.
use bevy::{
    camera::primitives::{Aabb, MeshAabb},
    ecs::system::SystemParam,
    prelude::*,
};

#[derive(Clone, Copy, Debug)]
pub struct Solid {
    pub min: Vec3,
    pub max: Vec3,
    pub owner: Option<Entity>,
}
impl Solid {
    pub fn hits(&self, from: Vec3, to: Vec3) -> bool {
        let direction = to - from;
        let mut enter = 0.0_f32;
        let mut leave = 1.0_f32;
        for axis in 0..3 {
            if direction[axis].abs() < 1e-6 {
                if from[axis] < self.min[axis] || from[axis] > self.max[axis] {
                    return false;
                }
            } else {
                let a = (self.min[axis] - from[axis]) / direction[axis];
                let b = (self.max[axis] - from[axis]) / direction[axis];
                enter = enter.max(a.min(b));
                leave = leave.min(a.max(b));
                if enter > leave {
                    return false;
                }
            }
        }
        enter <= leave
    }
    fn distance2(&self, point: Vec3) -> f32 {
        (point - point.clamp(self.min, self.max)).length_squared()
    }
}

#[derive(Resource, Default)]
pub struct EffectObstacles {
    pub roofs: Vec<RoofTriangle>,
    roof_meshes:
        std::collections::HashMap<bevy::asset::AssetId<Mesh>, std::sync::Arc<Vec<RoofTriangle>>>,
    static_roofs: Vec<RoofTriangle>,
    pub solids: Vec<Solid>,
    elapsed: f32,
    center: Option<Vec3>,
    origin: Vec3,
    statics: Vec<Solid>,
    pub refreshes: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct RoofTriangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
}
impl RoofTriangle {
    pub fn height(&self, point: Vec2) -> Option<f32> {
        let a = self.a.xz();
        let b = self.b.xz();
        let c = self.c.xz();
        let cross = |p: Vec2, q: Vec2| p.x * q.y - p.y * q.x;
        let det = cross(b - a, c - a);
        if det.abs() < 1e-6 {
            return None;
        }
        let u = cross(point - a, c - a) / det;
        let v = cross(b - a, point - a) / det;
        (u >= -0.0001 && v >= -0.0001 && u + v <= 1.0001)
            .then_some(self.a.y + (self.b.y - self.a.y) * u + (self.c.y - self.a.y) * v)
    }
}
fn roof_triangles(mesh: &Mesh, matrix: Mat4, out: &mut Vec<RoofTriangle>, limit: usize) {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(vertices)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return;
    };
    let indices = mesh
        .indices()
        .map(|i| i.iter().collect::<Vec<_>>())
        .unwrap_or_else(|| (0..vertices.len()).collect());
    for index in indices.as_chunks::<3>().0 {
        if out.len() >= limit {
            break;
        }
        let Some(a) = vertices.get(index[0]) else {
            continue;
        };
        let Some(b) = vertices.get(index[1]) else {
            continue;
        };
        let Some(c) = vertices.get(index[2]) else {
            continue;
        };
        let (a, b, c) = (
            matrix.transform_point3(Vec3::from_array(*a)),
            matrix.transform_point3(Vec3::from_array(*b)),
            matrix.transform_point3(Vec3::from_array(*c)),
        );
        let n = (b - a).cross(c - a);
        if n.y.abs() > n.length() * 0.15 {
            out.push(RoofTriangle { a, b, c });
        }
    }
}
fn transform_roofs(
    template: &[RoofTriangle],
    matrix: Mat4,
    out: &mut Vec<RoofTriangle>,
    limit: usize,
) {
    for t in template.iter().take(limit.saturating_sub(out.len())) {
        out.push(RoofTriangle {
            a: matrix.transform_point3(t.a),
            b: matrix.transform_point3(t.b),
            c: matrix.transform_point3(t.c),
        });
    }
}
impl EffectObstacles {
    pub fn blocked(&self, from: Vec3, to: Vec3, source: Option<Entity>) -> bool {
        self.solids
            .iter()
            .any(|b| (source.is_none() || b.owner != source) && b.hits(from, to))
    }
    pub fn wind_factor(&self, point: Vec3) -> f32 {
        if self.solids.iter().any(|b| {
            point.x >= b.min.x
                && point.x <= b.max.x
                && point.z >= b.min.z
                && point.z <= b.max.z
                && b.max.y > point.y + 0.25
                && b.min.y < point.y + 12.0
        }) {
            0.25
        } else {
            1.0
        }
    }
    /// Conservative near-field cover using individual opaque parts, excluding trains.
    pub fn overhead(&self, point: Vec3) -> bool {
        if !self.roofs.is_empty() {
            return self.roofs.iter().any(|t| {
                t.height(point.xz())
                    .is_some_and(|h| h > point.y + 0.15 && h < point.y + 12.)
            });
        }
        self.solids
            .iter()
            .any(|b| b.owner.is_none() && b.hits(point, point + Vec3::Y * 12.0))
    }
    pub fn enclosure(&self, point: Vec3) -> f32 {
        if !self.overhead(point) {
            return 0.0;
        }
        let walls = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z]
            .into_iter()
            .filter(|d| {
                self.solids
                    .iter()
                    .any(|b| b.owner.is_none() && b.hits(point, point + *d * 8.0))
            })
            .count();
        (walls as f32 / 3.0).clamp(0.0, 1.0)
    }
    pub fn nearest(&self, point: Vec3, count: usize, source: Entity) -> Vec<Solid> {
        // Do not block an authored outlet with a rest-baked source-car box.
        // Otherwise accelerated steps can kill a plume while it leaves its stack.
        let mut solids: Vec<_> = self
            .solids
            .iter()
            .copied()
            .filter(|b| b.owner != Some(source))
            .collect();
        solids.sort_by(|a, b| a.distance2(point).total_cmp(&b.distance2(point)));
        solids.truncate(count);
        solids
    }
}

#[derive(SystemParam)]
pub struct ObstacleScene<'w, 's> {
    objects: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static Aabb,
            &'static Mesh3d,
            Option<&'static MeshMaterial3d<StandardMaterial>>,
            Option<&'static crate::surface_weather::SnowSurfaceSource>,
            Option<&'static MeshMaterial3d<openrailsrs_bevy_scenery::OrSceneryMaterial>>,
            Option<&'static crate::world_instancing::WorldInstanceBuffer>,
            Option<&'static crate::world_instancing::WorldInstanceAppearance>,
            Has<crate::live::LiveTrainBody>,
            Option<&'static ChildOf>,
        ),
        Without<crate::weather_particles::WeatherMesh>,
    >,
    materials: Res<'w, Assets<StandardMaterial>>,
    native: Res<'w, Assets<openrailsrs_bevy_scenery::OrSceneryMaterial>>,
    meshes: Res<'w, Assets<Mesh>>,
}
pub fn update(
    time: Res<Time<Real>>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    scene: ObstacleScene,
    mut obstacles: ResMut<EffectObstacles>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let center = camera.translation();
    obstacles.elapsed += time.delta_secs();
    let refresh = obstacles
        .center
        .is_none_or(|p| p.distance_squared(center) > 16.0)
        || obstacles.elapsed >= 0.5
        || obstacles.origin != origin.shift;
    if refresh {
        obstacles.statics.clear();
        obstacles.static_roofs.clear();
        obstacles
            .roof_meshes
            .retain(|id, _| scene.meshes.get(*id).is_some());
    }
    let mut dynamic = Vec::new();
    let mut dynamic_roofs = Vec::new();
    for (tf, bounds, mesh, standard, source, native, instances, appearance, train, parent) in
        &scene.objects
    {
        if !train && !refresh {
            continue;
        }
        let opaque = if let Some(appearance) = appearance {
            appearance.alpha_cutoff == 0.0
        } else {
            standard
                .map(|m| &m.0)
                .or_else(|| source.map(|m| &m.0))
                .and_then(|h| scene.materials.get(h))
                .map(|m| m.alpha_mode)
                .or_else(|| {
                    native
                        .and_then(|m| scene.native.get(&m.0))
                        .map(|m| m.alpha_mode)
                })
                == Some(AlphaMode::Opaque)
        };
        if !opaque {
            continue;
        }
        // An instanced entity's Aabb encloses the entire group, not one mesh.
        // Reusing it for every instance would turn a whole station into a wall.
        let mesh_bounds = instances
            .and_then(|_| scene.meshes.get(&mesh.0))
            .and_then(MeshAabb::compute_aabb);
        let bounds = mesh_bounds.as_ref().unwrap_or(bounds);
        let mut insert = |matrix: Mat4| {
            let point = matrix.transform_point3(bounds.center.into());
            let extent = Mat3::from_mat4(matrix).abs() * Vec3::from(bounds.half_extents);
            let solid = Solid {
                min: point - extent - Vec3::splat(0.025),
                max: point + extent + Vec3::splat(0.025),
                owner: if train {
                    parent.map(|p| p.parent())
                } else {
                    None
                },
            };
            // Thin opaque roofs/walls still block; long tunnel parts are near
            // when their surface is near, even if their centre is far away.
            let broad_axes = [extent.x, extent.y, extent.z]
                .into_iter()
                .filter(|e| *e >= 0.12)
                .count();
            if broad_axes < 2 || solid.distance2(center) > 160.0_f32.powi(2) {
                return;
            }
            let template = if let Some(cached) = obstacles.roof_meshes.get(&mesh.0.id()) {
                cached.clone()
            } else {
                let mut triangles = Vec::new();
                if let Some(source) = scene.meshes.get(&mesh.0) {
                    roof_triangles(source, Mat4::IDENTITY, &mut triangles, 8192);
                }
                let triangles = std::sync::Arc::new(triangles);
                if obstacles.roof_meshes.len() >= 256 {
                    obstacles.roof_meshes.clear();
                }
                obstacles.roof_meshes.insert(mesh.0.id(), triangles.clone());
                triangles
            };
            if train {
                dynamic.push(solid);
                transform_roofs(&template, matrix, &mut dynamic_roofs, 8192);
            } else {
                obstacles.statics.push(solid);
                transform_roofs(&template, matrix, &mut obstacles.static_roofs, 32768);
            }
        };
        if let Some(instances) = instances {
            for instance in instances.iter() {
                let instance = Mat4::from_cols_array_2d(&[
                    instance.col0,
                    instance.col1,
                    instance.col2,
                    instance.col3,
                ]);
                insert(tf.to_matrix() * instance);
            }
        } else {
            insert(tf.to_matrix());
        }
    }
    if refresh {
        obstacles
            .statics
            .sort_by(|a, b| a.distance2(center).total_cmp(&b.distance2(center)));
        obstacles.statics.truncate(128);
        obstacles.center = Some(center);
        obstacles.origin = origin.shift;
        obstacles.elapsed = 0.0;
        obstacles.refreshes += 1;
    }
    dynamic.sort_by(|a, b| a.distance2(center).total_cmp(&b.distance2(center)));
    dynamic.truncate(64);
    obstacles.solids = obstacles.statics.clone();
    obstacles.solids.extend(dynamic);
    obstacles.roofs = obstacles.static_roofs.clone();
    obstacles.roofs.extend(dynamic_roofs);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swept_plume_does_not_tunnel_through_a_thin_roof() {
        let roof = Solid {
            min: Vec3::new(-5.0, 3.0, -5.0),
            max: Vec3::new(5.0, 3.1, 5.0),
            owner: None,
        };
        assert!(roof.hits(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 5.0, 0.0)));
        assert!(!roof.hits(Vec3::new(6.0, 2.0, 0.0), Vec3::new(6.0, 5.0, 0.0)));
        assert!(!roof.hits(Vec3::new(0.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 0.0)));
        let scene = EffectObstacles {
            solids: vec![roof],
            ..default()
        };
        assert_eq!(scene.wind_factor(Vec3::Y * 2.0), 0.25);
        assert_eq!(scene.wind_factor(Vec3::Y * 4.0), 1.0);
    }
    #[test]
    fn source_outlet_is_excluded_without_ignoring_station_roofs_or_other_cars() {
        let source = Entity::from_raw_u32(1).unwrap();
        let neighbour = Entity::from_raw_u32(2).unwrap();
        let bounds = Solid {
            min: Vec3::ZERO,
            max: Vec3::splat(5.0),
            owner: Some(source),
        };
        let mut scene = EffectObstacles {
            solids: vec![bounds],
            ..default()
        };
        let (from, to) = (Vec3::ONE, Vec3::Y * 6.0 + Vec3::X + Vec3::Z);
        assert!(!scene.blocked(from, to, Some(source)));
        scene.solids[0].owner = Some(neighbour);
        assert!(scene.blocked(from, to, Some(source)));
        scene.solids[0].owner = None;
        assert!(scene.blocked(from, to, Some(source)));
    }

    #[test]
    fn plan30_tunnel_enclosure_requires_near_world_roof_and_walls() {
        let roof = Solid {
            min: Vec3::new(-10., 3., -10.),
            max: Vec3::new(10., 4., 10.),
            owner: None,
        };
        let mut obstacles = EffectObstacles {
            solids: vec![roof],
            ..default()
        };
        assert!(obstacles.overhead(Vec3::ZERO));
        assert_eq!(obstacles.enclosure(Vec3::ZERO), 0.);
        obstacles.solids.extend([
            Solid {
                min: Vec3::new(-4., 0., -10.),
                max: Vec3::new(-3., 4., 10.),
                owner: None,
            },
            Solid {
                min: Vec3::new(3., 0., -10.),
                max: Vec3::new(4., 4., 10.),
                owner: None,
            },
        ]);
        assert!((obstacles.enclosure(Vec3::Y) - 2. / 3.).abs() < 1e-6);
        assert_eq!(obstacles.enclosure(Vec3::new(100., 0., 0.)), 0.);
        for solid in &mut obstacles.solids {
            solid.owner = Some(Entity::PLACEHOLDER);
        }
        assert_eq!(
            obstacles.enclosure(Vec3::Y),
            0.,
            "the player's body is not a tunnel"
        );
    }
}

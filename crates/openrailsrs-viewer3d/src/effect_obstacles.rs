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
    pub solids: Vec<Solid>,
    elapsed: f32,
    center: Option<Vec3>,
    origin: Vec3,
    statics: Vec<Solid>,
    pub refreshes: u64,
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
    }
    let mut dynamic = Vec::new();
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
            if train {
                dynamic.push(solid);
            } else {
                obstacles.statics.push(solid);
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
}

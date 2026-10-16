//! Presentation of the authoritative moving deck and the stock it carries.
//! Coordinates are absolute world metres; floating origin is applied once.
use crate::{floating_origin::FloatingOrigin, live::LiveDrive};
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct YardDeck(pub String);

fn world_pose(
    center: [f64; 3],
    heading: f64,
    offset: Vec3,
    focus: &crate::world::RouteFocus,
    origin: &FloatingOrigin,
) -> Transform {
    // Subtract the large railway coordinates before converting to GPU floats.
    let relative = Vec3::new(
        (center[0] + f64::from(offset.x) - f64::from(focus.center.x) - f64::from(origin.shift.x))
            as f32,
        (center[1] + f64::from(offset.y) - f64::from(focus.height_origin)) as f32,
        (center[2] + f64::from(offset.z) - f64::from(focus.center.z) - f64::from(origin.shift.z))
            as f32,
    );
    Transform::from_translation(relative).with_rotation(Quat::from_rotation_y(-heading as f32))
}

pub(crate) fn car_pose(
    live: &LiveDrive,
    track: usize,
    index: usize,
    offset: Vec3,
    focus: &crate::world::RouteFocus,
    origin: &FloatingOrigin,
) -> Option<Transform> {
    let pose = live.session_for_track(track)?.yard_vehicle_pose(index)?;
    Some(world_pose(
        pose.center_world,
        pose.heading_rad,
        offset,
        focus,
        origin,
    ))
}

pub(crate) fn offset_pose(
    live: &LiveDrive,
    track: usize,
    car_offset: f64,
    point_offset: f64,
    offset: Vec3,
    focus: &crate::world::RouteFocus,
    origin: &FloatingOrigin,
) -> Option<Transform> {
    let session = live.session_for_track(track)?;
    let index = (0..session.formation.coupled_count).min_by(|a, b| {
        (session.formation.center_offset_m(*a) - car_offset)
            .abs()
            .total_cmp(&(session.formation.center_offset_m(*b) - car_offset).abs())
    })?;
    let center = session.formation.center_offset_m(index);
    let mut pose = car_pose(live, track, index, offset, focus, origin)?;
    pose.translation += pose.rotation * Vec3::X * (point_offset - center) as f32;
    Some(pose)
}

pub(crate) fn update(
    mut commands: Commands,
    live: Res<LiveDrive>,
    origin: Res<FloatingOrigin>,
    offset: Res<crate::world::RouteWorldOffset>,
    focus: Res<crate::world::RouteFocus>,
    mut meshes: Option<ResMut<Assets<Mesh>>>,
    mut materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut decks: Query<(Entity, &YardDeck, &mut Transform)>,
) {
    let poses = live.session.yard_device_poses();
    let existing = decks
        .iter()
        .map(|(_, d, _)| d.0.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for (entity, deck, mut transform) in &mut decks {
        let Some(p) = poses.iter().find(|p| p.id == deck.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        *transform = world_pose(p.center_world, p.heading_rad, offset.delta, &focus, &origin);
        transform.translation.y -= 0.15;
    }
    let (Some(meshes), Some(materials)) = (meshes.as_mut(), materials.as_mut()) else {
        return;
    };
    for pose in poses {
        if existing.contains(&pose.id) {
            continue;
        }
        let mesh = meshes.add(Cuboid::new(
            pose.deck_length_m as f32,
            0.3,
            pose.deck_width_m as f32,
        ));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.23, 0.25, 0.27),
            perceptual_roughness: 0.8,
            ..default()
        });
        let rail = meshes.add(Cuboid::new(pose.deck_length_m as f32, 0.12, 0.07));
        let steel = materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.57, 0.60),
            metallic: 0.8,
            perceptual_roughness: 0.45,
            ..default()
        });
        let mut transform = world_pose(
            pose.center_world,
            pose.heading_rad,
            offset.delta,
            &focus,
            &origin,
        );
        transform.translation.y -= 0.15;
        commands
            .spawn((
                Name::new(format!("Tablero operativo {}", pose.id)),
                YardDeck(pose.id),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
            ))
            .with_children(|parent| {
                for lateral in [-0.7175, 0.7175] {
                    parent.spawn((
                        Mesh3d(rail.clone()),
                        MeshMaterial3d(steel.clone()),
                        Transform::from_xyz(0., 0.21, lateral),
                    ));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn railway_scale_coordinates_keep_centimetres_before_gpu_conversion() {
        let focus = crate::world::RouteFocus {
            center: Vec3::new(12_000_000., 0., -30_000_000.),
            height_origin: 120.,
        };
        let p = world_pose(
            [12_000_000.037_5, 125.25, -30_000_000.047_5],
            0.,
            Vec3::ZERO,
            &focus,
            &FloatingOrigin { shift: Vec3::ZERO },
        );
        assert!((p.translation.x - 0.0375).abs() < 1e-7);
        assert!((p.translation.z + 0.0475).abs() < 1e-7);
        assert_eq!(p.translation.y, 5.25);
    }

    #[test]
    fn deck_and_load_keep_absolute_alignment_through_rebase_and_pause() {
        let focus = crate::world::RouteFocus {
            center: Vec3::new(10_000., 0., 20_000.),
            height_origin: 120.,
        };
        let offset = Vec3::new(500., 0., -300.);
        let a = FloatingOrigin { shift: Vec3::ZERO };
        let b = FloatingOrigin {
            shift: Vec3::new(256., 0., 512.),
        };
        let center = [10_100., 125., 20_200.];
        let angle = std::f64::consts::FRAC_PI_2;
        let first = world_pose(center, angle, offset, &focus, &a);
        let shifted = world_pose(center, angle, offset, &focus, &b);
        assert_eq!(first.translation, Vec3::new(600., 5., -100.));
        assert_eq!(first.translation - shifted.translation, b.shift);
        assert!((first.rotation * Vec3::X - Vec3::Z).length() < 1e-6);
        // A paused frame rebuilds an identical pose, never integrates rotation.
        assert_eq!(shifted, world_pose(center, angle, offset, &focus, &b));
        assert_eq!(first.rotation, shifted.rotation);
    }
}

//! Flexible parts keep their authored mesh and use the actual adjacent car poses.
use bevy::prelude::*;
use openrailsrs_scenarios::presentation::{ConnectionKind, FlexibleConnectionDef};
use std::collections::HashMap;
#[derive(Component)]
pub struct NativePartMatrix(pub String);
#[derive(Resource, Default)]
pub struct JacobsJoints(pub HashMap<(usize, String), Transform>);
#[derive(Component)]
pub struct FlexiblePart {
    source: Handle<Mesh>,
    output: Handle<Mesh>,
    original: Vec<[f32; 3]>,
    last_target: Option<Vec3>,
}
#[derive(Resource, Default, serde::Serialize)]
pub struct ConnectionDiagnostics {
    pub eligible_parts: usize,
    pub connected_parts: usize,
    pub deformed: u64,
    pub disconnected: u64,
    pub span_rejected: u64,
    pub maximum_endpoint_error_m: f32,
}
fn deform(points: &[[f32; 3]], definition: &FlexibleConnectionDef, target: Vec3) -> Vec<[f32; 3]> {
    let start = Vec3::from_array(definition.own_m);
    let rest = Vec3::from_array(definition.rest_far_m);
    let axis = rest - start;
    let denominator = axis.length_squared().max(1e-6);
    points
        .iter()
        .map(|point| {
            let p = Vec3::from_array(*point);
            let weight = ((p - start).dot(axis) / denominator).clamp(0., 1.);
            let sag = if matches!(
                definition.kind,
                ConnectionKind::AirHose | ConnectionKind::ElectricCable
            ) {
                -0.12 * (weight * std::f32::consts::PI).sin()
            } else {
                0.
            };
            (p + (target - rest) * weight + Vec3::Y * sag).to_array()
        })
        .collect()
}
pub fn update(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut last: Local<f64>,
    live: Res<crate::live::LiveDrive>,
    state: Res<crate::train_presentation::TrainPresentation>,
    mut diagnostics: ResMut<ConnectionDiagnostics>,
    mut meshes: ResMut<Assets<Mesh>>,
    cars: Query<(
        Entity,
        &crate::rolling_stock::ConsistCarIndex,
        &crate::rolling_stock_anim::TrainCarTrackOffset,
        &GlobalTransform,
    )>,
    mut parts: Query<(
        Entity,
        &ChildOf,
        &NativePartMatrix,
        &mut Mesh3d,
        &mut Visibility,
        Option<&mut FlexiblePart>,
    )>,
) {
    let _profile = crate::performance::profile_system("flexible_connections");
    if time.elapsed_secs_f64() - *last < 0.08 {
        return;
    }
    *last = time.elapsed_secs_f64();
    diagnostics.eligible_parts = 0;
    diagnostics.connected_parts = 0;
    let cars = cars
        .iter()
        .map(|(e, i, o, t)| (e, (i.0, o.track_index, t)))
        .collect::<HashMap<_, _>>();
    for (entity, parent, name, mut mesh, mut visibility, part) in &mut parts {
        let Some(&(index, track, transform)) = cars.get(&parent.parent()) else {
            continue;
        };
        let Some(definition) = state
            .vehicles
            .iter()
            .find(|v| v.vehicle == index)
            .and_then(|v| {
                v.connections
                    .iter()
                    .find(|d| d.matrix.eq_ignore_ascii_case(&name.0))
            })
        else {
            continue;
        };
        diagnostics.eligible_parts += 1;
        let Some((_, _, other)) = cars
            .values()
            .find(|(i, t, _)| *i == definition.neighbor && *t == track)
        else {
            continue;
        };
        let Some(session) = live.session_for_track(track) else {
            continue;
        };
        let connected = (index < session.formation.coupled_count)
            == (definition.neighbor < session.formation.coupled_count)
            && match definition.kind {
                ConnectionKind::AirHose => session
                    .formation
                    .cars
                    .get(index.max(definition.neighbor))
                    .is_some_and(|c| c.hose_connected),
                ConnectionKind::ElectricCable => session
                    .formation
                    .cars
                    .get(index.max(definition.neighbor))
                    .is_some_and(|c| c.train_bus_connected),
                _ => true,
            };
        if !connected {
            *visibility = Visibility::Hidden;
            diagnostics.disconnected += 1;
            continue;
        }
        let target = transform
            .affine()
            .inverse()
            .transform_point3(other.transform_point(Vec3::from_array(definition.other_m)));
        if target.distance(Vec3::from_array(definition.own_m)) > definition.maximum_span_m {
            *visibility = Visibility::Hidden;
            diagnostics.span_rejected += 1;
            continue;
        }
        *visibility = Visibility::Inherited;
        diagnostics.connected_parts += 1;
        let end = deform(&[definition.rest_far_m], definition, target)[0];
        diagnostics.maximum_endpoint_error_m = diagnostics
            .maximum_endpoint_error_m
            .max(Vec3::from_array(end).distance(target));
        let Some(mut part) = part else {
            let Some(source) = meshes.get(&mesh.0) else {
                continue;
            };
            let Some(points) = source
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .filter(|p| p.len() <= 5000)
            else {
                continue;
            };
            let original = points.to_vec();
            let source_handle = mesh.0.clone();
            let mut copy = source.clone();
            copy.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                deform(&original, definition, target),
            );
            mesh.0 = meshes.add(copy);
            diagnostics.deformed += 1;
            commands.entity(entity).insert(FlexiblePart {
                source: source_handle,
                output: mesh.0.clone(),
                original,
                last_target: Some(target),
            });
            continue;
        };
        if mesh.0 != part.output {
            // LOD may replace this entity's mesh. Bind the new immutable source
            // and deform a private copy rather than applying the old LOD's
            // vertex count to a different authored topology.
            let Some(source) = meshes.get(&mesh.0) else {
                continue;
            };
            let Some(points) = source
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .filter(|p| p.len() <= 5000)
            else {
                continue;
            };
            let original = points.to_vec();
            let source_handle = mesh.0.clone();
            let mut copy = source.clone();
            copy.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                deform(&original, definition, target),
            );
            mesh.0 = meshes.add(copy);
            part.source = source_handle;
            part.output = mesh.0.clone();
            part.original = original;
            part.last_target = Some(target);
            diagnostics.deformed += 1;
        }
        if part
            .last_target
            .is_some_and(|p| p.distance_squared(target) < 1e-6)
        {
            continue;
        }
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            m.insert_attribute(
                Mesh::ATTRIBUTE_POSITION,
                deform(&part.original, definition, target),
            );
            part.last_target = Some(target);
            diagnostics.deformed += 1;
        }
        // Retain the original, immutable source handle across all LODs and saves.
        let _ = &part.source;
    }
}
#[allow(clippy::type_complexity)]
pub fn shared_bogies(
    joints: Res<JacobsJoints>,
    cars: Query<
        (&crate::rolling_stock::ConsistCarIndex, &Transform, &ChildOf),
        Without<crate::rolling_stock_anim::TrainExteriorAnimPart>,
    >,
    roots: Query<
        &Transform,
        (
            Without<crate::rolling_stock::ConsistCarIndex>,
            Without<crate::rolling_stock_anim::TrainExteriorAnimPart>,
        ),
    >,
    mut parts: Query<
        (
            &NativePartMatrix,
            &ChildOf,
            &openrailsrs_bevy_scenery::shapes::ShapeAnimBinding,
            &mut Transform,
        ),
        (
            With<crate::rolling_stock_anim::TrainBogieAnim>,
            With<crate::rolling_stock_anim::TrainExteriorAnimPart>,
        ),
    >,
) {
    let _profile = crate::performance::profile_system("jacobs_bogie");
    for (name, parent, binding, mut transform) in &mut parts {
        let Ok((index, car, root)) = cars.get(parent.parent()) else {
            continue;
        };
        let Some(target) = joints.0.get(&(index.0, name.0.to_ascii_uppercase())) else {
            continue;
        };
        let Ok(root) = roots.get(root.parent()) else {
            continue;
        };
        let world = *root * *car;
        let desired = world.to_matrix().inverse() * target.to_matrix();
        let desired = Transform::from_matrix(desired);
        let pivot = openrailsrs_or_shader::coordinates::static_hierarchy_chain_transform(
            &binding.shape,
            binding.matrix_idx,
        )
        .translation;
        transform.rotation = desired.rotation;
        transform.translation = desired.translation
            + desired.rotation * Vec3::new(pivot.x, pivot.y, 0.)
            - transform.rotation * pivot;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bogie_part_queries_are_disjoint_from_car_and_train_root_transforms() {
        let mut app = App::new();
        app.init_resource::<JacobsJoints>()
            .add_systems(Update, shared_bogies);
        app.update();
    }
    #[test]
    fn lod_rebind_keeps_authored_topology_and_sources_immutable_through_pause_cut_and_restore() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/player_lab/scenario_connections.toml");
        let definition = openrailsrs_scenarios::load_scenario(&path)
            .unwrap()
            .train
            .presentation[0]
            .clone();
        let drive = crate::live::LiveDrive::from_scenario_path(&path).unwrap();
        let mut presentation = crate::train_presentation::TrainPresentation::default();
        presentation.vehicles = vec![definition];
        let mut app = App::new();
        app.insert_resource(Time::<Real>::default())
            .insert_resource(drive)
            .insert_resource(presentation)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<ConnectionDiagnostics>()
            .add_systems(Update, update);
        let car = app
            .world_mut()
            .spawn((
                crate::rolling_stock::ConsistCarIndex(0),
                crate::rolling_stock_anim::TrainCarTrackOffset {
                    offset_m: -10.,
                    track_index: 0,
                    flipped: false,
                },
                GlobalTransform::default(),
            ))
            .id();
        app.world_mut().spawn((
            crate::rolling_stock::ConsistCarIndex(1),
            crate::rolling_stock_anim::TrainCarTrackOffset {
                offset_m: -30.,
                track_index: 0,
                flipped: false,
            },
            GlobalTransform::from_translation(Vec3::new(0., 0., 20.)),
        ));
        let original = vec![[0., 2., 9.8], [0., 2., 10.2]];
        let make = |points: Vec<[f32; 3]>| {
            let mut m = Mesh::new(
                bevy::mesh::PrimitiveTopology::TriangleList,
                bevy::asset::RenderAssetUsages::MAIN_WORLD,
            );
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, points);
            m
        };
        let source = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(make(original.clone()));
        let part = app
            .world_mut()
            .spawn((
                NativePartMatrix("GANGWAY".into()),
                ChildOf(car),
                Mesh3d(source.clone()),
                Visibility::Inherited,
            ))
            .id();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        assert_eq!(
            app.world()
                .resource::<ConnectionDiagnostics>()
                .connected_parts,
            1
        );
        assert_eq!(
            app.world()
                .get::<FlexiblePart>(part)
                .unwrap()
                .original
                .len(),
            2
        );
        let next_vertices = vec![
            [0., 2., 9.8],
            [0., 2., 10.2],
            [0.2, 2., 10.2],
            [0.2, 2., 9.8],
        ];
        let lod = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(make(next_vertices.clone()));
        app.world_mut().entity_mut(part).insert(Mesh3d(lod.clone()));
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        assert_eq!(
            app.world()
                .get::<FlexiblePart>(part)
                .unwrap()
                .original
                .len(),
            4
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&source)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap(),
            original
        );
        assert_eq!(
            app.world()
                .resource::<Assets<Mesh>>()
                .get(&lod)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap(),
            next_vertices
        );
        {
            let mut live = app.world_mut().resource_mut::<crate::live::LiveDrive>();
            live.session
                .operate_car(1, openrailsrs_sim::CarOperation::Handbrake)
                .unwrap();
            live.session.uncouple_after(0).unwrap();
            let saved = live.session.snapshot();
            live.session.restore_snapshot(saved).unwrap();
            live.paused = true;
        }
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(part).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(
            app.world()
                .resource::<ConnectionDiagnostics>()
                .connected_parts,
            0
        );
    }
    #[test]
    fn authored_endpoints_follow_curve_slack_and_reversal_without_changing_the_near_end() {
        for kind in [
            ConnectionKind::Gangway,
            ConnectionKind::AirHose,
            ConnectionKind::ElectricCable,
            ConnectionKind::Coupler,
        ] {
            let d = FlexibleConnectionDef {
                matrix: "GANGWAY_REAR".into(),
                neighbor: 1,
                own_m: [0., 2., 10.],
                rest_far_m: [0., 2., 11.],
                other_m: [0., 2., -10.],
                kind,
                maximum_span_m: 2.,
            };
            for far in [Vec3::new(0.5, 2.1, 10.9), Vec3::new(-0.4, 1.9, 11.2)] {
                let p = deform(&[d.own_m, d.rest_far_m], &d, far);
                assert_eq!(p[0], d.own_m);
                assert!(Vec3::from_array(p[1]).distance(far) < 1e-6);
            }
        }
    }
}

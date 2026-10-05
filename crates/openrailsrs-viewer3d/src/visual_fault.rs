//! Explicit capture-only faults used to prove pixel goldens detect real renderer failures.
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use std::collections::HashSet;

#[derive(Component)]
pub struct VisualOccluder;

pub fn inject(
    mut commands: Commands,
    camera: Query<&Transform, (With<Camera3d>, Without<VisualOccluder>)>,
    mut train: Query<&mut Visibility, With<crate::live::LiveTrainBody>>,
    mut occluder: Query<&mut Transform, With<VisualOccluder>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cab: Query<&Mesh3d, With<crate::cab_view::CabInteriorMarker>>,
    mut flipped: Local<HashSet<bevy::asset::AssetId<Mesh>>>,
) {
    if !crate::capture::capture_enabled() {
        return;
    }
    match std::env::var("OPENRAILSRS_VISUAL_FAULT").ok().as_deref() {
        Some("mirror") => {
            for handle in &cab {
                if flipped.contains(&handle.0.id()) {
                    continue;
                }
                if let Some(mut mesh) = meshes.get_mut(&handle.0)
                    && let Some(VertexAttributeValues::Float32x2(values)) =
                        mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
                {
                    for uv in values {
                        uv[0] = 1.0 - uv[0];
                    }
                    flipped.insert(handle.0.id());
                }
            }
        }
        Some("hide_train") => {
            for mut v in &mut train {
                *v = Visibility::Hidden;
            }
        }
        Some("occluder") => {
            let Ok(camera) = camera.single() else { return };
            let transform = Transform {
                translation: camera.translation + camera.forward().as_vec3() * 0.5,
                rotation: camera.rotation,
                ..default()
            };
            if let Ok(mut existing) = occluder.single_mut() {
                *existing = transform;
            } else {
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(3.0, 3.0, 0.02))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color: Color::BLACK,
                        unlit: true,
                        ..default()
                    })),
                    transform,
                    VisualOccluder,
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            }
        }
        _ => {}
    }
}

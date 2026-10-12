//! Near-field depth ray shadows; no camera-mounted mesh or persistent world decal.
use bevy::{core_pipeline::prepass::DepthPrepass, pbr::ContactShadows, prelude::*};
#[derive(Component)]
pub(crate) struct OwnedDepth;
pub fn update(
    mut commands: Commands,
    settings: Res<crate::player_settings::PlayerSettings>,
    follow: Option<Res<crate::camera::CameraFollowMode>>,
    cameras: Query<
        (
            Entity,
            Option<&ContactShadows>,
            Option<&DepthPrepass>,
            Has<OwnedDepth>,
        ),
        With<crate::camera::OrbitState>,
    >,
    mut lights: Query<&mut DirectionalLight>,
) {
    let steps = if settings.shadows
        && follow
            .as_ref()
            .is_none_or(|f| !(**f == crate::camera::CameraFollowMode::DriverCam || f.is_cab2d()))
    {
        match settings.contact_shadow_quality {
            1 => 8,
            2 => 16,
            _ => 0,
        }
    } else {
        0
    };
    for mut light in &mut lights {
        if light.contact_shadows_enabled != (steps > 0) {
            light.contact_shadows_enabled = steps > 0;
        }
    }
    for (entity, current, depth, owned) in &cameras {
        if steps == 0 {
            if current.is_some() {
                commands.entity(entity).remove::<ContactShadows>();
                if owned {
                    commands
                        .entity(entity)
                        .remove::<(DepthPrepass, OwnedDepth)>();
                }
            }
        } else if current.is_none_or(|c| c.linear_steps != steps) {
            if depth.is_none() {
                commands.entity(entity).insert(OwnedDepth);
            }
            commands.entity(entity).insert(ContactShadows {
                linear_steps: steps,
                thickness: 0.04,
                length: 0.35,
            });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cab_disable_restores_only_the_depth_pass_owned_by_contact_shadows() {
        let mut app = App::new();
        app.insert_resource(crate::player_settings::PlayerSettings {
            shadows: true,
            contact_shadow_quality: 1,
            ..default()
        })
        .insert_resource(crate::camera::CameraFollowMode::OrbitFollow)
        .add_systems(Update, update);
        let a = app
            .world_mut()
            .spawn((crate::camera::OrbitState::default(), Camera3d::default()))
            .id();
        let b = app
            .world_mut()
            .spawn((
                crate::camera::OrbitState::default(),
                Camera3d::default(),
                DepthPrepass,
            ))
            .id();
        app.update();
        assert!(app.world().get::<ContactShadows>(a).is_some());
        assert!(app.world().get::<OwnedDepth>(a).is_some());
        assert!(app.world().get::<OwnedDepth>(b).is_none());
        *app.world_mut()
            .resource_mut::<crate::camera::CameraFollowMode>() =
            crate::camera::CameraFollowMode::DriverCam;
        app.update();
        assert!(app.world().get::<ContactShadows>(a).is_none());
        assert!(app.world().get::<DepthPrepass>(a).is_none());
        assert!(app.world().get::<DepthPrepass>(b).is_some());
    }
}

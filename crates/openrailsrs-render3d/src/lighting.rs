//! Sol direccional, sombras en cascada y ambiente global.

use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;

use crate::SceneExtent;
use crate::activity::{self, ActivitySession};
use crate::or_cascade::{
    OR_SHADOW_CASCADE_COUNT, cascade_shadow_config_from_or_limits, or_limits_from_view_distance,
    or_max_shadow_view_distance,
};
use crate::textures::TextureEnvironment;

/// Sol/moon + ambiente global. Se ejecuta una sola vez al arrancar (no duplicar en `finish_loading`).
pub fn spawn_scene_sun(
    mut commands: Commands,
    mut ambient: ResMut<GlobalAmbientLight>,
    activity: Option<Res<ActivitySession>>,
    texture_env: Res<TextureEnvironment>,
    extent: Res<SceneExtent>,
) {
    let start_time_s = activity
        .as_ref()
        .map(|a| a.start_time_s)
        .unwrap_or(12.0 * 3600.0);
    let (rotation, illuminance, sun_color, ambient_color) =
        activity::sun_transform(start_time_s, texture_env.night);

    let night = texture_env.night;
    let side = extent.side_m.max(256.0);
    let max_shadow_dist = or_max_shadow_view_distance(side);
    let or_limits = or_limits_from_view_distance(max_shadow_dist);

    if night {
        commands.spawn((
            DirectionalLight {
                color: sun_color,
                illuminance,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_rotation(rotation),
            Name::new("moon"),
        ));
    } else {
        let cascade = cascade_shadow_config_from_or_limits(or_limits, 0.5, 0.2);
        commands.spawn((
            DirectionalLight {
                color: sun_color,
                illuminance: illuminance.max(25_000.0),
                shadow_maps_enabled: true,
                ..default()
            },
            cascade,
            Transform::from_rotation(rotation),
            Name::new("sun"),
        ));
        commands.insert_resource(DirectionalLightShadowMap { size: 2048 });
        commands.insert_resource(crate::or_vsm_moments::OrVsmCascadeLimits { limits: or_limits });
        let _ = OR_SHADOW_CASCADE_COUNT;
    }

    *ambient = GlobalAmbientLight {
        color: ambient_color,
        brightness: if night { 40.0 } else { 160.0 },
        affects_lightmapped_meshes: false,
    };
}

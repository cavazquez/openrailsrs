//! Sol direccional, sombras en cascada y ambiente global.

use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{SceneSunLight, directional_light_from_sun, sun_transform};

use crate::SceneExtent;
use crate::activity::ActivitySession;
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
    let night = texture_env.night;
    let mut sun = SceneSunLight::from_msts_start_time(start_time_s, night);
    if !night {
        sun.illuminance = sun.illuminance.max(25_000.0);
    }

    let side = extent.side_m.max(256.0);
    let max_shadow_dist = or_max_shadow_view_distance(side);
    let or_limits = or_limits_from_view_distance(max_shadow_dist);

    if night {
        commands.spawn((
            directional_light_from_sun(&sun, false),
            sun_transform(&sun),
            Name::new("moon"),
        ));
    } else {
        let cascade = cascade_shadow_config_from_or_limits(or_limits, 0.5, 0.2);
        commands.spawn((
            directional_light_from_sun(&sun, true),
            cascade,
            sun_transform(&sun),
            Name::new("sun"),
        ));
        commands.insert_resource(DirectionalLightShadowMap { size: 2048 });
        commands.insert_resource(crate::or_vsm_moments::OrVsmCascadeLimits { limits: or_limits });
        let _ = OR_SHADOW_CASCADE_COUNT;
    }

    *ambient = GlobalAmbientLight {
        color: sun.ambient_color,
        brightness: sun.ambient_brightness,
        affects_lightmapped_meshes: false,
    };
}

//! Route-authored solar direction with Bevy's lighting, shadows and tonemapping.

use bevy::prelude::*;
use openrailsrs_bevy_scenery::lighting::{season_ordinal, solar_direction};
use openrailsrs_formats::{
    EnvironmentSun, RouteFile,
    geography::{GeographicPosition, geographic_position},
};

use crate::{
    floating_origin::FloatingOrigin, live::LiveDrive, shapes::RouteAssets, world::RouteFocus,
};

#[derive(Component)]
pub struct RouteSunLight;

#[derive(Resource)]
pub struct RouteSunState {
    pub position: GeographicPosition,
    pub environment: Option<EnvironmentSun>,
    pub direction: Vec3,
    pub ambient_scale: f32,
    last_clock_s: Option<f64>,
}

/// Load once after the initial camera is placed. Native OR also anchors its sky
/// lookup to the initial viewer location; camera panning must not move the sun.
pub fn init_route_sun(
    mut commands: Commands,
    live: Res<LiveDrive>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    origin: Res<FloatingOrigin>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    let Ok(camera) = camera.single() else { return };
    let world = camera.translation + focus.center + origin.shift;
    let tile_x = openrailsrs_formats::msts_tile_x_index_for_coord(world.x);
    let tile_z = openrailsrs_formats::msts_tile_z_index_for_coord(world.z);
    let local_x = f64::from(world.x) - f64::from(tile_x) * 2048.0;
    let local_z = -f64::from(world.z) - f64::from(tile_z) * 2048.0;
    let Some(position) = geographic_position(tile_x, tile_z, local_x, local_z) else {
        return;
    };
    let env_path = RouteFile::from_route_dir(&assets.route_dir)
        .ok()
        .and_then(|route| route.environment_path(&assets.route_dir, &live.season, "Clear"));
    let environment = env_path
        .as_deref()
        .and_then(|path| EnvironmentSun::from_path(path).ok().flatten());
    crate::viewer_log!(
        "openrailsrs-viewer3d: route sun — {:.5}°, {:.5}°, season {}, clock {:.0}s, environment {}",
        position.latitude.to_degrees(),
        position.longitude.to_degrees(),
        live.season,
        live.clock_time_s(),
        env_path
            .as_deref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "astronomical fallback".into())
    );
    commands.insert_resource(RouteSunState {
        position,
        environment,
        direction: Vec3::Y,
        ambient_scale: 1.0,
        last_clock_s: None,
    });
}

pub fn update_route_sun(
    live: Res<LiveDrive>,
    state: Option<ResMut<RouteSunState>>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<RouteSunLight>>,
) {
    let Some(mut state) = state else { return };
    let clock = live.clock_time_s();
    // A one-second simulation cadence bounds shadow invalidations; pause costs nothing.
    if state
        .last_clock_s
        .is_some_and(|last| (clock - last).abs() < 1.0)
    {
        return;
    }
    state.last_clock_s = Some(clock);
    let ordinal = season_ordinal(&live.season, state.position.latitude);
    let direction = solar_direction(state.position, ordinal, clock, state.environment);
    if !direction.is_finite() || direction.length_squared() < 0.5 {
        return;
    }
    state.direction = direction;
    let daylight = (direction.y * 2.0).clamp(0.0, 1.0);
    state.ambient_scale = 0.02 + 0.98 * daylight;
    let up = if direction.y.abs() > 0.999 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    for (mut transform, mut light) in &mut sun {
        transform.set_if_neq(Transform::IDENTITY.looking_to(-direction, up));
        light.illuminance = 75_000.0 * daylight;
    }
}

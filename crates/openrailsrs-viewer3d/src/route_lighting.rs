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

fn ambient_scale(direction: Vec3) -> f32 {
    // The night camera exposes for headlamps. Two percent of the daytime
    // 15,000-lux fill would wash the whole town white at that exposure.
    0.0001 + 0.9999 * (direction.y * 2.0).clamp(0.0, 1.0)
}

fn route_sun_at(live: &LiveDrive, assets: &RouteAssets, world: Vec3) -> Option<RouteSunState> {
    let tile_x = openrailsrs_formats::msts_tile_x_index_for_coord(world.x);
    let tile_z = openrailsrs_formats::msts_tile_z_index_for_coord(world.z);
    let local_x = f64::from(world.x) - f64::from(tile_x) * 2048.0;
    let local_z = -f64::from(world.z) - f64::from(tile_z) * 2048.0;
    let native_position = geographic_position(tile_x, tile_z, local_x, local_z);
    let position = crate::environment::location_for_route(assets, native_position)
        .0
        .map(|p| p.geographic())
        .or(native_position)?;
    let environment = RouteFile::from_route_dir(&assets.route_dir)
        .ok()
        .and_then(|route| route.environment_path(&assets.route_dir, &live.season, "Clear"))
        .and_then(|path| EnvironmentSun::from_path(&path).ok().flatten());
    let direction = solar_direction(
        position,
        season_ordinal(&live.season, position.latitude),
        live.clock_time_s(),
        environment,
    );
    Some(RouteSunState {
        position,
        environment,
        direction,
        ambient_scale: ambient_scale(direction),
        last_clock_s: None,
    })
}

/// Publish the launch environment before any train, cab or scenery textures load.
pub fn prepare_route_textures(
    live: Option<&LiveDrive>,
    assets: &RouteAssets,
    center: Vec3,
    selection: crate::environment::EnvironmentSelection,
) {
    crate::shapes::set_scenery_season(live.map(|l| l.season.as_str()).unwrap_or("summer"));
    if let Some(live) = live
        && let Some(sun) = route_sun_at(live, assets, center)
    {
        let real = selection.time == crate::environment::EnvironmentSource::LocalNow
            && crate::environment::location_for_route(assets, Some(sun.position))
                .0
                .is_some();
        let direction = if real {
            crate::environment::real_solar_direction(sun.position, chrono::Utc::now())
        } else {
            sun.direction
        };
        crate::shapes::set_scenery_sun_y(direction.y);
    }
}

/// Load once after the initial camera is placed. Native OR also anchors its sky
/// lookup to the initial viewer location; camera panning must not move the sun.
pub fn init_route_sun(
    mut commands: Commands,
    live: Res<LiveDrive>,
    assets: Res<RouteAssets>,
    focus: Res<RouteFocus>,
    origin: Res<FloatingOrigin>,
    content: Res<crate::player_launch::ActivePlayerContent>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    let Ok(camera) = camera.single() else { return };
    let world = camera.translation + focus.center + origin.shift;
    let Some(mut state) = route_sun_at(&live, &assets, world) else {
        return;
    };
    if content.environment.time == crate::environment::EnvironmentSource::LocalNow
        && crate::environment::location_for_route(&assets, Some(state.position))
            .0
            .is_some()
    {
        state.direction =
            crate::environment::real_solar_direction(state.position, chrono::Utc::now());
        state.ambient_scale = ambient_scale(state.direction);
    }
    crate::shapes::set_scenery_sun_y(state.direction.y);
    crate::viewer_log!(
        "openrailsrs-viewer3d: route sun — {:.5}°, {:.5}°, season {}, clock {:.0}s, environment {}",
        state.position.latitude.to_degrees(),
        state.position.longitude.to_degrees(),
        live.season,
        live.clock_time_s(),
        if state.environment.is_some() {
            "route ENV"
        } else {
            "astronomical fallback"
        }
    );
    commands.insert_resource(state);
}

pub fn update_route_sun(
    live: Res<LiveDrive>,
    state: Option<ResMut<RouteSunState>>,
    settings: Res<crate::player_settings::PlayerSettings>,
    weather: Option<Res<crate::weather_state::WeatherState>>,
    environment: Res<crate::environment::LiveEnvironment>,
    content: Res<crate::player_launch::ActivePlayerContent>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<RouteSunLight>>,
) {
    let Some(mut state) = state else { return };
    let real = environment.solar_utc(content.environment);
    let clock = real.map_or_else(|| live.clock_time_s(), |t| t.timestamp() as f64);
    // A one-second simulation cadence bounds shadow invalidations; pause costs nothing.
    if state
        .last_clock_s
        .is_some_and(|last| (clock - last).abs() < 1.0)
    {
        return;
    }
    state.last_clock_s = Some(clock);
    let ordinal = season_ordinal(&live.season, state.position.latitude);
    let direction = if let Some(real) = real {
        crate::environment::real_solar_direction(
            environment
                .location
                .map_or(state.position, |p| p.geographic()),
            real,
        )
    } else {
        solar_direction(state.position, ordinal, clock, state.environment)
    };
    if !direction.is_finite() || direction.length_squared() < 0.5 {
        return;
    }
    state.direction = direction;
    crate::shapes::set_scenery_sun_y(direction.y);
    let daylight = (direction.y * 2.0).clamp(0.0, 1.0);
    state.ambient_scale = ambient_scale(direction);
    let up = if direction.y.abs() > 0.999 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    for (mut transform, mut light) in &mut sun {
        transform.set_if_neq(Transform::IDENTITY.looking_to(-direction, up));
        // Bevy omits a zero-intensity directional light from the view uniform.
        // Legacy shaders still need its below-horizon direction to shade night.
        let overcast = weather.as_ref().map_or(0.0, |s| s.atmosphere.overcast);
        light.illuminance = (75_000.0 * daylight * (1.0 - overcast * 0.8)).max(0.001);
        light.shadow_maps_enabled = settings.shadows && daylight > 0.005;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::{RailwaySkyMaterial, sky_parameters};
    use bevy::pbr::{DistanceFog, FogFalloff};
    use openrailsrs_bevy_scenery::SkyDome;

    use crate::player_launch::{ActivePlayerContent, PlayerWeather};
    use crate::sky::{FogState, sync_route_atmosphere, viewer_distance_fog};

    #[test]
    fn nighttime_fill_stays_dim_at_the_headlamp_camera_exposure() {
        let night_exposure = bevy::camera::Exposure { ev100: 6.0 }.exposure();
        let exposed_fill =
            crate::camera::LIVE_OUTDOOR_AMBIENT * ambient_scale(Vec3::NEG_Y) * night_exposure;
        assert!(
            exposed_fill < 0.03,
            "night fill washed out the scene: {exposed_fill}"
        );
        assert_eq!(ambient_scale(Vec3::Y), 1.0);
    }

    fn atmosphere_app(direction: Vec3) -> (App, Handle<RailwaySkyMaterial>, Entity) {
        let mut app = App::new();
        app.insert_resource(RouteSunState {
            position: GeographicPosition {
                latitude: 51.55_f64.to_radians(),
                longitude: -0.37_f64.to_radians(),
            },
            environment: None,
            direction,
            ambient_scale: 1.0,
            last_clock_s: None,
        })
        .init_resource::<ActivePlayerContent>()
        .init_resource::<FogState>()
        .init_resource::<ClearColor>()
        .init_resource::<Assets<RailwaySkyMaterial>>()
        .add_systems(Update, sync_route_atmosphere);
        let material = app
            .world_mut()
            .resource_mut::<Assets<RailwaySkyMaterial>>()
            .add(RailwaySkyMaterial {
                params: sky_parameters(direction.y, PlayerWeather::Clear, direction, 0.0),
            });
        app.world_mut()
            .spawn((SkyDome, MeshMaterial3d(material.clone())));
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), viewer_distance_fog(20_000.0, false)))
            .id();
        (app, material, camera)
    }

    fn assert_palette(app: &App, material: &Handle<RailwaySkyMaterial>, night: bool) {
        let sky = app
            .world()
            .resource::<Assets<RailwaySkyMaterial>>()
            .get(material)
            .unwrap();
        let clear = LinearRgba::from(app.world().resource::<ClearColor>().0);
        assert!(
            (Vec3::new(clear.red, clear.green, clear.blue) - sky.params.horizon.truncate())
                .length()
                < 1e-6
        );
        assert_eq!(sky.params.sun.w, if night { 0.0 } else { 1.0 });
        assert!(sky.params.zenith.z >= sky.params.zenith.x);
    }

    #[test]
    fn sky_and_camera_fog_follow_sunset_weather_and_sunrise() {
        let (mut app, material, camera) = atmosphere_app(Vec3::Y);
        app.update();
        assert_palette(&app, &material, false);
        assert!(matches!(
            app.world().get::<DistanceFog>(camera).unwrap().falloff,
            FogFalloff::Atmospheric { .. }
        ));
        let day_clear = LinearRgba::from(app.world().resource::<ClearColor>().0);
        let FogFalloff::Atmospheric {
            extinction: day_extinction,
            ..
        } = app.world().get::<DistanceFog>(camera).unwrap().falloff
        else {
            unreachable!()
        };
        app.world_mut().resource_mut::<RouteSunState>().direction = Vec3::NEG_Y;
        app.update();
        assert_palette(&app, &material, true);
        let fog = app.world().get::<DistanceFog>(camera).unwrap();
        assert_eq!(
            fog.color,
            app.world().resource::<ClearColor>().0.with_alpha(0.94)
        );
        assert!(matches!(fog.falloff, FogFalloff::Atmospheric { .. }));
        let night_clear = LinearRgba::from(app.world().resource::<ClearColor>().0);
        assert!(
            night_clear.red + night_clear.green + night_clear.blue
                < (day_clear.red + day_clear.green + day_clear.blue) * 0.1
        );

        app.world_mut()
            .resource_mut::<ActivePlayerContent>()
            .weather = PlayerWeather::Fog;
        app.world_mut().resource_mut::<RouteSunState>().direction = Vec3::Y;
        app.update();
        assert_palette(&app, &material, false);
        let fog = app.world().get::<DistanceFog>(camera).unwrap();
        assert_eq!(
            fog.color,
            app.world().resource::<ClearColor>().0.with_alpha(0.94)
        );
        let FogFalloff::Atmospheric { extinction, .. } = fog.falloff else {
            panic!("sunrise must restore daytime fog");
        };
        assert!(
            (extinction / day_extinction).min_element() > 30.0,
            "fog must have a shorter visibility than clear weather"
        );
    }

    #[test]
    fn fog_toggle_and_added_camera_keep_night_palette() {
        let (mut app, material, camera) = atmosphere_app(Vec3::NEG_Y);
        app.update();
        app.world_mut().resource_mut::<FogState>().enabled = false;
        app.update();
        assert!(matches!(
            app.world().get::<DistanceFog>(camera).unwrap().falloff,
            FogFalloff::Exponential { density } if density == 0.0
        ));
        app.world_mut().resource_mut::<FogState>().enabled = true;
        app.update();
        assert_eq!(
            app.world().get::<DistanceFog>(camera).unwrap().color,
            app.world().resource::<ClearColor>().0.with_alpha(0.94)
        );
        let added_camera = app
            .world_mut()
            .spawn((Camera3d::default(), viewer_distance_fog(20_000.0, false)))
            .id();
        app.update();
        assert_palette(&app, &material, true);
        assert_eq!(
            app.world().get::<DistanceFog>(added_camera).unwrap().color,
            app.world().resource::<ClearColor>().0.with_alpha(0.94)
        );
    }
}

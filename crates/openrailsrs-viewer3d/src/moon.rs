//! Low-precision lunar orbit for presentation, not a navigational ephemeris.
//! Elements: Paul Schlyter https://stjarnhimlen.se/comp/ppcomp.html .
use bevy::{
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use chrono::{NaiveDate, TimeZone, Utc};
use openrailsrs_formats::geography::GeographicPosition;

#[derive(Clone, Copy, Debug)]
pub struct LunarPosition {
    pub direction: Vec3,
    pub sun_direction: Vec3,
    pub illuminated: f32,
}
fn orbit(m: f64, e: f64) -> (f64, f64) {
    let mut eccentric = m;
    for _ in 0..8 {
        eccentric -= (eccentric - e * eccentric.sin() - m) / (1. - e * eccentric.cos());
    }
    let x = eccentric.cos() - e;
    let y = (1. - e * e).sqrt() * eccentric.sin();
    (y.atan2(x), x.hypot(y))
}
/// Mean elements plus the three largest longitude perturbations. Topocentric
/// parallax is applied as a Cartesian observer subtraction; no atmosphere refraction.
pub fn position(timestamp: i64, geo: GeographicPosition) -> LunarPosition {
    let d = (timestamp as f64 - 946598400.) / 86400.;
    let radians = |v: f64| v.rem_euclid(360.).to_radians();
    let n = radians(125.1228 - 0.0529538083 * d);
    let w = radians(318.0634 + 0.1643573223 * d);
    let m = radians(115.3654 + 13.0649929509 * d);
    let inclination = 5.1454_f64.to_radians();
    let (v, r) = orbit(m, 0.0549);
    let a = v + w;
    let x = r * (n.cos() * a.cos() - n.sin() * a.sin() * inclination.cos());
    let y = r * (n.sin() * a.cos() + n.cos() * a.sin() * inclination.cos());
    let z = r * a.sin() * inclination.sin();
    let ms = radians(356.0470 + 0.9856002585 * d);
    let ws = radians(282.9404 + 0.0000470935 * d);
    let (vs, _) = orbit(ms, 0.016709 - 1.151e-9 * d);
    let sun_lon = vs + ws;
    let elong = m + w + n - ms - ws;
    let lon = y.atan2(x)
        + (-1.274 * (m - 2. * elong).sin() + 0.658 * (2. * elong).sin() - 0.186 * ms.sin())
            .to_radians();
    let lat = z.atan2(x.hypot(y)) - 0.173_f64.to_radians() * (m + w - 2. * elong).sin();
    let tilt = (23.4393 - 3.563e-7 * d).to_radians();
    let ecliptic = |lon: f64, lat: f64| {
        bevy::math::DVec3::new(
            lon.cos() * lat.cos(),
            lon.sin() * lat.cos() * tilt.cos() - lat.sin() * tilt.sin(),
            lon.sin() * lat.cos() * tilt.sin() + lat.sin() * tilt.cos(),
        )
    };
    let ut = (timestamp as f64).rem_euclid(86400.) / 86400. * std::f64::consts::TAU;
    let sidereal = ms + ws + std::f64::consts::PI + ut + geo.longitude;
    let east = bevy::math::DVec3::new(-sidereal.sin(), sidereal.cos(), 0.);
    let up = bevy::math::DVec3::new(
        geo.latitude.cos() * sidereal.cos(),
        geo.latitude.cos() * sidereal.sin(),
        geo.latitude.sin(),
    );
    let north = up.cross(east);
    let local = |dir: bevy::math::DVec3| {
        Vec3::new(
            dir.dot(east) as f32,
            dir.dot(up) as f32,
            -dir.dot(north) as f32,
        )
        .normalize()
    };
    let moon = ecliptic(lon, lat);
    let sun = ecliptic(sun_lon, 0.);
    LunarPosition {
        direction: local(moon * (r * 60.2666) - up),
        sun_direction: local(sun),
        illuminated: ((1. - moon.dot(sun)) / 2.).clamp(0., 1.) as f32,
    }
}
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct MoonMaterial {
    #[uniform(0)]
    pub params: MoonParameters,
}
#[derive(ShaderType, Clone, Copy, Debug)]
pub struct MoonParameters {
    pub light: Vec4,
}
impl bevy::pbr::Material for MoonMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/moon.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}
#[derive(Component)]
pub struct MoonDisk;
#[derive(Component)]
pub struct MoonLight;
#[derive(Resource, Default)]
pub struct MoonState {
    pub illuminated: f32,
    pub altitude_deg: f32,
    pub date: String,
    pub lux: f32,
}
pub fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MoonMaterial>>,
) {
    commands.spawn((
        MoonDisk,
        Name::new("moon · dated lunar orbit"),
        Mesh3d(meshes.add(Rectangle::new(2., 2.))),
        MeshMaterial3d(materials.add(MoonMaterial {
            params: MoonParameters { light: Vec4::ZERO },
        })),
        Transform::IDENTITY,
        Visibility::Hidden,
        bevy::camera::visibility::NoFrustumCulling,
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
    ));
    commands.spawn((
        MoonLight,
        Name::new("moonlight"),
        DirectionalLight {
            illuminance: 0.,
            color: Color::srgb(0.73, 0.81, 1.),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::IDENTITY,
    ));
}
pub fn manual_utc(
    date: &str,
    clock: f64,
    timezone: Option<chrono_tz::Tz>,
    longitude: f64,
) -> Option<chrono::DateTime<Utc>> {
    let local = date.parse::<NaiveDate>().ok()?.and_hms_opt(0, 0, 0)?
        + chrono::Duration::seconds(clock.max(0.) as i64);
    timezone
        .and_then(|t| t.from_local_datetime(&local).earliest())
        .map(|t| t.with_timezone(&Utc))
        .or_else(|| {
            Some(
                (local - chrono::Duration::seconds((longitude.to_degrees() / 15. * 3600.) as i64))
                    .and_utc(),
            )
        })
}
#[allow(clippy::type_complexity)]
pub fn update(
    settings: Res<crate::player_settings::PlayerSettings>,
    live: Option<Res<crate::live::LiveDrive>>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    env: Option<Res<crate::environment::LiveEnvironment>>,
    content: Res<crate::player_launch::ActivePlayerContent>,
    weather: Option<Res<crate::weather_state::WeatherState>>,
    scene: Res<crate::track::TrackScene>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    mut state: ResMut<MoonState>,
    camera: Query<
        &Transform,
        (
            (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
            Without<MoonDisk>,
            Without<MoonLight>,
        ),
    >,
    mut disk: Query<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<MoonMaterial>,
        ),
        (With<MoonDisk>, Without<MoonLight>, Without<Camera3d>),
    >,
    mut light: Query<
        (&mut Transform, &mut DirectionalLight),
        (With<MoonLight>, Without<MoonDisk>, Without<Camera3d>),
    >,
    mut materials: ResMut<Assets<MoonMaterial>>,
) {
    let (Some(sun), Some(live), Ok(camera)) = (sun, live, camera.single()) else {
        return;
    };
    let utc = env
        .as_ref()
        .and_then(|e| e.solar_utc(content.environment))
        .or_else(|| {
            manual_utc(
                &settings.sky_date,
                live.clock_time_s(),
                env.as_ref().and_then(|e| e.timezone),
                sun.position.longitude,
            )
        });
    let Some(utc) = utc else {
        return;
    };
    let lunar = position(utc.timestamp(), sun.position);
    let clear = weather
        .as_ref()
        .map_or(1., |w| (1. - w.atmosphere.overcast).powi(2));
    let night = ((-sun.direction.y + 0.02) / 0.18).clamp(0., 1.);
    let opacity = clear * night;
    let radius = crate::sky::sky_dome_radius(&scene, &mode) * 0.97;
    for (mut transform, mut visibility, material) in &mut disk {
        *transform = Transform::from_translation(camera.translation + lunar.direction * radius)
            .looking_at(camera.translation, Vec3::Y)
            .with_scale(Vec3::splat(radius * 0.0045));
        // Rectangle's front normal is +Z; looking_at faces -Z toward the camera.
        transform.rotate_y(std::f32::consts::PI);
        *visibility = if opacity > 0.001 && lunar.direction.y > 0. && lunar.illuminated > 0.001 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        let light_local = transform.rotation.inverse() * lunar.sun_direction;
        if let Some(mut m) = materials.get_mut(&material.0) {
            m.params.light = light_local.extend(opacity);
        }
    }
    let lux = 0.25 * lunar.illuminated * lunar.direction.y.max(0.) * opacity;
    for (mut pose, mut light) in &mut light {
        *pose = Transform::IDENTITY.looking_to(-lunar.direction, Vec3::Y);
        light.illuminance = lux;
    }
    state.illuminated = lunar.illuminated;
    state.altitude_deg = lunar.direction.y.asin().to_degrees();
    state.date = utc.format("%Y-%m-%d %H:%M UTC").to_string();
    state.lux = lux;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nasa_new_and_full_moons_and_geographic_position() {
        let geo = GeographicPosition {
            latitude: 51.5_f64.to_radians(),
            longitude: 0.,
        };
        // NASA phase tables: 2000-01-06 18:14 new; 2000-01-21 04:40 full.
        let new = "2000-01-06T18:14:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        let full = "2000-01-21T04:40:00Z"
            .parse::<chrono::DateTime<Utc>>()
            .unwrap();
        assert!(position(new.timestamp(), geo).illuminated < 0.015);
        assert!(position(full.timestamp(), geo).illuminated > 0.985);
        let london = position(full.timestamp(), geo);
        let argentina = position(
            full.timestamp(),
            GeographicPosition {
                latitude: -34.6_f64.to_radians(),
                longitude: -58.4_f64.to_radians(),
            },
        );
        assert!(london.direction.distance(argentina.direction) > 0.5);
        assert!((london.direction.length() - 1.).abs() < 1e-5);
        assert_eq!(manual_utc("2026-10-11", 3600., None, 0.).unwrap().hour(), 1);
    }
    use chrono::Timelike;
}

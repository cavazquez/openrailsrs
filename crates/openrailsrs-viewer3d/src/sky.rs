//! Procedural sky dome and atmospheric distance fog (#8 / #39 / #123).

use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use openrailsrs_bevy_scenery::{
    SkyDome, distance_fog, sky_clear_color as shared_sky_clear_color, sky_palette,
};

use crate::player_launch::{ActivePlayerContent, PlayerWeather};
use crate::route_lighting::RouteSunState;
use crate::track::TrackScene;
use crate::viewer_log;
use crate::world::RouteFocus;

/// Atmospheric fog on the playable camera (#39). Enabled by default; toggle with `F`.
#[derive(Resource, Clone, Debug)]
pub struct FogState {
    pub enabled: bool,
}

impl Default for FogState {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl FogState {
    pub fn hud_label(&self) -> &'static str {
        if self.enabled { "on" } else { "off" }
    }
}

/// Spawn an inverted sky sphere centred on the route.
pub fn spawn_sky_dome(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<RailwaySkyMaterial>>,
    scene: Res<TrackScene>,
    mode: Res<crate::launch::ViewerSceneryMode>,
    _focus: Res<RouteFocus>,
) {
    // Tile-lab puede tener grafo vacío (bbox 0 → radio mínimo 500 m), pero la
    // cámara orbita a ~2.6 km: el domo debe envolverla siempre.
    let radius = sky_dome_radius(&scene, &mode);
    commands.spawn((
        SkyDome,
        Mesh3d(meshes.add(Sphere::new(radius))),
        MeshMaterial3d(materials.add(RailwaySkyMaterial {
            params: sky_parameters(1.0, PlayerWeather::Clear, Vec3::Y, 0.0),
        })),
        Transform::IDENTITY,
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
        Name::new("railway-sky"),
    ));
}

pub fn sky_dome_radius(scene: &TrackScene, _mode: &crate::launch::ViewerSceneryMode) -> f32 {
    (scene.bounds.orbit_distance() * 3.0).clamp(20_000.0, 150_000.0)
}

/// Horizon tint used as the window clear colour.
pub fn sky_clear_color() -> Color {
    shared_sky_clear_color(false)
}

/// Atmospheric fog keyed to viewing distance (parity with `render3d::scene_distance_fog`).
pub fn viewer_distance_fog(visibility_m: f32, night: bool) -> DistanceFog {
    distance_fog(visibility_m, night)
}

/// OR 1.6.1 WeatherControl.SetInitialWeatherParameters: clear weather is 20 km.
/// A smaller scenery loading radius must not turn a clear day into dense fog.
pub const CLEAR_WEATHER_VISIBILITY_M: f32 = 20_000.0;

/// Weather visibility for the playable camera, independent of the loading budget.
pub fn camera_distance_fog() -> DistanceFog {
    let visibility = std::env::var("OPENRAILSRS_FOG_VISIBILITY_M")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(CLEAR_WEATHER_VISIBILITY_M);
    viewer_distance_fog(visibility, false)
}

/// Keep [`DistanceFog`] on the camera with zero density.
///
/// Bevy's mesh view bind group layout includes fog binding 13 only when the
/// component is present. Removing it while pipelines still carry
/// `MeshPipelineKey::DISTANCE_FOG` triggers a wgpu validation crash on toggle.
pub fn disabled_distance_fog() -> DistanceFog {
    DistanceFog {
        color: Color::srgba(0.0, 0.0, 0.0, 0.0),
        directional_light_color: Color::NONE,
        directional_light_exponent: 1.0,
        falloff: FogFalloff::Exponential { density: 0.0 },
    }
}

/// Apply [`FogState::enabled`] without adding/removing the fog component.
pub fn sync_camera_fog(fog: &mut DistanceFog, enabled: bool) {
    *fog = if enabled {
        camera_distance_fog()
    } else {
        disabled_distance_fog()
    };
}

#[derive(Clone, Copy, Debug, bevy::render::render_resource::ShaderType)]
pub struct SkyParameters {
    pub horizon: Vec4,
    pub zenith: Vec4,
    pub sun: Vec4,
    pub clouds: Vec4,
}
#[derive(Asset, TypePath, bevy::render::render_resource::AsBindGroup, Debug, Clone)]
pub struct RailwaySkyMaterial {
    #[uniform(0)]
    pub params: SkyParameters,
}
impl bevy::pbr::Material for RailwaySkyMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/railway_sky.wgsl".into()
    }
    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        _descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        _descriptor.primitive.cull_mode = None;
        Ok(())
    }
}
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
pub fn sky_parameters(
    sun_y: f32,
    weather: PlayerWeather,
    sun: Vec3,
    seconds: f64,
) -> SkyParameters {
    let day = smooth(-0.16, 0.12, sun_y);
    let (dh, dz) = sky_palette(false);
    let (nh, nz) = sky_palette(true);
    let linear = |c: Color| Vec4::from_array(LinearRgba::from(c).to_f32_array());
    let twilight = (1.0 - smooth(0.0, 0.14, sun_y.abs())) * smooth(-0.16, -0.04, sun_y);
    let mut horizon = linear(nh).lerp(linear(dh), day);
    horizon = horizon.lerp(linear(Color::srgb(0.72, 0.39, 0.24)), twilight * 0.42);
    let mut zenith = linear(nz).lerp(linear(dz), day);
    let (coverage, overcast) = match weather {
        PlayerWeather::Clear => (0.22 * day, 0.0),
        PlayerWeather::Rain => (0.88, 0.7),
        PlayerWeather::Overcast => (0.78, 0.5),
        PlayerWeather::Storm => (0.99, 0.92),
        PlayerWeather::Fog => (0.96, 0.85),
        PlayerWeather::Snow => (0.6, 0.65),
    };
    let grey =
        linear(Color::srgb(0.045, 0.055, 0.075)).lerp(linear(Color::srgb(0.48, 0.54, 0.59)), day);
    horizon = horizon.lerp(grey, overcast);
    zenith = zenith.lerp(grey * 0.8, overcast);
    SkyParameters {
        horizon,
        zenith,
        sun: sun.extend(day),
        clouds: Vec4::new(
            coverage,
            (seconds % 86400.0) as f32 * 0.000025,
            twilight,
            overcast,
        ),
    }
}

/// Continuous solar-height palettes; the same horizon colour drives distance
/// fog so sunrise does not produce an abrupt blue/black seam.
#[allow(clippy::too_many_arguments)]
pub fn sync_route_atmosphere(
    sun: Option<Res<RouteSunState>>,
    live: Option<Res<crate::live::LiveDrive>>,
    content: Res<ActivePlayerContent>,
    environment: Option<Res<crate::environment::LiveEnvironment>>,
    storm: Option<Res<crate::storm::StormState>>,
    fog_state: Res<FogState>,
    mut clear: ResMut<ClearColor>,
    domes: Query<&MeshMaterial3d<RailwaySkyMaterial>, With<SkyDome>>,
    mut materials: ResMut<Assets<RailwaySkyMaterial>>,
    mut cameras: Query<(&mut DistanceFog, Option<&bevy::light::VolumetricFog>), With<Camera3d>>,
) {
    let direction = sun.as_ref().map_or(Vec3::Y, |s| s.direction);
    let clock = live.as_ref().map_or(0.0, |l| l.clock_time_s());
    let mut params = sky_parameters(direction.y, content.weather, direction, clock);
    if let Some(environment) = environment.as_ref()
        && let Some(sample) = environment.current_sample(content.environment)
    {
        params.clouds.x = sample.cloud_cover / 100.0;
    }
    let flash = storm.as_ref().map_or(0.0, |s| s.flash);
    params.horizon += Vec4::new(0.48, 0.56, 0.72, 0.0) * flash;
    params.zenith += Vec4::new(0.35, 0.43, 0.65, 0.0) * flash;
    let horizon = Color::linear_rgba(params.horizon.x, params.horizon.y, params.horizon.z, 1.0);
    clear.0 = horizon;
    for dome in &domes {
        if let Some(mut material) = materials.get_mut(&dome.0) {
            material.params = params;
        }
    }
    let visibility = crate::ground_fog::weather_visibility(content.weather);
    for (mut fog, volumetric) in &mut cameras {
        if !fog_state.enabled {
            *fog = disabled_distance_fog();
            continue;
        }
        // Local extinction supplies the dense near field. Retain a milder far
        // haze rather than charging scene geometry the same fog twice.
        let visibility = if content.weather == PlayerWeather::Fog && volumetric.is_some() {
            visibility * 4.0
        } else {
            visibility
        };
        let day = params.sun.w;
        let mut f = viewer_distance_fog(visibility, false);
        // One atmospheric model throughout twilight; reduce forward sun glare
        // continuously rather than changing the shader's model at y=0.
        f.color = horizon.with_alpha(0.94);
        f.directional_light_color = Color::srgba(1.0, 0.95, 0.86, 0.28 * day);
        f.falloff = FogFalloff::from_visibility_colors(
            visibility * (0.55 + 0.45 * day),
            Color::srgba(0.62, 0.70, 0.80, 0.88),
            horizon.with_alpha(0.96),
        );
        *fog = f;
    }
}

/// Toggle fog with `F` — zeros falloff instead of removing [`DistanceFog`].
pub fn toggle_distance_fog(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<FogState>,
    mut cameras: Query<(Entity, Option<&mut DistanceFog>), With<Camera3d>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::KeyF) {
        return;
    }
    // F1/F2 are camera modes; plain F is fog. Ignore if a function-key chord is held.
    if keys.pressed(KeyCode::F1) || keys.pressed(KeyCode::F2) {
        return;
    }
    state.enabled = !state.enabled;
    for (entity, fog) in &mut cameras {
        match fog {
            Some(mut fog) => sync_camera_fog(&mut fog, state.enabled),
            None => {
                // Camera missing the component (e.g. after hot-reload) — always insert
                // so the DISTANCE_FOG view layout stays stable across toggles.
                let mut fog = camera_distance_fog();
                sync_camera_fog(&mut fog, state.enabled);
                commands.entity(entity).insert(fog);
            }
        }
    }
    viewer_log!(
        "openrailsrs-viewer3d: fog {}",
        if state.enabled { "on" } else { "off" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::pbr::FogFalloff;
    use openrailsrs_bevy_scenery::sky_palette;
    use openrailsrs_track::TrackGraph;

    use crate::track::TrackScene;

    #[test]
    fn sunrise_is_continuous_and_overcast_changes_the_palette() {
        let before = sky_parameters(-0.00001, PlayerWeather::Clear, Vec3::X, 0.0);
        let after = sky_parameters(0.00001, PlayerWeather::Clear, Vec3::X, 0.0);
        assert!(before.horizon.distance(after.horizon) < 0.0002);
        assert!(before.zenith.distance(after.zenith) < 0.0002);
        let clear = sky_parameters(0.7, PlayerWeather::Clear, Vec3::Y, 0.0);
        let rain = sky_parameters(0.7, PlayerWeather::Rain, Vec3::Y, 0.0);
        assert!(rain.clouds.x > clear.clouds.x);
        assert!(rain.zenith.length() < clear.zenith.length());
    }

    #[test]
    fn clear_color_is_light_blue() {
        let c = sky_clear_color();
        assert!(c.to_srgba().blue > 0.9);
    }

    #[test]
    fn sky_radius_scales_with_route() {
        let scene = TrackScene::from_graph(TrackGraph::new());
        let radius = (scene.bounds.orbit_distance() * 3.0).clamp(500.0, 150_000.0);
        assert!(radius >= 500.0);
    }

    #[test]
    fn day_fog_uses_atmospheric_falloff() {
        let fog = viewer_distance_fog(2000.0, false);
        assert!(matches!(fog.falloff, FogFalloff::Atmospheric { .. }));
    }

    #[test]
    fn night_fog_uses_non_atmospheric_falloff() {
        let fog = viewer_distance_fog(2000.0, true);
        assert!(
            !matches!(fog.falloff, FogFalloff::Atmospheric { .. }),
            "night fog should use contrast/exponential path"
        );
    }

    #[test]
    fn fog_visibility_scales_with_viewing_distance() {
        let near = viewer_distance_fog(500.0, false);
        let far = viewer_distance_fog(4000.0, false);
        let dens = |f: &DistanceFog| match &f.falloff {
            FogFalloff::Atmospheric { extinction, .. } => extinction.x,
            other => panic!("expected atmospheric fog, got {other:?}"),
        };
        // Longer visibility → lower extinction density.
        assert!(dens(&far) < dens(&near));
    }

    #[test]
    fn shared_palette_matches_clear_color() {
        let (horizon, _) = sky_palette(false);
        assert_eq!(horizon.to_srgba().blue, sky_clear_color().to_srgba().blue);
    }

    #[test]
    fn fog_enabled_by_default() {
        assert!(FogState::default().enabled);
    }

    #[test]
    fn scenery_loading_budget_does_not_change_clear_weather() {
        let previous = std::env::var_os("OPENRAILSRS_VIEW_RADIUS_M");
        let dens = |f: &DistanceFog| match &f.falloff {
            FogFalloff::Atmospheric { extinction, .. } => extinction.x,
            other => panic!("expected atmospheric fog, got {other:?}"),
        };
        unsafe {
            std::env::set_var("OPENRAILSRS_VIEW_RADIUS_M", "450");
        }
        let near = dens(&camera_distance_fog());
        unsafe {
            std::env::set_var("OPENRAILSRS_VIEW_RADIUS_M", "4000");
        }
        let far = dens(&camera_distance_fog());
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("OPENRAILSRS_VIEW_RADIUS_M", value);
            } else {
                std::env::remove_var("OPENRAILSRS_VIEW_RADIUS_M");
            }
        }
        assert_eq!(near, far, "lowering RAM use must preserve the weather");
        assert!(near < dens(&viewer_distance_fog(450.0, false)) / 10.0);
    }

    #[test]
    fn toggle_off_keeps_distance_fog_component_with_zero_density() {
        let mut fog = camera_distance_fog();
        sync_camera_fog(&mut fog, false);
        match fog.falloff {
            FogFalloff::Exponential { density } => assert_eq!(density, 0.0),
            other => panic!("disabled fog must stay Exponential(0), got {other:?}"),
        }
        sync_camera_fog(&mut fog, true);
        assert!(
            matches!(fog.falloff, FogFalloff::Atmospheric { .. }),
            "re-enable must restore atmospheric camera fog"
        );
    }
}

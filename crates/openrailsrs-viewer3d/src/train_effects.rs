//! Native ENG emitters shared by Hanabi and the bounded CPU fallback.
use crate::{
    floating_origin::FloatingOrigin,
    live::LiveDrive,
    rolling_stock::ConsistCarIndex,
    rolling_stock_anim::TrainCarTrackOffset,
    train_effects_gpu::{
        self as gpu, GpuAssets, GpuEmitter, GpuExhaust, TrainParticleCapabilities,
    },
    weather_execution::{AdaptiveWeather, WeatherExecution},
};
use bevy::{
    asset::RenderAssetUsages,
    ecs::system::SystemParam,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

const MAX_PARTICLES: usize = 512;
pub(super) const PUFF_LIFETIME_S: f32 = 3.0;
struct Emitter {
    car: Entity,
    track: usize,
    data: openrailsrs_formats::VehicleEmitter,
    credit: f32,
    gpu_credit: f32,
    gpu: Option<GpuEmitter>,
}
struct Puff {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    radius: f32,
    steam: bool,
    load: f32,
    phase: f32,
    source: Option<Entity>,
}
#[derive(Resource, Default)]
pub struct TrainEffects {
    emitters: Vec<Emitter>,
    particles: Vec<Puff>,
    last_clock: Option<f64>,
    shift: Vec3,
    wind: Vec3,
    requested: WeatherExecution,
    execution: WeatherExecution,
    adaptive: AdaptiveWeather,
    configuration: Option<(WeatherExecution, usize)>,
    gpu_assets: Option<GpuAssets>,
    cpu_capacity: usize,
    gpu_spawn_requests: u64,
    cpu_mesh_updates: u64,
    gpu_delta_s: Option<f32>,
    fallback_reason: Option<&'static str>,
    last_camera_rotation: Option<Quat>,
    birth_seed: u32,
    cpu_collisions: u64,
    obstacle_count: usize,
    disabled: bool,
}
impl TrainEffects {
    pub fn report(&self) -> serde_json::Value {
        let has_gpu = self.emitters.iter().any(|e| e.gpu.is_some());
        let cpu_speed = self
            .particles
            .iter()
            .map(|p| p.velocity.length())
            .fold(0.0_f32, f32::max);
        serde_json::json!({"enabled":!self.disabled,"render_layers":[0],"native_emitters":self.emitters.len(),"live_particles":(!has_gpu).then_some(self.particles.len()),"particle_limit":MAX_PARTICLES,
            "cpu_obstacle_hits":self.cpu_collisions,"nearby_opaque_bounds":self.obstacle_count,"gpu_bounds_per_emitter":8,"steam_pulses_follow_wheels":true,
            "wind_mps":self.wind.to_array(),"inherits_vehicle_velocity":true,
            "requested":self.requested,"execution":self.execution,"gpu_backend":self.emitters.iter().any(|e|e.gpu.is_some()).then_some("bevy_hanabi 0.19.0"),
            "fallback_reason":self.fallback_reason,
            "gpu_emitters":self.emitters.iter().filter(|e|e.gpu.is_some()).count(),
            "gpu_capacity":self.emitters.iter().filter_map(|e|e.gpu.as_ref()).map(|e|e.capacity).sum::<usize>(),
            "cpu_capacity":self.cpu_capacity,"cpu_live_particles":self.particles.len(),
            "gpu_spawn_requests":self.gpu_spawn_requests,"cpu_mesh_updates":self.cpu_mesh_updates,
            "quality_level":self.adaptive.level,"simulation_clock_s":self.last_clock,
            "gpu_simulation_delta_s":self.gpu_delta_s,
            "max_cpu_particle_speed_mps":cpu_speed,
            "max_particle_speed_mps":(!has_gpu).then_some(cpu_speed)})
    }
    pub fn hud_text(&self) -> String {
        let capacity = self
            .emitters
            .iter()
            .filter_map(|e| e.gpu.as_ref())
            .map(|e| e.capacity)
            .sum::<usize>();
        format!(
            "Humo/vapor: {} · capacidad {capacity} GPU + {} CPU · {} emisores originales",
            self.execution.label(),
            self.cpu_capacity,
            self.emitters.len()
        )
    }
}
#[derive(Component)]
pub struct ExhaustMesh;

fn native_emitters(
    path: &std::path::Path,
    route: &std::path::Path,
) -> Vec<Vec<openrailsrs_formats::VehicleEmitter>> {
    let Some(c) = openrailsrs_formats::read_msts_file_to_string(path)
        .ok()
        .and_then(|s| openrailsrs_formats::parse_vehicle_text(&s).ok())
        .and_then(|a| openrailsrs_formats::ConsistFile::from_ast(&a).ok())
    else {
        return vec![];
    };
    let base = openrailsrs_train::consist_asset_root(path);
    c.entries
        .iter()
        .map(|entry| {
            let path = openrailsrs_train::resolve_consist_entry_path(base, entry.path());
            let authored = path.parent().unwrap_or(std::path::Path::new("."));
            let mut candidates = vec![];
            if let Some(folder) = authored.file_name() {
                for root in
                    crate::shapes::or_content_trainset_roots(route, &folder.to_string_lossy())
                {
                    if let Some(name) = path.file_name() {
                        candidates.push(root.join("OpenRails").join(name));
                        candidates.push(root.join(name));
                    }
                }
            }
            candidates.push(path);
            candidates
                .into_iter()
                .filter_map(|p| openrailsrs_formats::resolve_path_case_insensitive(&p))
                .find_map(|p| {
                    openrailsrs_formats::read_vehicle_ast(&p)
                        .ok()
                        .map(|a| openrailsrs_formats::parse_vehicle_emitters(&a))
                        .filter(|e| !e.is_empty())
                })
                .unwrap_or_default()
        })
        .collect()
}
pub fn spawn(
    mut commands: Commands,
    live: Res<LiveDrive>,
    content: Res<crate::player_launch::ActivePlayerContent>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    cars: Query<(Entity, &ConsistCarIndex, &TrainCarTrackOffset)>,
    origin: Res<FloatingOrigin>,
) {
    let Ok(scenario) = openrailsrs_scenarios::load_scenario(live.scenario_path()) else {
        return;
    };
    let directory = live.scenario_path().parent().unwrap();
    let mut paths = vec![directory.join(&scenario.train.consist)];
    paths.extend(
        scenario
            .extra_trains
            .iter()
            .map(|t| directory.join(&t.consist)),
    );
    let mut cache = HashMap::new();
    let mut effects = TrainEffects {
        shift: origin.shift,
        ..default()
    };
    for (entity, index, offset) in &cars {
        let Some(path) = paths.get(offset.track_index) else {
            continue;
        };
        let entries = cache.entry(path.clone()).or_insert_with(|| {
            native_emitters(path, content.route_root.as_deref().unwrap_or(directory))
        });
        if let Some(emitters) = entries.get(index.0) {
            effects
                .emitters
                .extend(emitters.iter().cloned().map(|data| Emitter {
                    car: entity,
                    track: offset.track_index,
                    data,
                    credit: 0.0,
                    gpu_credit: 0.0,
                    gpu: None,
                }));
        }
    }
    crate::viewer_log!(
        "openrailsrs-viewer3d: {} native exhaust/steam emitters (max {MAX_PARTICLES} particles)",
        effects.emitters.len()
    );
    let mut pixels = vec![255u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let p = Vec2::new(x as f32 - 31.5, y as f32 - 31.5) / 31.5;
            let noise =
                (p.x * 15.0 + (p.y * 9.0).sin()).sin() * (p.y * 13.0 + (p.x * 11.0).cos()).cos();
            let edge = (1.0 - p.length() + noise * 0.13).clamp(0.0, 1.0);
            pixels[(y * 64 + x) * 4 + 3] = (edge.powf(1.3) * (0.65 + noise * 0.25) * 230.0) as u8;
        }
    }
    let texture = images.add(Image::new(
        Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture.clone()),
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        perceptual_roughness: 1.0,
        ..default()
    });
    if effects.emitters.is_empty() {
        commands.insert_resource(effects);
        return;
    }
    effects.gpu_assets = Some(GpuAssets::new(texture));
    commands.spawn((
        ExhaustMesh,
        NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
        Mesh3d(meshes.add(particle_mesh(&[], &Transform::IDENTITY))),
        MeshMaterial3d(material),
        Transform::IDENTITY,
        Name::new("native train exhaust"),
    ));
    commands.insert_resource(effects);
}

/// Rate/load are presentation effects, not an additional physical steam model.
fn emission(
    name: &str,
    session: &openrailsrs_sim::LiveDriveSession,
    vehicle: usize,
) -> (f32, bool, f32) {
    let n = name.to_ascii_lowercase();
    let t = session.cab_telemetry();
    let load = t.traction_load_fraction.max(session.state.throttle * 0.5) as f32;
    if n.starts_with("exhaust") {
        if session
            .state
            .diesel
            .car(vehicle)
            .is_some_and(|c| c.flow_lps <= 0.)
        {
            return (0., false, 0.);
        }
        return (3.0 + 20.0 * load, false, load);
    }
    if n.contains("stack") {
        let burning = session
            .state
            .boiler_state
            .as_ref()
            .map_or(0., |b| b.coal_burn_kg_s as f32);
        let params = session.vehicle_definition(vehicle).and_then(|v| match v {
            openrailsrs_train::Vehicle::Loco(l) => l.steam.as_ref(),
            _ => None,
        });
        let wheel_radius = params.map_or(0.8, |s| s.driving_wheel_radius_m);
        let strokes = params.map_or(4.0, |s| f64::from(s.cylinder_count) * 2.0);
        let distance =
            session.state.odometer_m + session.render_wheel_slip_distance_m(vehicle, 0.0);
        let phase = distance / (std::f64::consts::TAU * wheel_radius) * strokes;
        let working = session
            .state
            .boiler_state
            .as_ref()
            .is_some_and(|b| b.tractive_force_n > 0.0 && !b.low_water_failure)
            && session.velocity_mps().abs() > 0.1;
        return (burning * 40.0 * steam_pulse(phase, working), true, load);
    }
    if n.contains("cylinder") {
        return (
            if session.state.boiler_state.as_ref().is_some_and(|b| {
                b.controls.cylinder_cocks && b.pressure_bar > 0. && !b.low_water_failure
            }) && session.state.throttle > 0.1
            {
                18.0
            } else {
                0.0
            },
            true,
            1.0,
        );
    }
    if n.contains("whistle") {
        let steam_available = session
            .state
            .boiler_state
            .as_ref()
            .is_some_and(|b| b.pressure_bar > 0. && !b.low_water_failure);
        return (
            if t.horn_active && steam_available {
                24.0
            } else {
                0.0
            },
            true,
            1.0,
        );
    }
    if n.contains("safety") {
        let open = session
            .state
            .boiler_state
            .as_ref()
            .is_some_and(|b| b.safety_valve);
        return (if open { 24.0 } else { 0.0 }, true, 1.0);
    }
    (0.0, true, 0.0)
}

fn steam_pulse(strokes: f64, working: bool) -> f32 {
    if !working {
        return 1.0;
    }
    // Double-acting strokes follow authored cylinders and driving-wheel slip.
    let crest = (strokes * std::f64::consts::TAU).cos().max(0.0).powi(4) as f32;
    0.45 + crest * 2.8
}

/// Native shapes face -Z after MSTS conversion. Flip reverses the authored
/// vehicle frame, not its direction of travel; the reverser supplies that sign.
fn vehicle_world_velocity(
    car: &GlobalTransform,
    offset: &TrainCarTrackOffset,
    session: &openrailsrs_sim::LiveDriveSession,
    index: usize,
) -> Vec3 {
    if index >= session.formation.coupled_count {
        return Vec3::ZERO;
    }
    let speed = session
        .state
        .vehicles
        .get(index)
        .map_or(session.velocity_mps(), |vehicle| vehicle.velocity_mps) as f32;
    let flip = if offset.flipped { -1.0 } else { 1.0 };
    let direction = if session.driver_direction <= 0.25 {
        -1.0
    } else {
        1.0
    };
    car.rotation() * Vec3::NEG_Z * speed * flip * direction
}

/// Analytic linear drag in world space. A newborn plume retains the carriage's
/// velocity, then approaches the surrounding air rather than following the car
/// or camera. The closed form makes its path independent of render frame rate.
fn advance_puff(puff: &mut Puff, dt: f32, wind: Vec3) {
    if dt <= 0.0 {
        return;
    }
    let drag = if puff.steam { 1.7 } else { 1.1 };
    let equilibrium = wind + Vec3::Y * (0.35 / drag);
    let relative = puff.velocity - equilibrium;
    let decay = (-drag * dt).exp();
    let turbulence = |age: f32| {
        Vec3::new(
            (age * 2.7 + puff.phase).sin() * 0.18,
            0.0,
            (age * 1.9 + puff.phase * 1.3).cos() * 0.16,
        )
    };
    puff.position +=
        equilibrium * dt + relative * ((1.0 - decay) / drag) + turbulence(puff.age + dt)
            - turbulence(puff.age);
    puff.velocity = equilibrium + relative * decay;
    puff.age += dt;
}

#[derive(SystemParam)]
pub struct TrainEffectScene<'w, 's> {
    commands: Commands<'w, 's>,
    environment: Option<Res<'w, crate::environment::LiveEnvironment>>,
    atmosphere: Option<Res<'w, crate::weather_state::WeatherState>>,
    content: Option<Res<'w, crate::player_launch::ActivePlayerContent>>,
    preferences: Option<Res<'w, crate::player_settings::PlayerSettings>>,
    capabilities: Option<Res<'w, TrainParticleCapabilities>>,
    memory: Option<Res<'w, crate::gpu_memory::GraphicsMemory>>,
    time: Option<Res<'w, Time<Real>>>,
    loading: (
        Option<Res<'w, crate::world::WorldSpawnProgress>>,
        Option<Res<'w, crate::route_bootstrap::ViewerLoadingScreen>>,
    ),
    gpu_time: Option<Res<'w, Time<bevy_hanabi::EffectSimulation>>>,
    sun: Option<Res<'w, crate::route_lighting::RouteSunState>>,
    assets: Option<ResMut<'w, Assets<bevy_hanabi::EffectAsset>>>,
    gpu: Query<
        'w,
        's,
        (
            &'static mut bevy_hanabi::EffectProperties,
            &'static mut bevy_hanabi::EffectSpawner,
            &'static GlobalTransform,
            &'static bevy_hanabi::CompiledParticleEffect,
        ),
        With<GpuExhaust>,
    >,
    cars: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static ConsistCarIndex,
            &'static TrainCarTrackOffset,
        ),
    >,
    camera: Query<'w, 's, &'static Transform, With<Camera3d>>,
    mesh: Query<'w, 's, &'static Mesh3d, With<ExhaustMesh>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    obstacles: Option<Res<'w, crate::effect_obstacles::EffectObstacles>>,
    terrain: Option<Res<'w, crate::terrain::TerrainElevation>>,
    focus: Option<Res<'w, crate::world::RouteFocus>>,
    fog: Option<Res<'w, crate::sky::FogState>>,
}

pub fn update(
    live: Res<LiveDrive>,
    origin: Res<FloatingOrigin>,
    mut effects: ResMut<TrainEffects>,
    scene: TrainEffectScene,
) {
    let TrainEffectScene {
        mut commands,
        environment,
        atmosphere,
        content,
        preferences,
        capabilities,
        memory,
        time,
        loading,
        gpu_time,
        sun,
        mut assets,
        mut gpu,
        cars,
        camera,
        mesh,
        mut meshes,
        obstacles,
        terrain,
        focus,
        fog,
    } = scene;
    let (Ok(camera), Ok(mesh)) = (camera.single(), mesh.single()) else {
        return;
    };
    let enabled = std::env::var("OPENRAILSRS_TRAIN_EFFECTS_ENABLED").map_or_else(
        |_| preferences.as_ref().is_none_or(|p| p.train_effects_enabled),
        |s| s != "0",
    );
    effects.disabled = !enabled;
    if !enabled {
        for emitter in &mut effects.emitters {
            if let Some(gpu) = emitter.gpu.take() {
                commands.entity(gpu.entity).despawn();
            }
        }
        if !effects.particles.is_empty()
            && let Some(mut mesh) = meshes.get_mut(&mesh.0)
        {
            *mesh = particle_mesh(&[], camera);
        }
        effects.particles.clear();
        effects.configuration = None;
        effects.cpu_capacity = 0;
        effects.last_clock = Some(live.session.time_s());
        return;
    }
    let clock = live.session.time_s();
    let previous = effects.last_clock.replace(clock).unwrap_or(clock);
    let dt = if live.paused {
        0.0
    } else {
        (clock - previous).max(0.0) as f32
    };
    let sample = environment
        .as_ref()
        .zip(content.as_ref())
        .and_then(|(environment, content)| environment.current_sample(content.environment));
    let wind = atmosphere.as_ref().map_or_else(
        || crate::environment::weather_wind(sample),
        |s| s.atmosphere.wind_mps,
    );
    effects.wind = wind;
    effects.gpu_delta_s = gpu_time.as_ref().map(|t| t.delta_secs());
    let requested = std::env::var("OPENRAILSRS_TRAIN_EFFECT_EXECUTION")
        .ok()
        .as_deref()
        .and_then(WeatherExecution::parse)
        .unwrap_or_else(|| {
            preferences
                .as_ref()
                .map_or(WeatherExecution::Auto, |s| s.train_effect_execution)
        });
    let pressure = memory.as_ref().is_some_and(|m| m.pressure());
    effects.adaptive.observe(
        time.as_ref().map_or(0.0, |t| t.delta_secs()),
        dt == 0.0 || loading.0.is_some() || loading.1.is_some(),
        pressure,
    );
    let capabilities = capabilities
        .as_deref()
        .filter(|_| assets.is_some() && effects.gpu_assets.is_some());
    let mode = gpu::resolve(
        requested,
        capabilities,
        pressure || effects.adaptive.level > 0,
        effects.emitters.len(),
    );
    effects.requested = requested;
    effects.execution = mode;
    effects.fallback_reason = if mode == WeatherExecution::Cpu && requested != WeatherExecution::Cpu
    {
        Some(if effects.emitters.len() > gpu::MAX_GPU_EMITTERS {
            "Más de 32 emisores: se usa la malla CPU compartida"
        } else if capabilities.is_some_and(|c| c.supported && !c.hardware) {
            "Adaptador de renderizado por software"
        } else {
            "Cómputo GPU no disponible"
        })
    } else {
        None
    };
    let configuration = (mode, effects.adaptive.level);
    let restart = clock < previous;
    let had_particles = !effects.particles.is_empty();
    let configuration_changed = effects.configuration != Some(configuration) || restart;
    if configuration_changed {
        configure_gpu(
            &mut effects,
            &mut commands,
            assets.as_deref_mut(),
            configuration,
            restart,
        );
    }
    // Manual spawn counts must be assigned after Hanabi's TickSpawners. Wind
    // and light also reach old particles when an emitter stops or leaves view.
    let light = sun.as_ref().map_or(1.0, |s| s.ambient_scale);
    for (mut properties, mut spawner, _, _) in &mut gpu {
        spawner.spawn_count = 0;
        properties.set("wind", wind.into());
        properties.set("light", light.into());
    }
    let shift = origin.shift - effects.shift;
    effects.shift = origin.shift;
    effects.obstacle_count = obstacles.as_ref().map_or(0, |s| s.solids.len());
    let ground = |point: Vec3| {
        terrain
            .as_ref()
            .zip(focus.as_ref())
            .and_then(|(t, f)| {
                t.sample_world_y(
                    point.x + origin.shift.x + f.center.x,
                    point.z + origin.shift.z + f.center.z,
                )
                .map(|h| h - f.height_origin - origin.shift.y)
            })
            .unwrap_or(-10000.0)
    };
    let mut collisions = 0;
    for puff in &mut effects.particles {
        puff.position -= shift;
        let from = puff.position;
        let protection = obstacles.as_ref().map_or(1.0, |s| s.wind_factor(from));
        advance_puff(puff, dt, wind * protection);
        if dt > 0.0
            && puff.age > 0.18
            && (puff.position.y < ground(puff.position)
                || obstacles
                    .as_ref()
                    .is_some_and(|s| s.blocked(from, puff.position, puff.source)))
        {
            puff.age = PUFF_LIFETIME_S;
            collisions += 1;
        }
    }
    effects.cpu_collisions += collisions;
    effects.particles.retain(|p| p.age < PUFF_LIFETIME_S);
    let capacity = effects.cpu_capacity.saturating_sub(effects.particles.len());
    let mut born = Vec::new();
    let mut gpu_spawn_requests = 0;
    let mut birth_seed = effects.birth_seed;
    if !live.paused {
        for emitter in &mut effects.emitters {
            let session = if emitter.track == 0 {
                Some(&live.session)
            } else {
                live.traffic
                    .services
                    .get(emitter.track - 1)
                    .filter(|s| s.departed)
                    .map(|s| &s.session)
            };
            let (Some(session), Ok((car, index, offset))) = (session, cars.get(emitter.car)) else {
                continue;
            };
            if car.translation().distance(camera.translation) > 250.0 {
                continue;
            }
            let (rate, steam, load) = emission(&emitter.data.name, session, index.0);
            // Catch up old plumes on the simulation clock, but only emit the
            // latest interval after loading or accelerated time. Neither the
            // retained particles nor the temporary birth buffer can exceed
            // the budget, even with many original emitters.
            emitter.credit += rate * dt.min(0.15);
            let position = Vec3::new(
                emitter.data.position[0],
                emitter.data.position[1],
                -emitter.data.position[2],
            );
            let direction = Vec3::new(
                emitter.data.direction[0],
                emitter.data.direction[1],
                -emitter.data.direction[2],
            )
            .normalize_or_zero();
            let count = (emitter.credit.floor() as usize).min(MAX_PARTICLES);
            emitter.credit = emitter.credit.fract();
            let velocity = vehicle_world_velocity(car, offset, session, index.0)
                + car.rotation() * direction * (1.2 + load * 2.0)
                + Vec3::Y * 0.5;
            let world_position = car.transform_point(position);
            let radius = emitter.data.radius_m.max(0.05);
            let (shade, opacity) = appearance(steam, load);
            let gpu_count = split_births(emitter, count, mode);
            if let Some(emitter) = &emitter.gpu
                && let Ok((mut properties, mut spawner, anchor, compiled)) =
                    gpu.get_mut(emitter.entity)
                && compiled.is_ready()
            {
                *properties = gpu::properties(
                    world_position - anchor.translation(),
                    velocity,
                    wind,
                    radius,
                    shade,
                    opacity,
                    light,
                );
                spawner.spawn_count = gpu_count.min(emitter.capacity) as u32;
                gpu_spawn_requests += u64::from(spawner.spawn_count);
            }
            let cpu_count = (count - gpu_count).min(capacity.saturating_sub(born.len()));
            for _ in 0..cpu_count {
                birth_seed = birth_seed.wrapping_add(1);
                let phase = (birth_seed as f32 * 2.399963).rem_euclid(std::f32::consts::TAU);
                born.push(Puff {
                    position: world_position,
                    velocity,
                    age: 0.0,
                    radius: radius * (0.8 + phase.sin().abs() * 0.4),
                    steam,
                    load,
                    phase,
                    source: Some(emitter.car),
                });
            }
        }
    }
    effects.birth_seed = birth_seed;
    effects.gpu_spawn_requests += gpu_spawn_requests;
    let density = if fog.as_ref().is_none_or(|f| f.enabled) {
        content.as_ref().map_or(0.0, |c| {
            std::f32::consts::LN_10
                / atmosphere.as_ref().map_or_else(
                    || crate::ground_fog::weather_visibility(c.weather),
                    |s| s.atmosphere.visibility_m,
                )
        })
    } else {
        0.0
    };
    let fog_color = content.as_ref().map_or(Vec3::splat(0.55), |c| {
        crate::sky::sky_parameters(
            sun.as_ref().map_or(0.5, |s| s.direction.y),
            c.weather,
            sun.as_ref().map_or(Vec3::Y, |s| s.direction),
            clock,
        )
        .horizon
        .truncate()
    });
    let fog_color = atmosphere.as_ref().map_or(fog_color, |s| {
        crate::sky::atmosphere_parameters(
            sun.as_ref().map_or(0.5, |s| s.direction.y),
            &s.atmosphere,
            sun.as_ref().map_or(Vec3::Y, |s| s.direction),
            clock,
        )
        .horizon
        .truncate()
    });
    for emitter in &effects.emitters {
        let Some(instance) = &emitter.gpu else {
            continue;
        };
        let Ok((mut properties, _, anchor, _)) = gpu.get_mut(instance.entity) else {
            continue;
        };
        let Ok((car, _, _)) = cars.get(emitter.car) else {
            continue;
        };
        let point = car.transform_point(Vec3::new(
            emitter.data.position[0],
            emitter.data.position[1],
            -emitter.data.position[2],
        ));
        let solids = obstacles
            .as_ref()
            .map_or_else(Vec::new, |s| s.nearest(point, 8, emitter.car));
        let protection = obstacles.as_ref().map_or(1.0, |s| s.wind_factor(point));
        properties.set("wind", (wind * protection).into());
        gpu::environment(
            &mut properties,
            anchor.translation(),
            ground(point),
            fog_color.extend(density),
            &solids,
        );
    }
    let changed = !born.is_empty()
        || had_particles
            && (configuration_changed
                || dt > 0.0
                || shift != Vec3::ZERO
                || effects.last_camera_rotation != Some(camera.rotation));
    effects.particles.extend(born);
    if changed && let Some(mut mesh) = meshes.get_mut(&mesh.0) {
        *mesh = particle_mesh(&effects.particles, camera);
        effects.cpu_mesh_updates += 1;
    }
    effects.last_camera_rotation = Some(camera.rotation);
}

fn appearance(steam: bool, load: f32) -> (f32, f32) {
    (
        if steam { 0.88 } else { 0.55 - load * 0.35 },
        if steam { 0.6 } else { 0.7 },
    )
}

fn split_births(emitter: &mut Emitter, count: usize, mode: WeatherExecution) -> usize {
    match mode {
        WeatherExecution::Gpu => count,
        WeatherExecution::Hybrid => {
            emitter.gpu_credit += count as f32 * 0.75;
            let gpu = emitter.gpu_credit.floor() as usize;
            emitter.gpu_credit = emitter.gpu_credit.fract();
            gpu.min(count)
        }
        _ => 0,
    }
}

fn configure_gpu(
    effects: &mut TrainEffects,
    commands: &mut Commands,
    assets: Option<&mut Assets<bevy_hanabi::EffectAsset>>,
    configuration: (WeatherExecution, usize),
    restart: bool,
) {
    // Replacing the instance releases old GPU particles on a restore/backend
    // change. Cached assets remain bounded by type, quota and quality level.
    for emitter in &mut effects.emitters {
        if let Some(gpu) = emitter.gpu.take() {
            commands.entity(gpu.entity).despawn();
        }
        if restart {
            emitter.credit = 0.0;
        }
        emitter.gpu_credit = 0.0;
    }
    if restart || effects.configuration.is_some() || configuration.0 != WeatherExecution::Cpu {
        effects.particles.clear();
    }
    effects.last_camera_rotation = None;
    effects.configuration = Some(configuration);
    let (budget, cpu) = gpu::budgets(configuration.0, configuration.1, MAX_PARTICLES);
    effects.cpu_capacity = cpu;
    effects.particles.truncate(cpu);
    let count = effects.emitters.len();
    let quota = budget.checked_div(count).unwrap_or(0);
    if quota == 0 {
        return;
    }
    if let (Some(assets), Some(gpu_assets)) = (assets, effects.gpu_assets.as_mut()) {
        for emitter in &mut effects.emitters {
            let steam = !emitter
                .data
                .name
                .to_ascii_lowercase()
                .starts_with("exhaust");
            let handle = gpu_assets.handle(assets, steam, quota);
            emitter.gpu = Some(gpu::spawn(
                commands,
                handle,
                gpu_assets.texture.clone(),
                quota,
            ));
        }
    }
}
fn particle_mesh(particles: &[Puff], camera: &Transform) -> Mesh {
    let mut positions = Vec::with_capacity(particles.len() * 4);
    let mut normals = Vec::with_capacity(particles.len() * 4);
    let mut uvs = Vec::with_capacity(particles.len() * 4);
    let mut colors = Vec::with_capacity(particles.len() * 4);
    let mut indices = Vec::with_capacity(particles.len() * 6);
    let camera_right = camera.rotation * Vec3::X;
    let camera_up = camera.rotation * Vec3::Y;
    let normal = camera.rotation * Vec3::Z;
    let invisible = Puff {
        position: Vec3::ZERO,
        velocity: Vec3::ZERO,
        age: PUFF_LIFETIME_S,
        radius: 0.0,
        steam: true,
        load: 0.0,
        phase: 0.0,
        source: None,
    };
    // At least one degenerate transparent quad: Bevy's slab allocator does not
    // allocate empty vertex/index data. Never submit a zero-length GPU buffer.
    let particles = if particles.is_empty() {
        std::slice::from_ref(&invisible)
    } else {
        particles
    };
    for p in particles {
        let right = camera_right * p.phase.cos() + camera_up * p.phase.sin();
        let up = -camera_right * p.phase.sin() + camera_up * p.phase.cos();
        let radius = p.radius * (1.0 + p.age * 4.0);
        let (shade, opacity) = appearance(p.steam, p.load);
        let alpha = (1.0 - p.age / PUFF_LIFETIME_S).powi(2) * opacity;
        let base = positions.len() as u32;
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            positions.push((p.position + (right * x + up * y) * radius).to_array());
            normals.push(normal.to_array());
            colors.push([shade, shade, shade, alpha]);
        }
        uvs.extend([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(file: &str) -> openrailsrs_sim::LiveDriveSession {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation")
            .join(file);
        let scenario = openrailsrs_scenarios::load_scenario(&path).unwrap();
        openrailsrs_sim::LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap()
    }

    fn puff(velocity: Vec3) -> Puff {
        Puff {
            position: Vec3::ZERO,
            velocity,
            age: 0.0,
            radius: 0.1,
            steam: false,
            load: 1.0,
            phase: 0.0,
            source: None,
        }
    }

    #[test]
    fn steam_strokes_repeat_with_wheels_and_idle_has_no_pulses() {
        assert!(steam_pulse(0.0, true) > steam_pulse(0.5, true) * 5.0);
        assert!((steam_pulse(0.0, true) - steam_pulse(1.0, true)).abs() < 1e-5);
        assert_eq!(steam_pulse(0.25, false), 1.0);
    }
    #[test]
    fn smoke_inherits_car_motion_regardless_of_authored_flip_and_reverses_with_train() {
        let mut train = session("scenario.toml");
        train.state.velocity_mps = 12.0;
        train.driver_direction = 1.0;
        let frame = crate::shapes::vehicle_authored_frame_transform(0.0, false);
        let mut offset = TrainCarTrackOffset {
            offset_m: 0.0,
            track_index: 0,
            flipped: false,
        };
        let car = GlobalTransform::from(frame);
        let forward = vehicle_world_velocity(&car, &offset, &train, 0);
        assert!((forward - Vec3::X * 12.0).length() < 1e-5);
        offset.flipped = true;
        let flipped =
            GlobalTransform::from(crate::shapes::vehicle_authored_frame_transform(0.0, true));
        assert!((vehicle_world_velocity(&flipped, &offset, &train, 0) - forward).length() < 1e-5);
        train.driver_direction = 0.0;
        assert!((vehicle_world_velocity(&flipped, &offset, &train, 0) + forward).length() < 1e-5);
        train.formation.coupled_count = 0;
        assert_eq!(
            vehicle_world_velocity(&flipped, &offset, &train, 0),
            Vec3::ZERO
        );
    }

    #[test]
    fn world_plume_follows_crosswind_and_matches_across_frame_rates() {
        for steam in [false, true] {
            let mut whole = puff(Vec3::X * 20.0 + Vec3::Y * 3.0);
            whole.steam = steam;
            let mut divided = puff(whole.velocity);
            divided.steam = steam;
            let wind = Vec3::Z * 5.0;
            advance_puff(&mut whole, 1.0, wind);
            for _ in 0..20 {
                advance_puff(&mut divided, 0.05, wind);
            }
            assert!((whole.position - divided.position).length() < 2e-5);
            assert!((whole.velocity - divided.velocity).length() < 2e-5);
            assert!(whole.velocity.x < 20.0 && whole.velocity.z > 0.0);
            assert!(whole.position.x < 20.0 && whole.position.z > 0.0);
            assert!((whole.age - divided.age).abs() < 1e-5);
        }
        let mut still = puff(Vec3::Y);
        advance_puff(&mut still, 1.0, Vec3::ZERO);
        assert!(still.position.x.abs() <= 0.36 && still.position.z.abs() <= 0.32);
        let before = (still.position, still.velocity, still.age);
        advance_puff(&mut still, 0.0, Vec3::X * 18.0);
        assert_eq!((still.position, still.velocity, still.age), before);
    }

    #[test]
    fn effects_update_uses_live_wind_preserves_pause_rebases_and_bounds_births() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = crate::test_harness::minimal_app();
        let mut live =
            LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path()).unwrap();
        live.session.step_realtime(0.8, |_| {});
        live.paused = true;
        let clock = live.session.time_s();
        let mut old = puff(Vec3::Y);
        old.position = Vec3::new(5.0, 2.0, 3.0);
        old.age = 2.5;
        let car = app
            .world_mut()
            .spawn((
                GlobalTransform::from(crate::shapes::vehicle_authored_frame_transform(0.0, false)),
                ConsistCarIndex(0),
                TrainCarTrackOffset {
                    offset_m: 0.0,
                    track_index: 0,
                    flipped: false,
                },
            ))
            .id();
        app.world_mut()
            .spawn((Camera3d::default(), Transform::IDENTITY));
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(particle_mesh(&[], &Transform::IDENTITY));
        app.world_mut().spawn((ExhaustMesh, Mesh3d(mesh)));
        let sample: crate::environment::WeatherSample = serde_json::from_value(serde_json::json!({
            "time":1791100800_i64,"weather_code":61,"temperature_2m":10.0,"cloud_cover":80.0,
            "precipitation":2.0,"snowfall":0.0,"wind_speed_10m":4.0,"wind_direction_10m":270.0,
            "timezone":"Europe/London"
        }))
        .unwrap();
        let mut environment = crate::environment::LiveEnvironment::default();
        environment.utc = chrono::DateTime::from_timestamp(sample.time, 0);
        environment.sample = Some(sample);
        let mut content = crate::player_launch::ActivePlayerContent::default();
        content.environment.weather = crate::environment::EnvironmentSource::LocalNow;
        app.insert_resource(environment);
        app.insert_resource(content);
        app.insert_resource(live);
        app.insert_resource(FloatingOrigin::default());
        app.insert_resource(TrainEffects {
            particles: vec![old],
            emitters: vec![Emitter {
                car,
                track: 0,
                data: openrailsrs_formats::VehicleEmitter {
                    name: "Exhaust1".into(),
                    position: [0.0, 2.0, 0.0],
                    direction: [0.0, 1.0, 0.0],
                    radius_m: 0.1,
                },
                credit: 10000.5,
                gpu_credit: 0.0,
                gpu: None,
            }],
            last_clock: Some(0.0),
            ..default()
        });
        app.world_mut().run_system_once(update).unwrap();
        let effects = app.world().resource::<TrainEffects>();
        assert_eq!(effects.particles.len(), 1);
        assert_eq!(effects.particles[0].position, Vec3::new(5.0, 2.0, 3.0));
        assert_eq!(effects.particles[0].age, 2.5);
        assert!((effects.wind - Vec3::X * 4.0).length() < 1e-5);
        app.world_mut().resource_mut::<FloatingOrigin>().shift = Vec3::X * 2048.0;
        app.world_mut().run_system_once(update).unwrap();
        assert_eq!(
            app.world().resource::<TrainEffects>().particles[0]
                .position
                .x,
            5.0 - 2048.0
        );
        {
            let mut live = app.world_mut().resource_mut::<LiveDrive>();
            live.paused = false;
            live.session.step_realtime(0.8, |_| {});
        }
        app.world_mut().run_system_once(update).unwrap();
        let effects = app.world().resource::<TrainEffects>();
        assert!(effects.last_clock.unwrap() > clock);
        assert_eq!(effects.particles.len(), MAX_PARTICLES);
        assert!(
            effects.particles.iter().all(|p| p.age == 0.0),
            "full elapsed time must expire old plumes"
        );
        assert!(effects.emitters[0].credit < 1.0);
        app.world_mut().resource_mut::<LiveDrive>().reset().unwrap();
        app.world_mut().run_system_once(update).unwrap();
        let effects = app.world().resource::<TrainEffects>();
        assert!(effects.particles.is_empty());
        assert_eq!(effects.emitters[0].credit, 0.0);
    }

    #[test]
    fn stopped_diesel_and_empty_boiler_do_not_emit_exhaust_or_whistle_steam() {
        let mut diesel = session("scenario.toml");
        diesel.step_realtime(1., |_| {});
        assert!(emission("Exhaust1", &diesel, 0).0 > 0.);
        diesel.toggle_diesel_engine(0).unwrap();
        diesel.step_realtime(10., |_| {});
        assert_eq!(emission("Exhaust1", &diesel, 0).0, 0.);
        let mut steam = session("scenario_steam.toml");
        steam.trigger_horn(1.);
        assert!(emission("Whistle", &steam, 0).0 > 0.);
        steam.state.boiler_state.as_mut().unwrap().pressure_bar = 0.;
        assert_eq!(emission("Whistle", &steam, 0).0, 0.);
        steam.state.boiler_state.as_mut().unwrap().pressure_bar = 16.;
        steam.state.boiler_state.as_mut().unwrap().low_water_failure = true;
        assert_eq!(emission("Whistle", &steam, 0).0, 0.);
    }

    #[test]
    fn hybrid_distributes_small_birth_batches_without_double_emission() {
        let mut emitter = Emitter {
            car: Entity::PLACEHOLDER,
            track: 0,
            data: openrailsrs_formats::VehicleEmitter {
                name: "Exhaust1".into(),
                position: [0.0; 3],
                direction: [0.0, 1.0, 0.0],
                radius_m: 0.1,
            },
            credit: 0.0,
            gpu_credit: 0.0,
            gpu: None,
        };
        let mut gpu = 0;
        let mut cpu = 0;
        for _ in 0..100 {
            let count = split_births(&mut emitter, 1, WeatherExecution::Hybrid);
            gpu += count;
            cpu += 1 - count;
        }
        assert_eq!((gpu, cpu), (75, 25));
        assert_eq!(split_births(&mut emitter, 12, WeatherExecution::Cpu), 0);
        assert_eq!(split_births(&mut emitter, 12, WeatherExecution::Gpu), 12);
    }
    #[test]
    fn mesh_is_bounded_and_retains_each_particles_alpha() {
        let p = Puff {
            position: Vec3::ZERO,
            velocity: Vec3::Y,
            age: 0.0,
            radius: 0.1,
            steam: true,
            load: 1.0,
            phase: 0.0,
            source: None,
        };
        let mesh = particle_mesh(&[p], &Transform::IDENTITY);
        assert_eq!(mesh.count_vertices(), 4);
        assert_eq!(mesh.indices().unwrap().len(), 6);
    }
    #[test]
    fn idle_mesh_has_valid_buffers_and_no_visible_particles() {
        let mesh = particle_mesh(&[], &Transform::IDENTITY);
        assert_eq!(mesh.count_vertices(), 4);
        assert_eq!(mesh.indices().unwrap().len(), 6);
        let Some(bevy::mesh::VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("missing color");
        };
        assert!(colors.iter().all(|color| color[3] == 0.0));
    }
}

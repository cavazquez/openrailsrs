//! Bounded Hanabi/CPU precipitation with the same analytic world-space field.
use crate::{
    precipitation::{PrecipitationState, rain_rng01},
    weather_execution::{AdaptiveWeather, WeatherExecution},
    weather_particles_gpu::{GpuWeather, HanabiWeather},
};
use bevy::ecs::system::SystemParam;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::NoFrustumCulling,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
};

const GPU_CAPACITY: usize = 8192;
const CPU_CAPACITY: usize = 2048;
const GRID: usize = 64;
const HALF: f32 = 55.0;
const HEIGHT: f32 = 40.0;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct ParticleUniforms {
    pub center: Vec4,
    pub phase: Vec4,
    pub right: Vec4,
    pub up: Vec4,
    pub wind_time: Vec4,
    pub grid: Vec4,
    /// Horizontal wind, flake size, fall-speed factor. Shared by CPU and GPU.
    pub motion: Vec4,
}

impl Default for ParticleUniforms {
    fn default() -> Self {
        Self {
            center: Vec4::ZERO,
            phase: Vec4::ZERO,
            right: Vec4::ZERO,
            up: Vec4::ZERO,
            wind_time: Vec4::ZERO,
            grid: Vec4::ZERO,
            motion: Vec4::new(0., 0., 1., 1.),
        }
    }
}
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ParticleExtension {
    #[uniform(100)]
    pub params: ParticleUniforms,
    #[texture(101, filterable = false)]
    pub shelter: Handle<Image>,
}
impl MaterialExtension for ParticleExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/weather_particles.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/weather_particles.wgsl".into()
    }
}
pub type ParticleMaterial = ExtendedMaterial<StandardMaterial, ParticleExtension>;

#[derive(Component)]
pub struct WeatherMesh;
struct Layer {
    mesh: Handle<Mesh>,
    material: Handle<ParticleMaterial>,
    count: usize,
    first_seed: u32,
    gpu: bool,
}
#[derive(Resource, Default)]
pub struct WeatherParticles {
    layers: Vec<Layer>,
    shelter: Handle<Image>,
    shelter_gpu: Handle<Image>,
    shelter_base: f32,
    shelter_values: Vec<f32>,
    shelter_center: Vec2,
    shelter_origin: Vec3,
    shelter_clock: f32,
    adaptive: AdaptiveWeather,
    live_wind: Option<WindDrift>,
    hanabi: GpuWeather,
    gpu_delta_s: Option<f32>,
    pub execution: WeatherExecution,
    pub requested: WeatherExecution,
    pub gpu_particles: usize,
    pub cpu_particles: usize,
    pub shelter_refreshes: u64,
    pub gpu_mesh_updates: u64,
    pub cpu_mesh_updates: u64,
}
/// Changing wind must not multiply a new velocity by the entire elapsed trip.
/// Integrating a smooth velocity keeps drops continuous across provider updates.
struct WindDrift {
    offset: bevy::math::DVec2,
    velocity: Vec2,
    clock: f64,
}
impl WindDrift {
    fn advance(&mut self, clock: f64, target: Vec2) -> Vec2 {
        if clock < self.clock {
            self.offset = target.as_dvec2() * clock;
        }
        let dt = (clock - self.clock).max(0.0);
        self.clock = clock;
        let old = self.velocity;
        self.velocity = old.lerp(target, (1.0 - (-dt / 4.0).exp()) as f32);
        self.offset += (old + self.velocity).as_dvec2() * 0.5 * dt;
        Vec2::new(
            self.offset.x.rem_euclid(f64::from(HALF) * 2.0) as f32,
            self.offset.y.rem_euclid(f64::from(HALF) * 2.0) as f32,
        )
    }
}
impl WeatherParticles {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"execution":self.execution,"requested":self.requested,"gpu_particles":self.gpu_particles,
            "cpu_particles":self.cpu_particles,"quality_level":self.adaptive.level,
            "shelter_refreshes":self.shelter_refreshes,"seed_upload_capacity":self.hanabi.configuration.map_or(0,|(_,n)|n)+CPU_CAPACITY,
            "gpu_mesh_updates":self.gpu_mesh_updates,"cpu_mesh_updates":self.cpu_mesh_updates,
            "gpu_backend":self.hanabi.entity.map(|_|"bevy_hanabi 0.19.0"),
            "gpu_capacity":self.hanabi.configuration.map_or(0,|(_,n)|n),
            "gpu_seed_initializations":self.hanabi.seed_initializations,
            "gpu_simulation_delta_s":self.gpu_delta_s})
    }
    pub fn hud_text(&self) -> String {
        format!(
            "Clima: {} · {} copos/gotas GPU + {} CPU · detalle {}",
            self.execution.label(),
            self.gpu_particles,
            self.cpu_particles,
            ["alto", "medio", "bajo"][self.adaptive.level.min(2)]
        )
    }
}

fn seed_values(seed: u32) -> Vec4 {
    Vec4::new(
        rain_rng01(seed, 0),
        rain_rng01(seed, 1),
        rain_rng01(seed, 2),
        rain_rng01(seed, 3),
    )
}
fn wrap(value: f32, period: f32) -> f32 {
    (value + period * 0.5).rem_euclid(period) - period * 0.5
}

/// Parameters match the WGSL vertex path. The camera only recycles particles at
/// volume boundaries; moving it does not translate the snow field with the eye.
pub fn particle_position(seed: Vec4, p: &ParticleUniforms) -> Vec3 {
    let t = p.wind_time.w;
    let snow = p.up.w > 0.5;
    let fall = if snow {
        0.7 + seed.w * 1.2
    } else {
        20.0 + seed.w * 16.0
    };
    let flutter = if snow {
        Vec2::new(
            (t * 0.73 + seed.x * 31.0).sin(),
            (t * 0.51 + seed.z * 27.0).cos(),
        ) * 0.65
    } else {
        Vec2::ZERO
    };
    let drift = if p.wind_time.y > 0.5 { 1.0 } else { t };
    let x = seed.x * HALF * 2.0 + p.wind_time.x * drift + flutter.x;
    let z = seed.z * HALF * 2.0 + p.wind_time.z * drift + flutter.y;
    let y = seed.y * HEIGHT - fall * t;
    p.center.truncate()
        + Vec3::new(
            wrap(x - p.phase.x, HALF * 2.0),
            wrap(y - p.phase.y, HEIGHT),
            wrap(z - p.phase.z, HALF * 2.0),
        )
}
fn particle_corner(seed: Vec4, uv: Vec2, p: &ParticleUniforms) -> Vec3 {
    if p.up.w <= 0.5 {
        return p.right.truncate() * ((uv.x - 0.5) * 0.025)
            + Vec3::new(p.motion.x / 32.0, -1.0, p.motion.y / 32.0).normalize()
                * ((uv.y - 0.5) * (0.8 + seed.w * 0.65));
    }
    let angle = seed.w * std::f32::consts::TAU + p.wind_time.w * (0.35 + seed.x);
    let v = (uv * 2.0 - Vec2::ONE) * (0.018 + seed.w * 0.040) * p.motion.z.max(0.1);
    let rotated = Vec2::new(
        v.x * angle.cos() - v.y * angle.sin(),
        v.x * angle.sin() + v.y * angle.cos(),
    );
    p.right.truncate() * rotated.x + p.up.truncate() * rotated.y
}

fn particle_mesh(capacity: usize, first_seed: u32) -> Mesh {
    let mut normals = Vec::with_capacity(capacity * 4);
    let mut colors = Vec::with_capacity(capacity * 4);
    let mut uv = Vec::with_capacity(capacity * 4);
    for i in 0..capacity {
        let seed = seed_values(first_seed + i as u32);
        for corner in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
            normals.push(seed.truncate().to_array());
            colors.push([seed.w, 0.0, 0.0, 1.0]);
            uv.push(corner);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[0.0, 0.0, 0.0]; capacity * 4],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0, 0.0]; capacity * 4]);
    mesh.insert_indices(Indices::U32(Vec::new()));
    mesh
}

fn set_count(mesh: &mut Mesh, count: usize) {
    let mut indices = Vec::with_capacity(count * 6);
    for i in 0..count as u32 {
        let b = i * 4;
        indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    mesh.insert_indices(Indices::U32(indices));
}

fn spawn_layers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<ParticleMaterial>,
    state: &mut WeatherParticles,
) {
    state.shelter_values = vec![-10000.0; GRID * GRID];
    let data = state
        .shelter_values
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    state.shelter = images.add(Image::new(
        Extent3d {
            width: GRID as u32,
            height: GRID as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::R32Float,
        RenderAssetUsages::default(),
    ));
    state.shelter_gpu = images.add(Image::new_fill(
        Extent3d {
            width: GRID as u32,
            height: GRID as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    ));
    {
        let (gpu, capacity, first_seed) = (false, CPU_CAPACITY, 1);
        let mesh = meshes.add(particle_mesh(capacity, first_seed));
        let material = materials.add(ParticleMaterial {
            base: StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                base_color: Color::srgba(0.86, 0.91, 0.96, 0.82),
                perceptual_roughness: 0.95,
                reflectance: 0.08,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            extension: ParticleExtension {
                params: default(),
                shelter: state.shelter.clone(),
            },
        });
        commands.spawn((
            WeatherMesh,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::IDENTITY,
            NoFrustumCulling,
            NotShadowCaster,
            NotShadowReceiver,
            Name::new(if gpu { "weather-gpu" } else { "weather-cpu" }),
        ));
        state.layers.push(Layer {
            mesh,
            material,
            count: 0,
            first_seed,
            gpu,
        });
    }
}

pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    live: Option<Res<crate::live::LiveDrive>>,
    mut precipitation: ResMut<PrecipitationState>,
) {
    if live.is_none() && keys.just_pressed(KeyCode::KeyP) {
        precipitation.enabled = !precipitation.enabled;
    }
}

#[allow(clippy::too_many_arguments)]
#[derive(SystemParam)]
pub struct WeatherDraw<'w, 's> {
    capabilities: Option<Res<'w, crate::train_effects_gpu::TrainParticleCapabilities>>,
    gpu_time: Option<Res<'w, Time<bevy_hanabi::EffectSimulation>>>,
    gpu_assets: Option<ResMut<'w, Assets<bevy_hanabi::EffectAsset>>>,
    gpu_effects: Query<
        'w,
        's,
        (
            &'static mut bevy_hanabi::EffectProperties,
            &'static mut bevy_hanabi::EffectSpawner,
            &'static bevy_hanabi::CompiledParticleEffect,
        ),
        With<HanabiWeather>,
    >,
    lamps: Query<
        'w,
        's,
        (
            &'static crate::train_lighting::Headlamp,
            &'static GlobalTransform,
            &'static SpotLight,
        ),
    >,
    solids: Option<Res<'w, crate::effect_obstacles::EffectObstacles>>,
    sun: Option<Res<'w, crate::route_lighting::RouteSunState>>,
    fog: Option<Res<'w, crate::sky::FogState>>,
    obstacles: Query<
        'w,
        's,
        (
            &'static GlobalTransform,
            &'static bevy::camera::primitives::Aabb,
            Option<&'static MeshMaterial3d<StandardMaterial>>,
            Option<&'static crate::surface_weather::SnowSurfaceSource>,
            Option<&'static MeshMaterial3d<openrailsrs_bevy_scenery::OrSceneryMaterial>>,
        ),
        Without<WeatherMesh>,
    >,
    originals: Res<'w, Assets<StandardMaterial>>,
    native_materials: Res<'w, Assets<openrailsrs_bevy_scenery::OrSceneryMaterial>>,
    environment: Option<Res<'w, crate::environment::LiveEnvironment>>,
    content: Option<Res<'w, crate::player_launch::ActivePlayerContent>>,
    atmosphere: Option<Res<'w, crate::weather_state::WeatherState>>,
    state: ResMut<'w, WeatherParticles>,
    meshes: ResMut<'w, Assets<Mesh>>,
    images: ResMut<'w, Assets<Image>>,
    materials: ResMut<'w, Assets<ParticleMaterial>>,
}
pub fn update(
    mut commands: Commands,
    time: Res<Time<Real>>,
    live: Option<Res<crate::live::LiveDrive>>,
    precipitation: Res<PrecipitationState>,
    preferences: Res<crate::player_settings::PlayerSettings>,
    pipelines: Option<Res<crate::performance::ScenePipelineStatus>>,
    memory: Res<crate::gpu_memory::GraphicsMemory>,
    loading: Option<Res<crate::world::WorldSpawnProgress>>,
    startup: Option<Res<crate::route_bootstrap::ViewerLoadingScreen>>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    focus: Res<crate::world::RouteFocus>,
    terrain: Option<Res<crate::terrain::TerrainElevation>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    draw: WeatherDraw,
) {
    let WeatherDraw {
        capabilities,
        gpu_time,
        mut gpu_assets,
        mut gpu_effects,
        lamps,
        sun,
        fog,
        solids,
        obstacles,
        originals,
        native_materials,
        environment,
        content,
        atmosphere,
        mut state,
        mut meshes,
        mut images,
        mut materials,
    } = draw;
    let Ok(camera) = cameras.single() else { return };
    if !precipitation.enabled {
        state.hanabi.disable(&mut commands);
        for layer in &mut state.layers {
            if layer.count > 0 {
                if let Some(mut mesh) = meshes.get_mut(&layer.mesh) {
                    set_count(&mut mesh, 0);
                }
                layer.count = 0;
            }
        }
        state.cpu_particles = 0;
        state.gpu_particles = 0;
        return;
    }
    if state.layers.is_empty() {
        spawn_layers(
            &mut commands,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut state,
        );
    }
    let center = camera.translation();
    let hardware = pipelines
        .as_ref()
        .and_then(|p| p.device())
        .is_some_and(|d| d.hardware);
    let requested = WeatherExecution::from_env().unwrap_or(preferences.weather_execution);
    state.requested = requested;
    state.adaptive.observe(
        time.delta_secs(),
        loading.is_some() || startup.is_some(),
        memory.pressure(),
    );
    let quality = std::env::var("OPENRAILSRS_WEATHER_QUALITY")
        .ok()
        .as_deref()
        .and_then(crate::weather_execution::WeatherQuality::parse)
        .unwrap_or(preferences.weather_quality);
    if let Some(level) = quality.level() {
        state.adaptive.level = level;
    }
    let compatible = hardware
        && gpu_assets.is_some()
        && capabilities
            .as_ref()
            .is_some_and(|c| c.supported && c.hardware);
    let mode = requested.resolved(compatible, memory.pressure() || state.adaptive.level > 0);
    let (mut gpu_count, mut cpu_count) = state.adaptive.counts(mode);
    let preferred = preferences.weather_particle_budget / [1, 2, 4][state.adaptive.level.min(2)];
    let total = gpu_count + cpu_count;
    if total > preferred {
        gpu_count = gpu_count * preferred / total;
        cpu_count = (preferred - gpu_count).min(CPU_CAPACITY);
    }
    if let Some(budget) = std::env::var("OPENRAILSRS_WEATHER_PARTICLE_BUDGET")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        let budget = budget.clamp(128, GPU_CAPACITY);
        match mode {
            WeatherExecution::Hybrid => {
                gpu_count = budget * 3 / 4;
                cpu_count = (budget - gpu_count).min(CPU_CAPACITY);
            }
            WeatherExecution::Cpu => cpu_count = budget.min(CPU_CAPACITY),
            _ => gpu_count = budget,
        }
    }
    let sample = environment
        .as_ref()
        .zip(content.as_ref())
        .and_then(|(e, c)| e.current_sample(c.environment));
    let gpu_capacity = gpu_count;
    if let Some(atmosphere) = atmosphere.as_ref() {
        let scale = if precipitation.snow {
            atmosphere.atmosphere.snow
        } else {
            atmosphere.atmosphere.rain
        };
        gpu_count = (gpu_count as f32 * scale) as usize;
        cpu_count = (cpu_count as f32 * scale) as usize;
    } else if let Some(sample) = sample {
        let intensity = if precipitation.snow {
            sample.snowfall / 0.15
        } else {
            sample.precipitation / 2.5
        };
        let scale = intensity.clamp(0.15, 1.0);
        gpu_count = (gpu_count as f32 * scale) as usize;
        cpu_count = (cpu_count as f32 * scale) as usize;
    }
    state.execution = mode;
    state.gpu_particles = gpu_count;
    state.cpu_particles = cpu_count;
    state.gpu_delta_s = gpu_time.as_ref().map(|t| t.delta_secs());
    let clock_s = live.map_or_else(|| time.elapsed_secs_f64(), |l| l.session.time_s());
    let clock = clock_s as f32;
    let desired_wind = atmosphere.as_ref().map_or_else(
        || crate::environment::weather_wind(sample),
        |s| s.atmosphere.wind_mps,
    );
    let desired_wind = Vec2::new(desired_wind.x, desired_wind.z);
    if (sample.is_some() || atmosphere.is_some()) && state.live_wind.is_none() {
        state.live_wind = Some(WindDrift {
            offset: desired_wind.as_dvec2() * clock_s,
            velocity: desired_wind,
            clock: clock_s,
        });
    }
    let wind = if let Some(drift) = state.live_wind.as_mut() {
        let offset = drift.advance(clock_s, desired_wind);
        Vec4::new(offset.x, 1.0, offset.y, clock)
    } else {
        Vec4::new(desired_wind.x, 0.0, desired_wind.y, clock)
    };
    // Reduce BEFORE converting to f32 so large cumulative origin shifts retain
    // sub-centimetre phase; a rebase never restarts the field or its clock.
    let phase = |value: f32, shift: f32, period: f64| {
        ((value as f64 + shift as f64).rem_euclid(period)) as f32
    };
    let mut params = ParticleUniforms {
        center: center.extend(HALF),
        phase: Vec4::new(
            phase(center.x, origin.shift.x, HALF as f64 * 2.0),
            phase(center.y, 0.0, HEIGHT as f64),
            phase(center.z, origin.shift.z, HALF as f64 * 2.0),
            HEIGHT,
        ),
        right: camera.right().as_vec3().extend(1.0),
        up: camera.up().as_vec3().extend(f32::from(precipitation.snow)),
        wind_time: wind,
        grid: Vec4::new(state.shelter_center.x, state.shelter_center.y, HALF, 0.0),
        motion: Vec4::new(
            desired_wind.x,
            desired_wind.y,
            atmosphere.as_ref().map_or(1.0, |s| s.atmosphere.flake_size),
            1.0,
        ),
    };
    state.shelter_clock += time.delta_secs();
    if state.shelter_refreshes == 0
        || state.shelter_clock >= 0.5
        || state.shelter_center.distance(center.xz()) > 3.0
        || state.shelter_origin != origin.shift
    {
        state.shelter_clock = 0.0;
        state.shelter_center = center.xz();
        state.shelter_base = center.y - 256.0;
        state.shelter_origin = origin.shift;
        for z in 0..GRID {
            for x in 0..GRID {
                let px = center.x - HALF + (x as f32 + 0.5) / GRID as f32 * HALF * 2.0;
                let pz = center.z - HALF + (z as f32 + 0.5) / GRID as f32 * HALF * 2.0;
                state.shelter_values[z * GRID + x] = terrain
                    .as_ref()
                    .and_then(|t| {
                        t.sample_world_y(
                            px + origin.shift.x + focus.center.x,
                            pz + origin.shift.z + focus.center.z,
                        )
                    })
                    .map_or(-10000.0, |h| h - focus.height_origin);
            }
        }
        for (tf, bounds, standard, source, native) in &obstacles {
            let alpha = standard
                .map(|s| &s.0)
                .or_else(|| source.map(|s| &s.0))
                .and_then(|h| originals.get(h))
                .map(|m| m.alpha_mode)
                .or_else(|| {
                    native
                        .and_then(|s| native_materials.get(&s.0))
                        .map(|m| m.alpha_mode)
                });
            if solids.is_some() {
                continue;
            }
            // Cutout tree cards and glass must not create rectangular roofs.
            if alpha != Some(AlphaMode::Opaque) {
                continue;
            }
            let affine = tf.affine();
            let center = affine.transform_point3(bounds.center.into());
            let extent = affine.matrix3.abs() * bounds.half_extents;
            if extent.x < 0.1 || extent.z < 0.1 {
                continue;
            }
            let grid_center = state.shelter_center;
            raster_roof(
                &mut state.shelter_values,
                grid_center,
                center,
                extent.into(),
            );
        }
        if let Some(solids) = &solids {
            let grid_center = state.shelter_center;
            for solid in &solids.solids {
                raster_roof(
                    &mut state.shelter_values,
                    grid_center,
                    (solid.min + solid.max) * 0.5,
                    (solid.max - solid.min) * 0.5,
                );
            }
        }
        let bytes = state
            .shelter_values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        if let Some(mut image) = images.get_mut(&state.shelter) {
            image.data = Some(bytes);
        }
        let encoded = encode_shelter(&state.shelter_values, state.shelter_base);
        if let Some(mut image) = images.get_mut(&state.shelter_gpu) {
            image.data = Some(encoded);
        }
        state.shelter_refreshes += 1;
    }
    params.grid = Vec4::new(state.shelter_center.x, state.shelter_center.y, HALF, 0.0);
    let mut gpu_updates = 0;
    let mut cpu_updates = 0;
    for layer in &mut state.layers {
        let count = if layer.gpu { gpu_count } else { cpu_count };
        let first_seed = if !layer.gpu && mode == WeatherExecution::Hybrid {
            gpu_count as u32 + 1
        } else {
            1
        };
        // Merely obtaining a mutable Mesh marks it modified in Bevy. Leave GPU
        // seed/vertex buffers untouched between quality/mode changes.
        if (count != layer.count || layer.first_seed != first_seed || (!layer.gpu && count > 0))
            && let Some(mut mesh) = meshes.get_mut(&layer.mesh)
        {
            if layer.gpu {
                gpu_updates += 1;
            } else {
                cpu_updates += 1;
            }
            if first_seed != layer.first_seed {
                *mesh = particle_mesh(
                    if layer.gpu {
                        GPU_CAPACITY
                    } else {
                        CPU_CAPACITY
                    },
                    first_seed,
                );
                layer.first_seed = first_seed;
                layer.count = usize::MAX;
            }
            if count != layer.count {
                set_count(&mut mesh, count);
                layer.count = count;
            }
            if !layer.gpu && count > 0 {
                params.right.w = 0.0;
                if let Some(VertexAttributeValues::Float32x3(positions)) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
                {
                    for i in 0..count {
                        let seed = seed_values(layer.first_seed + i as u32);
                        let center = particle_position(seed, &params);
                        for (j, uv) in [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]
                            .into_iter()
                            .enumerate()
                        {
                            positions[i * 4 + j] =
                                (center + particle_corner(seed, uv, &params)).to_array();
                        }
                    }
                }
            }
        }
        params.right.w = f32::from(layer.gpu);
        if let Some(mut material) = materials.get_mut(&layer.material) {
            material.extension.params = params;
        }
    }
    state.gpu_mesh_updates += gpu_updates;
    state.cpu_mesh_updates += cpu_updates;
    let shelter = state.shelter_gpu.clone();
    if let Some(assets) = gpu_assets.as_deref_mut() {
        state.hanabi.configure(
            &mut commands,
            assets,
            shelter,
            precipitation.snow,
            gpu_capacity,
        );
    } else {
        state.hanabi.disable(&mut commands);
    }
    if let Some(entity) = state.hanabi.entity
        && let Ok((mut properties, mut spawner, compiled)) = gpu_effects.get_mut(entity)
    {
        let light = sun.as_ref().map_or(1.0, |s| s.ambient_scale);
        let direction = sun.as_ref().map_or(Vec3::Y, |s| s.direction);
        let weather = content
            .as_ref()
            .map_or(crate::player_launch::PlayerWeather::Clear, |c| c.weather);
        let sky = atmosphere.as_ref().map_or_else(
            || crate::sky::sky_parameters(direction.y, weather, direction, clock_s),
            |s| crate::sky::atmosphere_parameters(direction.y, &s.atmosphere, direction, clock_s),
        );
        let extinction = if fog.as_ref().is_none_or(|f| f.enabled) {
            3.912
                / atmosphere.as_ref().map_or_else(
                    || crate::ground_fog::weather_visibility(weather),
                    |s| s.atmosphere.visibility_m,
                )
        } else {
            0.0
        };
        let fog = sky.horizon.truncate().extend(extinction);
        let (lamp_position, lamp_direction) = lamps
            .iter()
            .find(|(l, _, s)| l.service_index == 0 && s.intensity > 0.0)
            .map_or((Vec4::ZERO, Vec4::ZERO), |(_, t, s)| {
                (
                    t.translation()
                        .extend(s.intensity / (4.0 * std::f32::consts::PI) * 0.001),
                    t.forward().as_vec3().extend(s.outer_angle.cos()),
                )
            });
        *properties = crate::weather_particles_gpu::properties(
            &params,
            state.shelter_base,
            light,
            fog,
            lamp_position,
            lamp_direction,
            gpu_count,
        );
        state.hanabi.seed_once(&mut spawner, compiled.is_ready());
    }
}

fn encode_shelter(values: &[f32], base: f32) -> Vec<u8> {
    values
        .iter()
        .flat_map(|height| {
            let packed = (((height - base) / 512.0).clamp(0.0, 1.0) * 65535.0).round() as u16;
            [(packed >> 8) as u8, packed as u8, 0, 255]
        })
        .collect()
}

fn raster_roof(values: &mut [f32], grid_center: Vec2, center: Vec3, extent: Vec3) {
    let min =
        (center.xz() - extent.xz() - grid_center + Vec2::splat(HALF)) / (HALF * 2.0) * GRID as f32;
    let max =
        (center.xz() + extent.xz() - grid_center + Vec2::splat(HALF)) / (HALF * 2.0) * GRID as f32;
    let top = center.y + extent.y;
    for z in (min.y.floor().max(0.0) as usize)..(max.y.ceil().max(0.0) as usize).min(GRID) {
        for x in (min.x.floor().max(0.0) as usize)..(max.x.ceil().max(0.0) as usize).min(GRID) {
            values[z * GRID + x] = values[z * GRID + x].max(top);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_and_gpu_shelter_uploads_both_include_the_shared_station_roof() {
        use bevy::ecs::system::RunSystemOnce;
        let scene =
            crate::track::TrackScene::from_graph(crate::test_harness::tiny_graph_with_signal());
        crate::test_harness::with_replay_world(
            scene,
            crate::train::ReplayState::default(),
            |world| {
                world.insert_resource(PrecipitationState {
                    enabled: true,
                    ..default()
                });
                world.insert_resource(crate::player_settings::PlayerSettings::default());
                world.insert_resource(crate::gpu_memory::GraphicsMemory::default());
                world.insert_resource(WeatherParticles::default());
                world.insert_resource(Assets::<ParticleMaterial>::default());
                let mut solids = crate::effect_obstacles::EffectObstacles::default();
                solids.solids = vec![crate::effect_obstacles::Solid {
                    min: Vec3::new(-5.0, 3.0, -5.0),
                    max: Vec3::new(5.0, 4.0, 5.0),
                    owner: None,
                }];
                world.insert_resource(solids);
                world.run_system_once(crate::camera::spawn_camera).unwrap();
                world.run_system_once(update).unwrap();
                let state = world.resource::<WeatherParticles>();
                let images = world.resource::<Assets<Image>>();
                let pixel = (GRID / 2 * GRID + GRID / 2) * 4;
                let cpu = images.get(&state.shelter).unwrap().data.as_ref().unwrap();
                assert_eq!(
                    f32::from_le_bytes(cpu[pixel..pixel + 4].try_into().unwrap()),
                    4.0
                );
                let gpu = images
                    .get(&state.shelter_gpu)
                    .unwrap()
                    .data
                    .as_ref()
                    .unwrap();
                let decoded = state.shelter_base
                    + f32::from(u16::from_be_bytes([gpu[pixel], gpu[pixel + 1]])) / 65535.0 * 512.0;
                assert!((decoded - 4.0).abs() < 0.01);
            },
        );
    }
    #[test]
    fn encoded_gpu_roof_height_matches_cpu_at_a_centimetre_tolerance() {
        let base = -180.0;
        let heights = [-12.3, 0.0, 3.125, 83.65, 300.0];
        let bytes = encode_shelter(&heights, base);
        for (height, packed) in heights.iter().zip(bytes.as_chunks::<4>().0.iter()) {
            let decoded =
                base + f32::from(u16::from_be_bytes([packed[0], packed[1]])) / 65535.0 * 512.0;
            assert!((height - decoded).abs() < 0.01);
        }
    }
    #[test]
    fn updated_wind_does_not_retroactively_move_drops_and_pause_is_stable() {
        let mut drift = WindDrift {
            offset: bevy::math::DVec2::new(10., 20.),
            velocity: Vec2::X,
            clock: 600.,
        };
        assert_eq!(
            drift.advance(600., Vec2::new(-15., 10.)),
            Vec2::new(10., 20.)
        );
        let before = drift.advance(600.01, Vec2::new(-15., 10.));
        assert!(before.distance(Vec2::new(10., 20.)) < 0.1);
        assert_eq!(before, drift.advance(600.01, Vec2::new(-15., 10.)));
        assert!(drift.advance(0., Vec2::Y).is_finite());
    }
    #[test]
    fn camera_motion_and_origin_rebase_do_not_drag_or_restart_snow() {
        let seed = seed_values(517);
        let mut p = ParticleUniforms {
            center: Vec4::ZERO,
            phase: Vec4::ZERO,
            right: Vec3::X.extend(1.0),
            up: Vec3::Y.extend(1.0),
            wind_time: Vec4::new(0.8, 0.0, 0.3, 10.0),
            ..default()
        };
        let start = particle_position(seed, &p);
        p.center.x += 2.0;
        p.phase.x += 2.0;
        assert!((particle_position(seed, &p) - start).length() < 1e-4);
        p.center.x -= 256.0;
        let rebased = particle_position(seed, &p);
        assert!((rebased + Vec3::X * 256.0 - start).length() < 1e-4);
        assert!(particle_corner(seed, Vec2::ZERO, &p).length() < 0.10);
    }
    #[test]
    fn roof_height_mask_is_bounded_and_preserves_surroundings() {
        let mut map = vec![0.0; GRID * GRID];
        raster_roof(
            &mut map,
            Vec2::ZERO,
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(5.0, 1.0, 5.0),
        );
        assert_eq!(map[32 * GRID + 32], 4.0);
        assert_eq!(map[0], 0.0);
        raster_roof(
            &mut map,
            Vec2::ZERO,
            Vec3::new(200.0, 3.0, 200.0),
            Vec3::ONE,
        );
        assert_eq!(map[0], 0.0);
    }
}

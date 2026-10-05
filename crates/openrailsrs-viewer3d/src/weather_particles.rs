//! Bounded CPU/GPU precipitation with the same world-space analytic trajectories.
//! GPU mode uploads seeds once; only uniforms and the small shelter map change.
use crate::{
    precipitation::{PrecipitationState, rain_rng01},
    weather_execution::{AdaptiveWeather, WeatherExecution},
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

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct ParticleUniforms {
    pub center: Vec4,
    pub phase: Vec4,
    pub right: Vec4,
    pub up: Vec4,
    pub wind_time: Vec4,
    pub grid: Vec4,
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
    shelter_values: Vec<f32>,
    shelter_center: Vec2,
    shelter_origin: Vec3,
    shelter_clock: f32,
    adaptive: AdaptiveWeather,
    pub execution: WeatherExecution,
    pub requested: WeatherExecution,
    pub gpu_particles: usize,
    pub cpu_particles: usize,
    pub shelter_refreshes: u64,
    pub gpu_mesh_updates: u64,
    pub cpu_mesh_updates: u64,
}
impl WeatherParticles {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"execution":self.execution,"requested":self.requested,"gpu_particles":self.gpu_particles,
            "cpu_particles":self.cpu_particles,"quality_level":self.adaptive.level,
            "shelter_refreshes":self.shelter_refreshes,"seed_upload_capacity":GPU_CAPACITY+CPU_CAPACITY,
            "gpu_mesh_updates":self.gpu_mesh_updates,"cpu_mesh_updates":self.cpu_mesh_updates})
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
    let x = seed.x * HALF * 2.0 + p.wind_time.x * t + flutter.x;
    let z = seed.z * HALF * 2.0 + p.wind_time.z * t + flutter.y;
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
            + Vec3::Y * ((uv.y - 0.5) * (0.8 + seed.w * 0.65));
    }
    let angle = seed.w * std::f32::consts::TAU + p.wind_time.w * (0.35 + seed.x);
    let v = (uv * 2.0 - Vec2::ONE) * (0.018 + seed.w * 0.040);
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
    for (gpu, capacity, first_seed) in [(true, GPU_CAPACITY, 1), (false, CPU_CAPACITY, 1)] {
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
        obstacles,
        originals,
        native_materials,
        mut state,
        mut meshes,
        mut images,
        mut materials,
    } = draw;
    let Ok(camera) = cameras.single() else { return };
    if !precipitation.enabled {
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
    let mode = requested.resolved(hardware, memory.pressure() || state.adaptive.level > 0);
    let (mut gpu_count, mut cpu_count) = state.adaptive.counts(mode);
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
    state.execution = mode;
    state.gpu_particles = gpu_count;
    state.cpu_particles = cpu_count;
    let clock = live.map_or_else(|| time.elapsed_secs(), |l| l.session.time_s() as f32);
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
        wind_time: Vec4::new(0.8, 0.0, 0.3, clock),
        grid: Vec4::new(state.shelter_center.x, state.shelter_center.y, HALF, 0.0),
    };
    state.shelter_clock += time.delta_secs();
    if state.shelter_refreshes == 0
        || state.shelter_clock >= 0.5
        || state.shelter_center.distance(center.xz()) > 3.0
        || state.shelter_origin != origin.shift
    {
        state.shelter_clock = 0.0;
        state.shelter_center = center.xz();
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
        let bytes = state
            .shelter_values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        if let Some(mut image) = images.get_mut(&state.shelter) {
            image.data = Some(bytes);
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

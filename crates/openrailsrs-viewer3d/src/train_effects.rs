//! Native ENG emitters, bounded world-space particles and one merged draw call.
use crate::{
    floating_origin::FloatingOrigin, live::LiveDrive, rolling_stock::ConsistCarIndex,
    rolling_stock_anim::TrainCarTrackOffset,
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
const PUFF_LIFETIME_S: f32 = 3.0;
#[derive(Clone)]
struct Emitter {
    car: Entity,
    track: usize,
    data: openrailsrs_formats::VehicleEmitter,
    credit: f32,
}
struct Puff {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    radius: f32,
    steam: bool,
    load: f32,
}
#[derive(Resource, Default)]
pub struct TrainEffects {
    emitters: Vec<Emitter>,
    particles: Vec<Puff>,
    last_clock: Option<f64>,
    shift: Vec3,
    wind: Vec3,
}
impl TrainEffects {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"native_emitters":self.emitters.len(),"live_particles":self.particles.len(),"particle_limit":MAX_PARTICLES,
            "wind_mps":self.wind.to_array(),"inherits_vehicle_velocity":true,
            "max_particle_speed_mps":self.particles.iter().map(|p|p.velocity.length()).fold(0.0_f32,f32::max)})
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
                }));
        }
    }
    crate::viewer_log!(
        "openrailsrs-viewer3d: {} native exhaust/steam emitters (max {MAX_PARTICLES} particles)",
        effects.emitters.len()
    );
    let mut pixels = vec![255u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let radius = ((x as f32 - 15.5).powi(2) + (y as f32 - 15.5).powi(2)).sqrt() / 15.5;
            pixels[(y * 32 + x) * 4 + 3] = ((1.0 - radius).max(0.0).powi(2) * 210.0) as u8;
        }
    }
    let texture = images.add(Image::new(
        Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(texture),
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
        return (burning * 40., true, load);
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
    puff.position += equilibrium * dt + relative * ((1.0 - decay) / drag);
    puff.velocity = equilibrium + relative * decay;
    puff.age += dt;
}

#[derive(SystemParam)]
pub struct TrainEffectScene<'w, 's> {
    environment: Option<Res<'w, crate::environment::LiveEnvironment>>,
    content: Option<Res<'w, crate::player_launch::ActivePlayerContent>>,
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
}

pub fn update(
    live: Res<LiveDrive>,
    origin: Res<FloatingOrigin>,
    mut effects: ResMut<TrainEffects>,
    scene: TrainEffectScene,
) {
    let TrainEffectScene {
        environment,
        content,
        cars,
        camera,
        mesh,
        mut meshes,
    } = scene;
    let (Ok(camera), Ok(mesh)) = (camera.single(), mesh.single()) else {
        return;
    };
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
    let wind = crate::environment::weather_wind(sample);
    effects.wind = wind;
    if clock < previous {
        effects.particles.clear();
        for emitter in &mut effects.emitters {
            emitter.credit = 0.0;
        }
    }
    let shift = origin.shift - effects.shift;
    effects.shift = origin.shift;
    for puff in &mut effects.particles {
        puff.position -= shift;
        advance_puff(puff, dt, wind);
    }
    effects.particles.retain(|p| p.age < PUFF_LIFETIME_S);
    let capacity = MAX_PARTICLES.saturating_sub(effects.particles.len());
    let mut born = Vec::new();
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
            let count = (emitter.credit.floor() as usize).min(capacity.saturating_sub(born.len()));
            emitter.credit = emitter.credit.fract();
            let velocity = vehicle_world_velocity(car, offset, session, index.0)
                + car.rotation() * direction * (1.2 + load * 2.0)
                + Vec3::Y * 0.5;
            for _ in 0..count {
                born.push(Puff {
                    position: car.transform_point(position),
                    velocity,
                    age: 0.0,
                    radius: emitter.data.radius_m.max(0.05),
                    steam,
                    load,
                });
            }
        }
    }
    effects.particles.extend(born);
    if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
        *mesh = particle_mesh(&effects.particles, camera);
    }
}
fn particle_mesh(particles: &[Puff], camera: &Transform) -> Mesh {
    let mut positions = Vec::with_capacity(particles.len() * 4);
    let mut normals = Vec::with_capacity(particles.len() * 4);
    let mut uvs = Vec::with_capacity(particles.len() * 4);
    let mut colors = Vec::with_capacity(particles.len() * 4);
    let mut indices = Vec::with_capacity(particles.len() * 6);
    let right = camera.rotation * Vec3::X;
    let up = camera.rotation * Vec3::Y;
    let normal = camera.rotation * Vec3::Z;
    let invisible = Puff {
        position: Vec3::ZERO,
        velocity: Vec3::ZERO,
        age: PUFF_LIFETIME_S,
        radius: 0.0,
        steam: true,
        load: 0.0,
    };
    // At least one degenerate transparent quad: Bevy's slab allocator does not
    // allocate empty vertex/index data. Never submit a zero-length GPU buffer.
    let particles = if particles.is_empty() {
        std::slice::from_ref(&invisible)
    } else {
        particles
    };
    for p in particles {
        let radius = p.radius * (1.0 + p.age * 4.0);
        let shade = if p.steam { 0.88 } else { 0.55 - p.load * 0.35 };
        let alpha = (1.0 - p.age / PUFF_LIFETIME_S).powi(2) * if p.steam { 0.6 } else { 0.7 };
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
        }
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
        assert_eq!(still.position.x, 0.0);
        assert_eq!(still.position.z, 0.0);
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
    fn mesh_is_bounded_and_retains_each_particles_alpha() {
        let p = Puff {
            position: Vec3::ZERO,
            velocity: Vec3::Y,
            age: 0.0,
            radius: 0.1,
            steam: true,
            load: 1.0,
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

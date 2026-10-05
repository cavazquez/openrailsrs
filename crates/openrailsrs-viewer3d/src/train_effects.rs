//! Native ENG emitters, bounded world-space particles and one merged draw call.
use crate::{
    floating_origin::FloatingOrigin, live::LiveDrive, rolling_stock::ConsistCarIndex,
    rolling_stock_anim::TrainCarTrackOffset,
};
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::HashMap;

const MAX_PARTICLES: usize = 512;
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
}
impl TrainEffects {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"native_emitters":self.emitters.len(),"live_particles":self.particles.len(),"particle_limit":MAX_PARTICLES})
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
fn emission(name: &str, session: &openrailsrs_sim::LiveDriveSession) -> (f32, bool, f32) {
    let n = name.to_ascii_lowercase();
    let t = session.cab_telemetry();
    let load = t.traction_load_fraction.max(session.state.throttle * 0.5) as f32;
    if n.starts_with("exhaust") {
        return (3.0 + 20.0 * load, false, load);
    }
    if n.contains("stack") {
        return (5.0 + 24.0 * session.state.throttle as f32, true, load);
    }
    if n.contains("cylinder") {
        return (
            if session.velocity_mps() < 4.0 && session.state.throttle > 0.1 {
                18.0
            } else {
                0.0
            },
            true,
            1.0,
        );
    }
    if n.contains("whistle") {
        return (if t.horn_active { 24.0 } else { 0.0 }, true, 1.0);
    }
    if n.contains("safety") {
        let open = session
            .physics
            .steam_params
            .as_ref()
            .zip(t.boiler_bar)
            .is_some_and(|(p, b)| b > p.working_pressure_bar * 1.01);
        return (if open { 24.0 } else { 0.0 }, true, 1.0);
    }
    (0.0, true, 0.0)
}

pub fn update(
    live: Res<LiveDrive>,
    origin: Res<FloatingOrigin>,
    mut effects: ResMut<TrainEffects>,
    cars: Query<&GlobalTransform, With<ConsistCarIndex>>,
    camera: Query<&Transform, With<Camera3d>>,
    mesh: Query<&Mesh3d, With<ExhaustMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (Ok(camera), Ok(mesh)) = (camera.single(), mesh.single()) else {
        return;
    };
    let clock = live.session.time_s();
    let previous = effects.last_clock.replace(clock).unwrap_or(clock);
    let dt = (clock - previous).clamp(0.0, 0.15) as f32;
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
        puff.age += dt;
        puff.position += puff.velocity * dt;
        puff.velocity += Vec3::Y * dt * 0.35;
    }
    effects.particles.retain(|p| p.age < 3.0);
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
            let (Some(session), Ok(car)) = (session, cars.get(emitter.car)) else {
                continue;
            };
            if car.translation().distance(camera.translation) > 250.0 {
                continue;
            }
            let (rate, steam, load) = emission(&emitter.data.name, session);
            emitter.credit += rate * dt;
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
            while emitter.credit >= 1.0 {
                emitter.credit -= 1.0;
                born.push(Puff {
                    position: car.transform_point(position),
                    velocity: car.rotation() * direction * (1.2 + load * 2.0)
                        + Vec3::new(0.4, 0.5, 0.15),
                    age: 0.0,
                    radius: emitter.data.radius_m.max(0.05),
                    steam,
                    load,
                });
            }
        }
    }
    let capacity = MAX_PARTICLES.saturating_sub(effects.particles.len());
    effects.particles.extend(born.into_iter().take(capacity));
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
        age: 3.0,
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
        let alpha = (1.0 - p.age / 3.0).powi(2) * if p.steam { 0.6 } else { 0.7 };
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

//! Coarse native heightfields behind the streamed detailed terrain. Bounded
//! resident tiles, background reads and no replacement of the close scenery.
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task},
};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
};
#[derive(Component)]
struct DistantTile((i32, i32));
#[derive(Resource, Default)]
pub struct Horizon {
    center: Option<(i32, i32)>,
    requested_radius_m: f32,
    pressured: bool,
    pending: VecDeque<(i32, i32, PathBuf)>,
    task: Option<Task<Option<((i32, i32), Mesh, Color)>>>,
    tiles: HashMap<(i32, i32), (Entity, Handle<Mesh>, Handle<StandardMaterial>)>,
    pub published: u64,
}
pub fn install(app: &mut App) {
    app.init_resource::<Horizon>()
        .add_systems(
            Update,
            update.run_if(in_state(crate::ViewerAppState::Playing)),
        )
        .add_systems(OnExit(crate::ViewerAppState::Playing), cleanup);
}
fn coarse(path: PathBuf, x: i32, z: i32) -> Option<((i32, i32), Mesh, Color)> {
    let tile = openrailsrs_formats::TerrainFile::from_path_with_coords(&path, x, z).ok()?;
    let data = crate::terrain_io::load_tile_data(&tile, &path)?;
    let size = 2048.;
    let mut positions = Vec::with_capacity(289);
    let mut indices = Vec::with_capacity(1536);
    for z in 0..=16 {
        for x in 0..=16 {
            let px = x as f32 * size / 16.;
            let pz = z as f32 * size / 16.;
            positions.push([
                px,
                data.grid
                    .sample_or_triangle(px as f64, pz as f64, tile.samples.sample_size),
                -pz,
            ]);
        }
    }
    for z in 0..16 {
        for x in 0..16 {
            let stride = (data.grid.nsamples - 1) / 16;
            if data.features.as_ref().is_some_and(|f| {
                (z * stride..=(z + 1) * stride)
                    .any(|zz| (x * stride..=(x + 1) * stride).any(|xx| f.is_vertex_hidden(xx, zz)))
            }) {
                continue;
            }
            let i = (z * 17 + x) as u32;
            indices.extend([i, i + 17, i + 1, i + 1, i + 17, i + 18]);
        }
    }
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 1., 0.]; 289]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        (0..289)
            .map(|i| [(i % 17) as f32 / 16., (i / 17) as f32 / 16.])
            .collect::<Vec<_>>(),
    );
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    // A distant average is derived from the author's ground texture, rather
    // than a flat plane guessed from the locomotive's height.
    let color = tile
        .shaders
        .first()
        .and_then(|s| s.texslots.first())
        .and_then(|slot| {
            let root = path.parent()?.parent()?;
            let base = openrailsrs_formats::resolve_path_case_insensitive(&root.join("TERRTEX"))?
                .canonicalize()
                .ok()?;
            let tex =
                openrailsrs_formats::resolve_path_case_insensitive(&base.join(&slot.filename))?
                    .canonicalize()
                    .ok()?;
            if !tex.starts_with(base) || tex.metadata().ok()?.len() > 32 * 1024 * 1024 {
                return None;
            }
            let ace = openrailsrs_ace::read_ace_limited(tex, 32 * 1024 * 1024).ok()?;
            let mut sum = [0u64; 3];
            let mut count = 0;
            for p in ace.mip0.chunks_exact(4).step_by(64) {
                for i in 0..3 {
                    sum[i] += u64::from(p[i]);
                }
                count += 1;
            }
            (count > 0).then(|| {
                Color::srgb(
                    sum[0] as f32 / count as f32 / 255.,
                    sum[1] as f32 / count as f32 / 255.,
                    sum[2] as f32 / count as f32 / 255.,
                )
            })
        })
        .unwrap_or(Color::srgb(0.22, 0.27, 0.19));
    Some(((x, z), mesh, color))
}
#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    mut state: ResMut<Horizon>,
    assets: Res<crate::shapes::RouteAssets>,
    focus: Res<crate::world::RouteFocus>,
    origin: Res<crate::floating_origin::FloatingOrigin>,
    window: Res<crate::view_window::ViewWindow>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    near: Query<&crate::terrain_spawn::TerrainTileTag>,
    mut tiles: Query<(&DistantTile, &mut Transform, &mut Visibility)>,
    memory: Res<crate::gpu_memory::ResidencyPolicy>,
) {
    let _profile = crate::performance::profile_system("horizon");
    let center = (
        openrailsrs_formats::msts_tile_x_index_for_coord(window.center_world.x),
        openrailsrs_formats::msts_tile_z_index_for_coord(window.center_world.z),
    );
    if state.center != Some(center)
        || (state.requested_radius_m - window.radius_m).abs() > 1.
        || state.pressured != memory.pressured
    {
        state.requested_radius_m = window.radius_m;
        state.pressured = memory.pressured;
        state.center = Some(center);
        state.pending = crate::terrain::discover_terrain_tile_entries(
            &assets.route_dir,
            Some(window.center_world),
            window.radius_m + 8000.,
        )
        .into_iter()
        .filter(|(x, z, _)| !state.tiles.contains_key(&(*x, *z)))
        .take(if memory.pressured { 25 } else { 64 })
        .collect();
        let stale = state
            .tiles
            .keys()
            .filter(|(x, z)| x.abs_diff(center.0) > 5 || z.abs_diff(center.1) > 5)
            .copied()
            .collect::<Vec<_>>();
        for key in stale {
            if let Some((entity, mesh, material)) = state.tiles.remove(&key) {
                commands.entity(entity).despawn();
                meshes.remove(mesh.id());
                materials.remove(material.id());
            }
        }
    }
    if let Some(task) = state.task.as_mut()
        && let Some(result) = bevy::tasks::block_on(bevy::tasks::poll_once(task))
    {
        state.task = None;
        if let Some((key, mesh, color)) = result {
            let mesh = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 1.,
                ..default()
            });
            let entity = commands
                .spawn((
                    DistantTile(key),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    bevy::light::NotShadowCaster,
                    crate::surface_weather::SnowReceiver,
                    Name::new("native-distant-heightfield"),
                ))
                .id();
            state.tiles.insert(key, (entity, mesh, material));
            state.published += 1;
        }
    }
    let cap = if memory.pressured { 25 } else { 64 };
    while state.tiles.len() > cap || (state.tiles.len() >= cap && !state.pending.is_empty()) {
        let oldest = state
            .tiles
            .keys()
            .max_by_key(|(x, z)| x.abs_diff(center.0) + z.abs_diff(center.1))
            .copied();
        if let Some(key) = oldest
            && let Some((e, m, mat)) = state.tiles.remove(&key)
        {
            commands.entity(e).despawn();
            meshes.remove(m.id());
            materials.remove(mat.id());
        } else {
            break;
        }
    }
    if state.task.is_none()
        && state.tiles.len() < cap
        && let Some((x, z, path)) = state.pending.pop_front()
    {
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move { coarse(path, x, z) }));
    }
    for (tile, mut tf, mut visibility) in &mut tiles {
        let (x, z) = tile.0;
        let (wx, wz) = openrailsrs_formats::msts_tile_world_origin(x, z);
        tf.translation = focus.to_render_surface(Vec3::new(wx, 0., wz)) - origin.shift;
        *visibility = if near.iter().any(|n| n.tile_x == x && n.tile_z == z) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}
fn cleanup(
    mut commands: Commands,
    mut state: ResMut<Horizon>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (_, (e, m, mat)) in state.tiles.drain() {
        commands.entity(e).despawn();
        meshes.remove(m.id());
        materials.remove(mat.id());
    }
    state.pending.clear();
    state.task = None;
    state.center = None;
}

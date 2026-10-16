//! Owned scenery for visibility/refinement parity and bounded frame measurements.
use bevy::{
    asset::RenderAssetUsages,
    camera::primitives::MeshAabb,
    prelude::*,
    render::{
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        view::screenshot::{Screenshot, save_to_disk},
    },
};
use openrailsrs_bevy_scenery::progressive_mips::{
    AUTHORED_TEXTURE_LABEL, MipStreamingStats, ProgressiveMipsPlugin,
};
use openrailsrs_viewer3d::world_instancing::{
    WorldDrawCounters, WorldInstanceBuffer, WorldInstanceData, WorldInstancingPlugin,
    WorldOccluder, appearance_from_standard_material, instances_aabb,
};
use std::{path::PathBuf, time::Instant};

#[derive(Component)]
struct SceneRoot;
#[derive(Component)]
struct Eye;
#[derive(Resource)]
struct Bench {
    out: PathBuf,
    frame: u32,
    last: Instant,
    started: Instant,
    first_opaque_ms: Option<f64>,
    times: Vec<f64>,
    records: Vec<serde_json::Value>,
    max_asset_bytes: usize,
    instances_per_group: usize,
}
fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("visibility_oracle OUT_DIR"));
    let instances_per_group = std::env::args()
        .nth(2)
        .map(|n| n.parse::<usize>().expect("instance count"))
        .unwrap_or(128);
    assert!((16..=8192).contains(&instances_per_group));
    std::fs::create_dir_all(&out).unwrap();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(openrailsrs_bevy_scenery::shared_asset_plugin())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Visibility oracle".into(),
                        resolution: (512, 256).into(),
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins((WorldInstancingPlugin, ProgressiveMipsPlugin))
        .insert_resource(ClearColor(Color::srgb(0.015, 0.02, 0.03)))
        .insert_resource(Bench {
            out,
            frame: 0,
            last: Instant::now(),
            started: Instant::now(),
            first_opaque_ms: None,
            times: vec![],
            records: vec![],
            max_asset_bytes: 0,
            instances_per_group,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, capture)
        .run();
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    bench: Res<Bench>,
) {
    commands.spawn((
        Camera3d::default(),
        Eye,
        bevy::render::view::NoIndirectDrawing,
        Msaa::Off,
        AmbientLight {
            brightness: 500.,
            ..default()
        },
        Transform::from_xyz(0., 4., 25.).looking_at(Vec3::new(0., 2., -25.), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.5, 0.)),
    ));
    let mut image = Image::new_uninit(
        Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = 10;
    image.texture_descriptor.label = Some(AUTHORED_TEXTURE_LABEL);
    let mut pixels = vec![];
    for level in 0..10 {
        let side = (512usize >> level).max(1);
        for _ in 0..side * side {
            pixels.extend(if level == 0 {
                [255, 255, 255, 255]
            } else {
                [15, 60, 230, 255]
            });
        }
    }
    image.data = Some(pixels);
    let texture = images.add(image);
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.25, 0.15),
        base_color_texture: Some(texture.clone()),
        cull_mode: None,
        double_sided: true,
        perceptual_roughness: 1.,
        ..default()
    });
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.5, 0.55),
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let mask_mesh = Mesh::from(Rectangle::new(16., 10.));
    let mask_bounds = mask_mesh.compute_aabb();
    let mask_mesh = meshes.add(mask_mesh);
    let mut mask_pixels = vec![];
    for y in 0..8 {
        for x in 0..8 {
            mask_pixels.extend([64, 255, 64, if (x + y) % 2 == 0 { 0 } else { 255 }]);
        }
    }
    let mask_texture = images.add(Image::new_fill(
        Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &mask_pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let mask_material = materials.add(StandardMaterial {
        base_color_texture: Some(mask_texture),
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    let wall = meshes.add(Cuboid::new(24., 14., 0.5));
    let floor = meshes.add(Cuboid::new(120., 0.2, 180.));
    let small = Mesh::from(Cuboid::new(0.5, 0.5, 0.5));
    let bounds = small.compute_aabb();
    let small = meshes.add(small);
    let sphere = Mesh::from(Sphere::new(0.35));
    let sphere_bounds = sphere.compute_aabb();
    let sphere = meshes.add(sphere);
    let big = Mesh::from(Cuboid::new(7., 7., 2.));
    let big_bounds = big.compute_aabb();
    let big = meshes.add(big);
    commands
        .spawn((SceneRoot, Transform::default(), Visibility::default()))
        .with_children(|p| {
            p.spawn((
                WorldOccluder,
                Mesh3d(wall),
                MeshMaterial3d(wall_material.clone()),
                Transform::from_xyz(0., 5., 0.),
            ));
            p.spawn((
                WorldOccluder,
                Mesh3d(floor),
                MeshMaterial3d(wall_material),
                Transform::from_xyz(0., -1., -30.),
            ));
            for group in 0..36 {
                let x = (group % 6) as f32 * 1.6 - 4.;
                let y = (group / 6) as f32 * 0.8;
                let data = vec![
                    WorldInstanceData::from_transform(Transform::from_xyz(x, y, -20.)),
                    WorldInstanceData::from_transform(Transform::from_xyz(x + 0.6, y, -21.)),
                ];
                p.spawn((
                    Mesh3d(small.clone()),
                    MeshMaterial3d(material.clone()),
                    instances_aabb(&data, bounds.as_ref()),
                    WorldInstanceBuffer(data.into()),
                    appearance_from_standard_material(&materials, &material),
                    Transform::default(),
                ));
            }
            let data = (0..64)
                .map(|i| {
                    WorldInstanceData::from_transform(Transform::from_xyz(
                        18. + i as f32 * 32.,
                        4.,
                        7.,
                    ))
                })
                .collect::<Vec<_>>();
            p.spawn((
                Mesh3d(mask_mesh),
                MeshMaterial3d(mask_material.clone()),
                instances_aabb(&data, mask_bounds.as_ref()),
                WorldInstanceBuffer(data.into()),
                appearance_from_standard_material(&materials, &mask_material),
                Transform::default(),
            ));
            let data = vec![
                WorldInstanceData::from_transform(Transform::from_xyz(13., 2., 3.)),
                WorldInstanceData::from_transform(Transform::from_xyz(14., 2., 3.)),
            ];
            p.spawn((
                Mesh3d(small.clone()),
                MeshMaterial3d(material.clone()),
                instances_aabb(&data, bounds.as_ref()),
                WorldInstanceBuffer(data.into()),
                appearance_from_standard_material(&materials, &material),
                Transform::default(),
            ));
            for group in 0..8 {
                let data = (0..bench.instances_per_group)
                    .map(|i| {
                        WorldInstanceData::from_transform(Transform::from_xyz(
                            (i as f32 - 64.) * 7.,
                            (group % 4) as f32,
                            -30. - group as f32 * 4.,
                        ))
                    })
                    .collect::<Vec<_>>();
                p.spawn((
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(material.clone()),
                    instances_aabb(&data, sphere_bounds.as_ref()),
                    WorldInstanceBuffer(data.into()),
                    appearance_from_standard_material(&materials, &material),
                    Transform::default(),
                ));
            }
            let data = vec![
                WorldInstanceData::from_transform(Transform::from_xyz(-5., 2.5, 12.)),
                WorldInstanceData::from_transform(Transform::from_xyz(7., 2.5, 10.)),
            ];
            p.spawn((
                Mesh3d(big),
                MeshMaterial3d(material.clone()),
                instances_aabb(&data, big_bounds.as_ref()),
                WorldInstanceBuffer(data.into()),
                appearance_from_standard_material(&materials, &material),
                Transform::default(),
            ));
        });
}
#[allow(clippy::too_many_arguments)] // Independent Bevy system parameters.
fn capture(
    mut commands: Commands,
    mut bench: ResMut<Bench>,
    mut eyes: Query<&mut Transform, With<Eye>>,
    mut roots: Query<&mut Transform, (With<SceneRoot>, Without<Eye>)>,
    counts: Res<WorldDrawCounters>,
    mips: Res<MipStreamingStats>,
    status: Res<openrailsrs_viewer3d::performance::ScenePipelineStatus>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = Instant::now();
    let ms = (now - bench.last).as_secs_f64() * 1000.;
    bench.last = now;
    bench.frame += 1;
    bench.max_asset_bytes = bench.max_asset_bytes.max(mips.submitted_asset_bytes);
    let has_opaque = counts.report()["opaque"].as_u64().unwrap_or(0) > 0;
    if has_opaque && bench.first_opaque_ms.is_none() {
        bench.first_opaque_ms = Some((now - bench.started).as_secs_f64() * 1000.);
    }
    if now - bench.started < std::time::Duration::from_secs(2) || !has_opaque {
        bench.frame = bench.frame.min(120);
        return;
    }
    if bench.frame <= 120 {
        return;
    }
    let phase = ((bench.frame - 121) / 120).min(3);
    let offset = if phase == 3 {
        Vec3::new(-5000., 0., 3000.)
    } else {
        Vec3::ZERO
    };
    for mut root in &mut roots {
        root.translation = offset;
    }
    let (position, focus) = match phase {
        1 => (Vec3::new(28., 7., 20.), Vec3::new(0., 2., -25.)),
        2 => (Vec3::new(0., 2., 0.3), Vec3::new(0., 2., -15.)),
        _ => (Vec3::new(0., 4., 25.), Vec3::new(0., 2., -25.)),
    };
    for mut eye in &mut eyes {
        *eye = Transform::from_translation(position + offset).looking_at(focus + offset, Vec3::Y);
    }
    let within = (bench.frame - 121) % 120;
    if within >= 60 {
        bench.times.push(ms);
    }
    if within == 100 {
        let path = bench.out.join(format!("phase-{phase}.png"));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        let times = std::mem::take(&mut bench.times);
        bench.records.push(serde_json::json!({"phase":phase,"frame_ms":times,"draws":counts.report(),"mips":{"staged":mips.staged,"refined":mips.refined,"pending_bytes":mips.pending_bytes}}));
    }
    if bench.frame >= 620 {
        let document = serde_json::json!({"scope":"owned stationary scenery; whole-frame time, not isolated GPU timestamps","window":[512,256],"source":"visibility_oracle.rs","records":bench.records,"first_opaque_ms":bench.first_opaque_ms,"device":status.device(),"instances_per_group":bench.instances_per_group,"max_refinement_asset_bytes_per_frame":bench.max_asset_bytes,"options":{"gpu":std::env::var("OPENRAILSRS_WORLD_GPU_CULLING").ok(),"occlusion":std::env::var("OPENRAILSRS_WORLD_OCCLUSION").ok(),"mips":std::env::var("OPENRAILSRS_PROGRESSIVE_MIPS").ok()}});
        std::fs::write(
            bench.out.join("result.json"),
            serde_json::to_vec_pretty(&document).unwrap(),
        )
        .unwrap();
        exit.write(AppExit::Success);
    }
}

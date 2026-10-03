//! GPU placement oracle: custom WORLD instances vs ordinary Bevy meshes.
//! No route assets are required; the translated group and unrelated mesh expose
//! dependence on Bevy's mesh-uniform ordering / automatic draw batching.

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::MeshAabb;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use openrailsrs_viewer3d::world_instancing::{
    WorldInstanceBuffer, WorldInstanceData, WorldInstancingPlugin,
    appearance_from_standard_material, instances_aabb,
};

#[derive(Resource)]
struct Capture {
    path: String,
    individual: bool,
    frames: u32,
    saved: bool,
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|s| s == "--verify") && args.len() == 3 {
        verify(&args[1], &args[2]);
        return;
    }
    assert!(
        !args.is_empty(),
        "scenery_oracle <output.png> [--individual] | --verify <individual.png> <instanced.png>"
    );
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "WORLD instance placement oracle".into(),
                resolution: (512, 256).into(),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WorldInstancingPlugin)
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Capture {
            path: args[0].clone(),
            individual: args.iter().any(|s| s == "--individual"),
            frames: 0,
            saved: false,
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
    capture: Res<Capture>,
) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Off,
        AmbientLight {
            brightness: 500.0,
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 14.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_rotation_x(-0.6)),
    ));
    // Keep the marker on an unrelated allocation. Sharing the tested mesh
    // could hide draw commands that depend on another mesh's GPU bindings.
    let marker_mesh = meshes.add(Sphere::new(0.4));
    let marker = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.8, 0.05),
        ..default()
    });
    commands.spawn((
        Mesh3d(marker_mesh),
        MeshMaterial3d(marker),
        Transform::from_xyz(0.0, 4.0, -5.0),
    ));
    let white = images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.05, 0.02),
        base_color_texture: Some(white),
        perceptual_roughness: 1.0,
        cull_mode: None,
        double_sided: true,
        ..default()
    });
    // Its source-mesh bounds are outside the frustum. Only the aggregate
    // instance bounds enclose the visible placements after the inverse below.
    let group = Transform::from_xyz(500.0, 200.0, -100.0).with_rotation(Quat::from_rotation_y(0.3));
    let inverse = group.to_matrix().inverse();
    // A back-facing open card, rather than a closed box whose far faces could
    // preserve the silhouette despite incorrect culling.
    let mut post = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    post.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.4, -1.25, 0.0],
            [0.4, 1.25, 0.0],
            [0.4, -1.25, 0.0],
            [-0.4, -1.25, 0.0],
            [-0.4, 1.25, 0.0],
            [0.4, 1.25, 0.0],
        ],
    );
    post.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, -1.0]; 6]);
    post.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0]; 6]);
    let mut crossbar = Mesh::from(Cuboid::new(2.5, 0.6, 0.6));
    crossbar.duplicate_vertices();
    // Imported MSTS parts use non-indexed triangle lists; test those alongside
    // indexed Bevy geometry, on distinct allocator ranges and mesh handles.
    for (x, mesh) in [
        (-5.0, meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
        (0.0, meshes.add(post)),
        (5.0, meshes.add(crossbar)),
    ] {
        let transforms = [-1.7, 1.7].map(|y| Transform::from_xyz(x, y, 0.0));
        let placements: Vec<_> = transforms
            .iter()
            .map(|tf| WorldInstanceData::from_mat4(inverse * tf.to_matrix()))
            .collect();
        if capture.individual {
            for transform in transforms {
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    transform,
                ));
            }
        } else {
            let aabb = instances_aabb(
                &placements,
                meshes.get(&mesh).and_then(MeshAabb::compute_aabb).as_ref(),
            );
            commands.spawn((
                Mesh3d(mesh),
                group,
                aabb,
                WorldInstanceBuffer(placements.into()),
                appearance_from_standard_material(&materials, &material),
            ));
        }
    }
}

fn capture(mut state: ResMut<Capture>, mut commands: Commands, mut exit: MessageWriter<AppExit>) {
    state.frames += 1;
    if state.saved {
        exit.write(AppExit::Success);
    } else if state.frames == 60 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(state.path.clone()))
            .observe(|_: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                capture.saved = true;
            });
    }
}

fn verify(reference: &str, candidate: &str) {
    let a = image::open(reference)
        .expect("individual screenshot")
        .to_rgb8();
    let b = image::open(candidate)
        .expect("instanced screenshot")
        .to_rgb8();
    assert_eq!(a.dimensions(), b.dimensions());
    let mask = |p: &image::Rgb<u8>| {
        p[0] > 40 && u16::from(p[0]) > 2 * u16::from(p[1]) && u16::from(p[0]) > 2 * u16::from(p[2])
    };
    let mut intersection = [0usize; 3];
    let mut union = [0usize; 3];
    let mut expected = [0usize; 3];
    for ((x, _, pa), pb) in a.enumerate_pixels().zip(b.pixels()) {
        let region = (x * 3 / a.width()).min(2) as usize;
        let ma = mask(pa);
        let mb = mask(pb);
        expected[region] += usize::from(ma);
        intersection[region] += usize::from(ma && mb);
        union[region] += usize::from(ma || mb);
    }
    for region in 0..3 {
        assert!(
            expected[region] > 400,
            "individual reference is empty in region {region}"
        );
        let overlap = intersection[region] as f64 / union[region].max(1) as f64;
        println!("WORLD region {region} silhouette overlap: {overlap:.6} (minimum 0.98)");
        assert!(
            overlap >= 0.98,
            "custom GPU instances changed placement or disappeared in region {region}"
        );
    }
}

//! Independent bounded rear views. The main UI camera never targets these textures.
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
};
#[derive(Component)]
pub struct MirrorCamera {
    pub image: Handle<Image>,
    pub vehicle: usize,
    pub definition: openrailsrs_scenarios::presentation::MirrorDef,
}
#[derive(Component)]
struct MirrorPanel {
    vehicle: usize,
}
#[derive(Resource, Default)]
pub struct Mirrors {
    pub views: usize,
    pub rendered_frames: u64,
    pub resolution: [u32; 2],
    last: f64,
}
pub fn install(app: &mut App) {
    app.init_resource::<Mirrors>()
        .add_systems(
            OnEnter(crate::ViewerAppState::Playing),
            spawn.after(crate::train_presentation::prepare),
        )
        .add_systems(
            PostUpdate,
            update
                .after(bevy::transform::TransformSystems::Propagate)
                // An activated camera must exist before Bevy builds its shadow
                // cascades. Activating it afterwards makes prepare_lights read
                // a missing cascade entry when the 10 Hz mirror wakes up.
                .before(bevy::camera::CameraUpdateSystems)
                .before(bevy::light::cascade::build_directional_light_cascades)
                .run_if(in_state(crate::ViewerAppState::Playing)),
        )
        .add_systems(OnExit(crate::ViewerAppState::Playing), cleanup);
}
fn spawn(
    mut commands: Commands,
    presentation: Res<crate::train_presentation::TrainPresentation>,
    mut images: ResMut<Assets<Image>>,
    mut state: ResMut<Mirrors>,
) {
    *state = Mirrors {
        resolution: [256, 160],
        ..Default::default()
    };
    for vehicle in &presentation.vehicles {
        for (side, definition) in vehicle.mirrors.iter().enumerate() {
            if state.views >= 4 {
                break;
            }
            let image = images.add(Image::new_target_texture(
                256,
                160,
                TextureFormat::Rgba8Unorm,
                Some(TextureFormat::Rgba8UnormSrgb),
            ));
            commands.spawn((
                MirrorCamera {
                    image: image.clone(),
                    vehicle: vehicle.vehicle,
                    definition: definition.clone(),
                },
                Camera3d::default(),
                bevy::camera::Exposure::default(),
                bevy::core_pipeline::tonemapping::Tonemapping::default(),
                Camera {
                    order: -10 - state.views as isize,
                    is_active: false,
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Projection::Perspective(PerspectiveProjection {
                    fov: definition.fov_degrees.to_radians(),
                    far: 600.,
                    ..default()
                }),
                RenderLayers::from_layers(&[
                    crate::cab_render::LAYER_WORLD,
                    crate::cab_render::LAYER_TRAIN_EXTERIOR,
                ]),
                bevy::render::view::NoIndirectDrawing,
                Msaa::Off,
                Transform::default(),
                crate::sky::camera_distance_fog(),
                bevy::light::NotShadowCaster,
                Name::new("cab-rear-mirror-camera"),
            ));
            commands.spawn((
                MirrorPanel {
                    vehicle: vehicle.vehicle,
                },
                Node {
                    position_type: PositionType::Absolute,
                    top: px(300),
                    left: if side == 0 { px(8) } else { auto() },
                    right: if side == 1 { px(8) } else { auto() },
                    width: px(256),
                    height: px(160),
                    border: UiRect::all(px(3)),
                    ..default()
                },
                ImageNode {
                    image,
                    flip_x: true,
                    ..default()
                },
                BorderColor::all(Color::srgb(0.2, 0.25, 0.3)),
                Visibility::Hidden,
                Name::new("cab-mirror-view"),
            ));
            state.views += 1;
        }
    }
}
#[allow(clippy::type_complexity)]
fn update(
    time: Res<Time<Real>>,
    mut state: ResMut<Mirrors>,
    live: Option<Res<crate::live::LiveDrive>>,
    follow: Res<crate::camera::CameraFollowMode>,
    budget: Res<crate::render_budget::RenderBudget>,
    cars: Query<
        (
            &crate::rolling_stock::ConsistCarIndex,
            &crate::rolling_stock_anim::TrainCarTrackOffset,
            &GlobalTransform,
        ),
        Without<MirrorCamera>,
    >,
    main: Query<
        (
            Option<&DistanceFog>,
            Option<&bevy::camera::Exposure>,
            Option<&bevy::core_pipeline::tonemapping::Tonemapping>,
        ),
        (With<Camera3d>, Without<MirrorCamera>),
    >,
    mut cameras: Query<
        (
            &MirrorCamera,
            &mut Camera,
            &mut Transform,
            &mut GlobalTransform,
            &mut DistanceFog,
            &mut bevy::camera::Exposure,
            &mut bevy::core_pipeline::tonemapping::Tonemapping,
        ),
        With<MirrorCamera>,
    >,
    mut panels: Query<(&MirrorPanel, &mut Visibility)>,
) {
    let _profile = crate::performance::profile_system("mirrors");
    let vehicle = live
        .as_ref()
        .map(|s| s.session.state.active_cab_vehicle.unwrap_or(0));
    let visible = matches!(
        *follow,
        crate::camera::CameraFollowMode::DriverCam | crate::camera::CameraFollowMode::Cab2d
    ) && !budget.memory_pressure;
    let refresh = time.elapsed_secs_f64() - state.last >= 0.1;
    if refresh {
        state.last = time.elapsed_secs_f64();
    }
    for (mirror, mut camera, mut tf, mut global, mut fog, mut exposure, mut tone) in &mut cameras {
        if let Ok((main_fog, main_exposure, main_tone)) = main.single() {
            if let Some(value) = main_fog {
                *fog = value.clone();
            }
            if let Some(value) = main_exposure {
                *exposure = *value;
            }
            if let Some(value) = main_tone {
                *tone = *value;
            }
        }
        camera.is_active = visible && vehicle == Some(mirror.vehicle) && refresh;
        if !camera.is_active {
            continue;
        }
        if let Some((_, _, car)) = cars
            .iter()
            .find(|(i, o, _)| i.0 == mirror.vehicle && o.track_index == 0)
        {
            *tf = Transform::from_translation(
                car.transform_point(Vec3::from_array(mirror.definition.position_m)),
            )
            .looking_to(
                car.rotation() * Vec3::from_array(mirror.definition.direction).normalize(),
                car.rotation() * Vec3::Y,
            );
            *global = GlobalTransform::from(*tf);
            state.rendered_frames += 1;
        } else {
            camera.is_active = false;
        }
    }
    for (panel, mut visibility) in &mut panels {
        *visibility = if visible && vehicle == Some(panel.vehicle) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
fn cleanup(
    mut commands: Commands,
    entities: Query<Entity, Or<(With<MirrorCamera>, With<MirrorPanel>)>>,
    cameras: Query<&MirrorCamera>,
    mut images: ResMut<Assets<Image>>,
) {
    for mirror in &cameras {
        images.remove(mirror.image.id());
    }
    for e in &entities {
        commands.entity(e).despawn();
    }
}

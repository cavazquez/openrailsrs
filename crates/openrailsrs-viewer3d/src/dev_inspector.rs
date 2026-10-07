//! Read-only runtime inspector. Native data is copied to a reflected diagnostic
//! view; no editor, mutable asset access, filesystem loading or entity deletion.
use bevy::prelude::*;
use bevy_inspector_egui::{
    bevy_egui::{EguiContext, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui},
    inspector_egui_impls::InspectorEguiImpl,
    reflect_inspector::ui_for_value_readonly,
};
use openrailsrs_formats::ShapeFile;
use std::path::Path;

#[derive(Component, Reflect, Clone, Debug, Default, serde::Serialize)]
#[reflect(Component)]
pub struct NativePrimitive {
    pub shape: String,
    pub prim_state: i32,
    pub prim_flags: i32,
    pub z_bias: f64,
    pub z_buf_mode: i32,
    pub alpha_test_mode: i32,
    pub shader: String,
    pub texture: String,
}
impl NativePrimitive {
    pub fn from_shape(shape: &ShapeFile, index: i32, path: &Path) -> Self {
        let ps = (index >= 0)
            .then(|| shape.prim_states.get(index as usize))
            .flatten();
        Self {
            shape: path.to_string_lossy().into_owned(),
            prim_state: index,
            prim_flags: ps.map_or(0, |p| p.flags),
            z_bias: ps.and_then(|p| p.z_bias).unwrap_or(0.),
            z_buf_mode: ps.map_or(-1, |p| p.z_buf_mode),
            alpha_test_mode: ps.map_or(-1, |p| p.alpha_test_mode),
            shader: openrailsrs_bevy_scenery::shapes::shader_name_for_prim_state(shape, index)
                .unwrap_or_default(),
            texture: openrailsrs_bevy_scenery::shapes::texture_for_prim_state(shape, index)
                .unwrap_or_default(),
        }
    }
}
#[derive(Resource, Default)]
pub struct InspectorState {
    pub enabled: bool,
    pub selected: Option<Entity>,
    pub snapshot: serde_json::Value,
    filter: String,
}
#[derive(Reflect, Debug, Default, serde::Serialize)]
pub struct PrimitiveView {
    pub entity: String,
    pub name: String,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub visibility: String,
    pub render_layers: String,
    pub lod: String,
    pub mesh: String,
    pub material: String,
    pub effective_texture: String,
    pub alpha_mode: String,
    pub effective_depth_bias: f32,
    pub shader_flags: f32,
    pub shader_kind: f32,
    pub imported: NativePrimitive,
}
pub fn view(world: &World, entity: Entity) -> PrimitiveView {
    let mut v = PrimitiveView {
        entity: format!("{entity:?}"),
        ..default()
    };
    if let Some(name) = world.get::<Name>(entity) {
        v.name = name.as_str().to_string();
    }
    if let Some(tf) = world.get::<Transform>(entity) {
        v.translation = tf.translation.to_array();
        v.rotation = tf.rotation.to_array();
        v.scale = tf.scale.to_array();
    }
    v.visibility = format!(
        "{:?}; inherited={:?}; in_view={:?}",
        world.get::<Visibility>(entity),
        world.get::<InheritedVisibility>(entity),
        world.get::<ViewVisibility>(entity)
    );
    v.render_layers = world
        .get::<bevy::camera::visibility::RenderLayers>(entity)
        .map_or_else(|| "[0] (default)".into(), |l| format!("{l:?}"));
    v.mesh = world
        .get::<Mesh3d>(entity)
        .map_or_else(|| "none".into(), |m| format!("{:?}", m.id()));
    if let Some(import) = world.get::<NativePrimitive>(entity) {
        v.imported = import.clone();
    }
    if let Some(lod) = world.get::<crate::world::WorldSceneryLod>(entity) {
        v.lod = format!(
            "band {} · part {} · enabled {}",
            lod.lod_idx, lod.part_index, lod.enabled
        );
        if let Some(shape) = world
            .resource::<crate::world::WorldShapeLodCache>()
            .shapes
            .get(&lod.shape_path)
        {
            v.imported = NativePrimitive::from_shape(shape, lod.prim_state_idx, &lod.shape_path);
        }
    }
    if let Some(group) = world.get::<crate::world_instancing::WorldInstancedGroup>(entity) {
        v.lod = format!(
            "band {} · part {} · {} GPU instances",
            group.lod_idx, group.part_index, group.instance_count
        );
        if let Some(shape) = world
            .resource::<crate::world::WorldShapeLodCache>()
            .shapes
            .get(&group.shape_path)
        {
            v.imported =
                NativePrimitive::from_shape(shape, group.prim_state_idx, &group.shape_path);
        }
    }
    if v.lod.is_empty() {
        v.lod = "fixed train/cab primitive".into();
    }
    if let Some(handle) = world.get::<MeshMaterial3d<StandardMaterial>>(entity) {
        if let Some(m) = world.resource::<Assets<StandardMaterial>>().get(&handle.0) {
            v.material = format!("StandardMaterial {:?}", handle.id());
            v.alpha_mode = format!("{:?}", m.alpha_mode);
            v.effective_depth_bias = m.depth_bias;
            v.effective_texture = m
                .base_color_texture
                .as_ref()
                .map_or_else(|| "none".into(), |h| format!("{:?}", h.id()));
        }
    } else if let Some(handle) =
        world.get::<MeshMaterial3d<crate::terrain_material::TerrainMaterial>>(entity)
        && let Some(m) = world
            .resource::<Assets<crate::terrain_material::TerrainMaterial>>()
            .get(&handle.0)
    {
        v.material = format!("TerrainMaterial {:?}", handle.id());
        v.alpha_mode = "Opaque".into();
        v.effective_texture = format!(
            "base {:?}; detail {:?}",
            m.base_texture.id(),
            m.overlay_texture.id()
        );
        v.lod = "native terrain patch/chunk".into();
    } else if let Some(handle) =
        world.get::<MeshMaterial3d<openrailsrs_bevy_scenery::OrForestMaterial>>(entity)
        && let Some(m) = world
            .resource::<Assets<openrailsrs_bevy_scenery::OrForestMaterial>>()
            .get(&handle.0)
    {
        v.material = format!("OrForestMaterial {:?}", handle.id());
        v.alpha_mode = format!("{:?}", m.alpha_mode);
        v.effective_texture = format!("{:?}", m.base_texture.id());
        v.lod = "native Forest billboards".into();
    } else if let Some(handle) =
        world.get::<MeshMaterial3d<crate::surface_weather::SnowMaterial>>(entity)
    {
        if let Some(m) = world
            .resource::<Assets<crate::surface_weather::SnowMaterial>>()
            .get(&handle.0)
        {
            v.material = format!("SnowMaterial {:?}", handle.id());
            v.alpha_mode = format!("{:?}", m.base.alpha_mode);
            v.effective_depth_bias = m.base.depth_bias;
            v.effective_texture = m
                .base
                .base_color_texture
                .as_ref()
                .map_or_else(|| "none".into(), |h| format!("{:?}", h.id()));
        }
    } else if let Some(handle) =
        world.get::<MeshMaterial3d<crate::or_cab_material::OrCabMaterial>>(entity)
    {
        if let Some(m) = world
            .resource::<Assets<crate::or_cab_material::OrCabMaterial>>()
            .get(&handle.0)
        {
            v.material = format!("OrCabMaterial {:?}", handle.id());
            v.alpha_mode = format!("{:?}", m.alpha_mode);
            v.effective_depth_bias = m.depth_bias;
            v.effective_texture = format!("{:?}", m.base_texture.id());
            v.shader_flags = m.params.flags;
            v.shader_kind = m.params.shader_kind;
        }
    } else if let Some(handle) =
        world.get::<MeshMaterial3d<openrailsrs_bevy_scenery::OrSceneryMaterial>>(entity)
        && let Some(m) = world
            .resource::<Assets<openrailsrs_bevy_scenery::OrSceneryMaterial>>()
            .get(&handle.0)
    {
        v.material = format!("OrSceneryMaterial {:?}", handle.id());
        v.alpha_mode = format!("{:?}", m.alpha_mode);
        v.effective_depth_bias = m.depth_bias();
        v.effective_texture = format!("{:?}", m.base_texture.id());
        v.shader_flags = m.params.flags;
        v.shader_kind = m.params.shader_kind;
    }
    if let Some(appearance) = world.get::<crate::world_instancing::WorldInstanceAppearance>(entity)
    {
        v.material = "WorldInstanceAppearance · GPU batching".into();
        v.effective_texture = appearance
            .base_color_texture
            .as_ref()
            .map_or_else(|| "fallback".into(), |h| format!("{:?}", h.id()));
        v.alpha_mode = if appearance.alpha_cutoff > 0. {
            format!("Mask({})", appearance.alpha_cutoff)
        } else {
            "Opaque".into()
        };
    }
    v
}
pub fn install(app: &mut App) {
    register_diagnostic_types(app);
    app.add_plugins(EguiPlugin::default())
        .insert_resource(bevy_inspector_egui::bevy_egui::EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .add_systems(
            PreUpdate,
            attach_context.before(bevy_inspector_egui::bevy_egui::EguiPreUpdateSet::InitContexts),
        )
        .init_resource::<InspectorState>()
        .insert_resource(bevy::picking::mesh_picking::MeshPickingSettings {
            require_markers: true,
            ..default()
        })
        .add_systems(Update, (keys, picking_targets).chain())
        .add_systems(
            Update,
            capture_pointer
                .after(crate::player_ui::UiPointerCaptureSet)
                .before(crate::camera::orbit_camera_system)
                .before(crate::camera::fly_camera_system),
        )
        .add_systems(EguiPrimaryContextPass, panel)
        .add_observer(select);
    app.world_mut().resource_mut::<InspectorState>().enabled =
        std::env::var("OPENRAILSRS_DEV_INSPECTOR").is_ok_and(|v| v == "1");
}
fn register_diagnostic_types(app: &mut App) {
    // Reflect registration alone does not supply readonly widgets for opaque
    // primitives such as String and f32. Keep the native snapshot inspectable
    // without installing a mutable world inspector.
    // The viewer uses a reduced Bevy plugin set. The inspector's general
    // configuration assumes unrelated types such as Instant and asset handles
    // are already registered. This adapter needs only these four leaf widgets.
    app.register_type::<NativePrimitive>()
        .register_type::<PrimitiveView>()
        .register_type::<String>()
        .register_type_data::<String, InspectorEguiImpl>()
        .register_type::<f32>()
        .register_type_data::<f32, InspectorEguiImpl>()
        .register_type::<f64>()
        .register_type_data::<f64, InspectorEguiImpl>()
        .register_type::<i32>()
        .register_type_data::<i32, InspectorEguiImpl>();
    let mut registry = app.world().resource::<AppTypeRegistry>().write();
    registry
        .get_mut(std::any::TypeId::of::<f32>())
        .unwrap()
        .insert(precise_number_widget::<f32>());
    registry
        .get_mut(std::any::TypeId::of::<f64>())
        .unwrap()
        .insert(precise_number_widget::<f64>());
}
fn precise_number_widget<T: std::any::Any + std::fmt::Debug>() -> InspectorEguiImpl {
    // The stock readonly number widget keeps one decimal. Small ZBias values
    // and quaternion components must remain visible with round-trip precision.
    InspectorEguiImpl::new(
        |value, ui, options, id, env| {
            precise_number_readonly::<T>(value, ui, options, id, env);
            false
        },
        precise_number_readonly::<T>,
        |_, _, _, _, _, _| false,
    )
}
fn precise_number_readonly<T: std::any::Any + std::fmt::Debug>(
    value: &dyn std::any::Any,
    ui: &mut egui::Ui,
    _: &dyn std::any::Any,
    _: egui::Id,
    _: bevy_inspector_egui::reflect_inspector::InspectorUi<'_, '_>,
) {
    ui.monospace(format!("{:?}", value.downcast_ref::<T>().unwrap()));
}
fn capture_pointer(
    state: Res<InspectorState>,
    mut pointer: ResMut<crate::player_ui::UiPointerCapture>,
) {
    pointer.0 |= state.enabled;
}
fn attach_context(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Without<PrimaryEguiContext>)>,
    contexts: Query<Entity, With<PrimaryEguiContext>>,
) {
    // The loading and HUD cameras must never own a second primary Egui pass.
    if contexts.is_empty()
        && let Some(camera) = cameras.iter().next()
    {
        commands.entity(camera).insert(PrimaryEguiContext);
    }
}
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    preferences: Res<crate::player_settings::PlayerSettings>,
    mut state: ResMut<InspectorState>,
) {
    if keys.just_pressed(KeyCode::F12) && !preferences.keys.values().any(|k| k == "F12") {
        state.enabled = !state.enabled;
    }
}
fn picking_targets(
    mut commands: Commands,
    state: Res<InspectorState>,
    #[cfg(feature = "dev-tools")] debug: Res<bevy::dev_tools::picking_debug::DebugPickingMode>,
    cameras: Query<(Entity, Has<bevy::picking::mesh_picking::MeshPickingCamera>), With<Camera3d>>,
    targets: Query<Entity, (With<Mesh3d>, Without<bevy::picking::Pickable>)>,
) {
    let enabled = state.enabled;
    #[cfg(feature = "dev-tools")]
    let enabled = enabled || *debug != bevy::dev_tools::picking_debug::DebugPickingMode::Disabled;
    for (camera, marked) in &cameras {
        if enabled && !marked {
            commands
                .entity(camera)
                .insert(bevy::picking::mesh_picking::MeshPickingCamera);
        } else if !enabled && marked {
            commands
                .entity(camera)
                .remove::<bevy::picking::mesh_picking::MeshPickingCamera>();
        }
    }
    if enabled {
        for target in &targets {
            commands
                .entity(target)
                .insert(bevy::picking::Pickable::default());
        }
    }
}
fn select(click: On<Pointer<Click>>, meshes: Query<&Mesh3d>, mut state: ResMut<InspectorState>) {
    if state.enabled && meshes.contains(click.entity) {
        state.selected = Some(click.entity);
    }
}
fn panel(world: &mut World) {
    if !world.resource::<InspectorState>().enabled {
        return;
    }
    let Ok(mut context) = world
        .query_filtered::<&mut EguiContext, With<PrimaryEguiContext>>()
        .single_mut(world)
    else {
        return;
    };
    let context = context.get_mut().clone();
    let filter = world
        .resource::<InspectorState>()
        .filter
        .to_ascii_lowercase();
    if world.resource::<InspectorState>().selected.is_none()
        && let Ok(prefix) = std::env::var("OPENRAILSRS_DEV_INSPECTOR_SELECT")
    {
        let selected = world
            .query_filtered::<(Entity, &Name), With<Mesh3d>>()
            .iter(world)
            .find(|(_, name)| name.as_str().starts_with(&prefix))
            .map(|(e, _)| e);
        world.resource_mut::<InspectorState>().selected = selected;
    }
    let selected = world.resource::<InspectorState>().selected;
    let snapshot = selected
        .filter(|e| world.entities().contains(*e))
        .map(|e| view(world, e));
    let mut list: Vec<_> = world
        .query_filtered::<(Entity, Option<&Name>), With<Mesh3d>>()
        .iter(world)
        .filter_map(|(e, n)| {
            let name = n.map_or_else(|| format!("{e:?}"), |n| n.as_str().to_string());
            name.to_ascii_lowercase()
                .contains(&filter)
                .then_some((e, name))
        })
        .take(500)
        .collect();
    list.sort_by(|a, b| a.1.cmp(&b.1));
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let mut state = world.resource_mut::<InspectorState>();
    state.snapshot = snapshot.as_ref().map_or(serde_json::Value::Null, |s| {
        serde_json::to_value(s).unwrap_or_default()
    });
    egui::Window::new("Primitiva · lectura")
        .default_width(640.)
        .min_width(600.)
        .default_pos(egui::pos2(265., 35.))
        .show(&context, |ui| {
            ui.label("F12: cerrar · seleccionar un mesh con el mouse o la lista");
            ui.text_edit_singleline(&mut state.filter);
            egui::ComboBox::from_label("Entidad")
                .selected_text(snapshot.as_ref().map_or("Seleccionar", |s| &s.name))
                .show_ui(ui, |ui| {
                    for (entity, name) in &list {
                        if ui
                            .selectable_label(Some(*entity) == state.selected, name)
                            .clicked()
                        {
                            state.selected = Some(*entity);
                        }
                    }
                });
            if let Some(snapshot) = snapshot.as_ref() {
                egui::ScrollArea::vertical()
                    .max_height(510.)
                    .show(ui, |ui| ui_for_value_readonly(snapshot, ui, &registry));
            }
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readonly_snapshot_has_widgets_for_opaque_native_values() {
        use std::any::TypeId;
        let mut app = App::new();
        register_diagnostic_types(&mut app);
        let registry = app.world().resource::<AppTypeRegistry>().read();
        for id in [
            TypeId::of::<String>(),
            TypeId::of::<f32>(),
            TypeId::of::<f64>(),
            TypeId::of::<i32>(),
        ] {
            assert!(
                registry
                    .get(id)
                    .unwrap()
                    .data::<InspectorEguiImpl>()
                    .is_some()
            );
        }
    }
    #[test]
    fn readonly_bias_widget_preserves_small_effective_values_on_screen() {
        let mut app = App::new();
        register_diagnostic_types(&mut app);
        let registry = app.world().resource::<AppTypeRegistry>().read();
        let context = egui::Context::default();
        let output = context.run_ui(default(), |ui| {
            ui_for_value_readonly(&0.00115_f32, ui, &registry);
        });
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape, egui::Shape::Text(text) if text.galley.text() == "0.00115"
        )));
    }
    #[test]
    fn native_state_and_effective_material_are_distinct_readonly_values() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>();
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                depth_bias: 0.012,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
        let e = app
            .world_mut()
            .spawn((
                Name::new("train:live:car:0:part:1"),
                MeshMaterial3d(material),
                NativePrimitive {
                    z_bias: 2.,
                    prim_flags: 4,
                    alpha_test_mode: 2,
                    ..default()
                },
            ))
            .id();
        let snapshot = view(app.world(), e);
        assert_eq!(snapshot.imported.z_bias, 2.);
        assert_eq!(snapshot.imported.prim_flags, 4);
        assert_eq!(snapshot.effective_depth_bias, 0.012);
        assert_eq!(snapshot.alpha_mode, "Blend");
        assert_eq!(app.world().get::<NativePrimitive>(e).unwrap().z_bias, 2.);
    }
}

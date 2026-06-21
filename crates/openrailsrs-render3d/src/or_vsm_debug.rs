//! Depuracion VSM/sombras OR: toggle de modo, tint de cascadas, preview del atlas.

use bevy::prelude::*;
use bevy::ui::widget::ImageNode;
use openrailsrs_bevy_scenery::materials::OrSceneryMaterial;
use openrailsrs_bevy_scenery::vsm::{
    OR_VSM_ATLAS_LAYERS, OrMomentPreviewImage, OrVsmMode, OrVsmRenderSettings,
};

use crate::debug_hud::{DebugHudEnabled, DebugHudRoot, SceneDebugContext, scene_to_msts};
use crate::loading::AppState;
use crate::or_vsm_moments::{OrMomentMaps, OrVsmCascadeLimits};

pub use openrailsrs_bevy_scenery::vsm::OrVsmDebugSettings;

#[derive(Component)]
pub struct OrVsmPreviewPanel;

pub fn spawn_or_vsm_preview_ui(mut commands: Commands, preview: Res<OrMomentPreviewImage>) {
    commands.spawn((
        OrVsmPreviewPanel,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            right: Val::Px(8.0),
            width: Val::Px(320.0),
            height: Val::Px(320.0),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
        BorderColor::all(Color::srgba(0.9, 0.75, 0.2, 0.9)),
        ImageNode::new(preview.0.clone()),
        ZIndex(199),
        Visibility::Hidden,
    ));
}

pub fn or_vsm_debug_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<OrVsmDebugSettings>,
) {
    if keys.just_pressed(KeyCode::F4) {
        settings.mode = settings.mode.next();
        info!("VSM modo: {}", settings.mode.label());
    }
    if keys.just_pressed(KeyCode::F5) {
        settings.cascade_tint = !settings.cascade_tint;
        info!(
            "VSM tint cascada: {}",
            if settings.cascade_tint { "on" } else { "off" }
        );
    }
    if keys.just_pressed(KeyCode::F6) {
        settings.atlas_preview = !settings.atlas_preview;
        info!(
            "VSM atlas preview: {}",
            if settings.atlas_preview { "on" } else { "off" }
        );
    }
    if keys.just_pressed(KeyCode::F7) {
        settings.atlas_layer =
            (settings.atlas_layer + OR_VSM_ATLAS_LAYERS - 1) % OR_VSM_ATLAS_LAYERS;
        info!("VSM atlas capa: {}", settings.atlas_layer);
    }
    if keys.just_pressed(KeyCode::F8) {
        settings.atlas_layer = (settings.atlas_layer + 1) % OR_VSM_ATLAS_LAYERS;
        info!("VSM atlas capa: {}", settings.atlas_layer);
    }
}

pub fn or_vsm_debug_preset(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<OrVsmDebugSettings>,
    mut hud_enabled: ResMut<DebugHudEnabled>,
    mut hud_roots: Query<&mut Visibility, With<DebugHudRoot>>,
    ctx: Res<SceneDebugContext>,
    limits: Res<OrVsmCascadeLimits>,
    cam: Query<&Transform, With<Camera3d>>,
) {
    if !keys.just_pressed(KeyCode::F9) {
        return;
    }
    settings.apply_debug_preset();
    hud_enabled.0 = true;
    for mut vis in &mut hud_roots {
        *vis = Visibility::Visible;
    }

    if let Ok(tf) = cam.single() {
        let pos = tf.translation;
        let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
        let (tile_x, tile_z, wx, wy, wz) = scene_to_msts(pos, ctx.center_tile);
        info!(
            "VSM debug preset (F9): exact + tint + atlas capa 1/4 + HUD ON\n             cam escena ({:+.1}, {:+.1}, {:+.1})  yaw {:+.0} deg  pitch {:+.0} deg\n             MSTS world X {:+.1}  Y {:+.1}  Z {:+.1}  tile ({tile_x}, {tile_z})\n             ShadowMapLimit [{:.0}, {:.0}, {:.0}, {:.0}]",
            pos.x,
            pos.y,
            pos.z,
            yaw.to_degrees(),
            pitch.to_degrees(),
            wx,
            wy,
            wz,
            limits.limits[0],
            limits.limits[1],
            limits.limits[2],
            limits.limits[3],
        );
    } else {
        info!("VSM debug preset (F9): exact + tint + atlas capa 1/4 + HUD ON");
    }
}

pub fn sync_or_vsm_preview_visibility(
    settings: Res<OrVsmDebugSettings>,
    mut panels: Query<&mut Visibility, With<OrVsmPreviewPanel>>,
) {
    let show = settings.atlas_preview && settings.mode == OrVsmMode::Exact;
    for mut vis in &mut panels {
        *vis = if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

pub fn sync_or_vsm_debug_to_materials(
    settings: Res<OrVsmDebugSettings>,
    mut materials: ResMut<Assets<OrSceneryMaterial>>,
) {
    let mode_gpu = settings.mode.as_gpu();
    let flags = settings.debug_flags();
    for (_, mat) in materials.iter_mut() {
        mat.params.vsm_mode = mode_gpu;
        mat.params.debug_flags = flags;
    }
}

pub fn sync_or_vsm_debug_camera(
    settings: Res<OrVsmDebugSettings>,
    mut commands: Commands,
    cameras: Query<Entity, With<Camera3d>>,
) {
    let enabled = settings.mode == OrVsmMode::Exact;
    for entity in &cameras {
        if enabled {
            commands
                .entity(entity)
                .insert(OrVsmRenderSettings { enabled: true });
        } else {
            commands.entity(entity).remove::<OrVsmRenderSettings>();
        }
    }
}

pub struct OrVsmDebugPlugin;

impl Plugin for OrVsmDebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrVsmDebugSettings>()
            .add_systems(PostStartup, spawn_or_vsm_preview_ui)
            .add_systems(
                Update,
                (
                    or_vsm_debug_input,
                    or_vsm_debug_preset,
                    sync_or_vsm_preview_visibility,
                    sync_or_vsm_debug_to_materials,
                    sync_or_vsm_debug_camera,
                )
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

pub fn vsm_debug_hud_lines(
    settings: &OrVsmDebugSettings,
    limits: &OrVsmCascadeLimits,
    moments: &OrMomentMaps,
) -> Vec<String> {
    vec![
        format!("VSM {} (F4 ciclar)", settings.mode.label()),
        format!(
            "cascada tint {} | atlas {} capa {}/{} (F5-F8, F9 preset)",
            if settings.cascade_tint { "ON" } else { "off" },
            if settings.atlas_preview { "ON" } else { "off" },
            settings.atlas_layer + 1,
            OR_VSM_ATLAS_LAYERS,
        ),
        format!(
            "ShadowMapLimit [{:.0}, {:.0}, {:.0}, {:.0}]",
            limits.limits[0], limits.limits[1], limits.limits[2], limits.limits[3],
        ),
        format!(
            "moment atlas {}x{} x{} {}",
            moments.resolution,
            moments.resolution,
            moments.cascades,
            if moments.ready { "listo" } else { "off" },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_bevy_scenery::vsm::OR_DEBUG_CASCADE_TINT;

    #[test]
    fn debug_preset_sets_exact_and_overlays() {
        let mut s = OrVsmDebugSettings {
            mode: OrVsmMode::PcfOr,
            cascade_tint: false,
            atlas_preview: false,
            atlas_layer: 2,
        };
        s.apply_debug_preset();
        assert_eq!(s.mode, OrVsmMode::Exact);
        assert!(s.cascade_tint);
        assert!(s.atlas_preview);
        assert_eq!(s.atlas_layer, 0);
        assert_eq!(s.debug_flags(), OR_DEBUG_CASCADE_TINT);
    }
}

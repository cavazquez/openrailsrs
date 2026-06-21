//! HUD de depuracion: posicion, tile MSTS, orientacion, FPS.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use openrailsrs_formats::{msts_tile_x_index_for_coord, msts_tile_z_index_for_coord};

use crate::or_vsm_debug::{OrVsmDebugSettings, vsm_debug_hud_lines};
use crate::or_vsm_moments::{OrMomentMaps, OrVsmCascadeLimits};
use crate::textures::TextureEnvironment;
use crate::track::TILE_SIZE_M;

/// Cámara fly del jugador (evita confundirla con otras `Camera3d`).
#[derive(Component)]
pub struct FlyCamera;

/// Marcador del bloque de texto del HUD.
#[derive(Component)]
pub struct DebugHudText;

/// Contexto de escena para convertir coords locales -> MSTS.
#[derive(Resource, Clone)]
pub struct SceneDebugContext {
    pub center_tile: (i32, i32),
    pub radius: u32,
    pub tile_count: usize,
    pub object_count: usize,
}

#[derive(Resource)]
pub struct DebugHudEnabled(pub bool);

#[derive(Component)]
pub struct DebugHudRoot;

/// Convierte posicion de escena (tile central en origen) a coords MSTS/OR.
pub fn scene_to_msts(scene: Vec3, center_tile: (i32, i32)) -> (i32, i32, f32, f32, f32) {
    let (cx, cz) = center_tile;
    let world_x = scene.x + cx as f32 * TILE_SIZE_M;
    let bevy_z = scene.z - cz as f32 * TILE_SIZE_M;
    let tile_x = msts_tile_x_index_for_coord(world_x);
    let tile_z = msts_tile_z_index_for_coord(bevy_z);
    let msts_z = -bevy_z;
    (tile_x, tile_z, world_x, scene.y, msts_z)
}

/// Cámara 2D transparente encima del mundo 3D para dibujar la UI.
pub fn spawn_ui_overlay_camera(commands: &mut Commands) {
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        IsDefaultUiCamera,
        Name::new("ui_overlay_camera"),
    ));
}

pub fn spawn_debug_hud(commands: &mut Commands, enabled: bool) {
    let vis = if enabled {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    commands.spawn((
        DebugHudRoot,
        DebugHudText,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            min_width: Val::Px(420.0),
            max_width: Val::Px(520.0),
            padding: UiRect::all(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.04, 0.06, 0.09, 0.92)),
        BorderColor::all(Color::srgba(0.35, 0.55, 0.75, 0.85)),
        ZIndex(1000),
        vis,
        Text::new("openrailsrs-render3d\nHUD cargando…"),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.92, 0.96, 1.0)),
    ));
}

pub fn toggle_debug_hud(
    keys: Res<ButtonInput<KeyCode>>,
    mut enabled: ResMut<DebugHudEnabled>,
    mut roots: Query<&mut Visibility, With<DebugHudRoot>>,
) {
    if !keys.just_pressed(KeyCode::F3) {
        return;
    }
    enabled.0 = !enabled.0;
    for mut vis in &mut roots {
        *vis = if enabled.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_debug_hud(
    enabled: Res<DebugHudEnabled>,
    ctx: Res<SceneDebugContext>,
    route: Res<crate::RouteDir>,
    speed: Res<FlySpeed>,
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    texture_env: Res<TextureEnvironment>,
    vsm_debug: Res<OrVsmDebugSettings>,
    vsm_limits: Res<OrVsmCascadeLimits>,
    vsm_moments: Res<OrMomentMaps>,
    fly_cam: Query<&Transform, With<FlyCamera>>,
    mut hud_text: Query<&mut Text, With<DebugHudText>>,
) {
    if !enabled.0 {
        return;
    }
    let Ok(mut text) = hud_text.single_mut() else {
        return;
    };
    let Ok(tf) = fly_cam.single() else {
        return;
    };

    *text = Text::new(hud_body(
        tf,
        &ctx,
        &route,
        speed.0,
        &time,
        &diagnostics,
        &texture_env,
        &vsm_debug,
        &vsm_limits,
        &vsm_moments,
    ));
}

/// Actualiza el título de la ventana con tile y coords MSTS en vivo.
pub fn update_window_title(
    ctx: Res<SceneDebugContext>,
    route: Res<crate::RouteDir>,
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    fly_cam: Query<&Transform, With<FlyCamera>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let Ok(tf) = fly_cam.single() else {
        return;
    };
    let (cx, cz) = ctx.center_tile;
    let (tile_x, tile_z, wx, wy, wz) = scene_to_msts(tf.translation, ctx.center_tile);
    let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
    let yaw_deg = yaw.to_degrees().rem_euclid(360.0);
    let pitch_deg = pitch.to_degrees();
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or_else(|| 1.0 / time.delta_secs().max(1e-6) as f64);
    let route_name = route.0.file_name().and_then(|s| s.to_str()).unwrap_or("?");
    window.title = format!(
        "OR3D {route_name} | tile ({cx},{cz}) r={} | MSTS ({tile_x},{tile_z}) X {wx:.0} Y {wy:.0} Z {wz:.0} | yaw {yaw_deg:.0}° pitch {pitch_deg:.0}° | {fps:.0} fps",
        ctx.radius
    );
}

#[allow(clippy::too_many_arguments)]
fn hud_body(
    tf: &Transform,
    ctx: &SceneDebugContext,
    route: &crate::RouteDir,
    fly_speed: f32,
    time: &Time,
    diagnostics: &DiagnosticsStore,
    texture_env: &TextureEnvironment,
    vsm_debug: &OrVsmDebugSettings,
    vsm_limits: &OrVsmCascadeLimits,
    vsm_moments: &OrMomentMaps,
) -> String {
    let pos = tf.translation;
    let (tile_x, tile_z, wx, wy, wz) = scene_to_msts(pos, ctx.center_tile);
    let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
    let yaw_deg = yaw.to_degrees().rem_euclid(360.0);
    let pitch_deg = pitch.to_degrees();

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or_else(|| 1.0 / time.delta_secs().max(1e-6) as f64);

    let route_name = route.0.file_name().and_then(|s| s.to_str()).unwrap_or("?");

    let (cx, cz) = ctx.center_tile;
    let on_center = tile_x == cx && tile_z == cz;

    let mut lines = vec![
        format!("openrailsrs-render3d  |  {route_name}"),
        format!("FPS {:.0}  |  fly {:.0} m/s", fps, fly_speed as f64),
        format!(
            "cam escena  X {:+.1}  Y {:+.1}  Z {:+.1}",
            pos.x, pos.y, pos.z
        ),
        format!("MSTS world  X {:+.1}  Y {:+.1}  Z {:+.1}", wx, wy, wz),
        format!(
            "tile ({tile_x}, {tile_z}){}  centro ({cx}, {cz}) r={}",
            if on_center { " *" } else { "" },
            ctx.radius
        ),
        format!(
            "yaw {:+.0}°  pitch {:+.0}°  |  WASD Q/E  RMB mirar  Shift rápido",
            yaw_deg, pitch_deg
        ),
        format!(
            "tiles {}  objs {}  |  vis {} / {}{}",
            ctx.tile_count,
            ctx.object_count,
            texture_env.season.label(),
            if texture_env.night { "noche" } else { "dia" },
            if texture_env.snow_weather {
                " / snow"
            } else {
                ""
            },
        ),
    ];

    lines.extend(vsm_debug_hud_lines(vsm_debug, vsm_limits, vsm_moments));
    lines.push("F3 ocultar HUD | F4 VSM | Esc salir".to_string());
    lines.join("\n")
}

/// Velocidad fly (resource en `main`).
#[derive(Resource)]
pub struct FlySpeed(pub f32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_origin_maps_to_center_tile() {
        let (tx, tz, wx, _, _) = scene_to_msts(Vec3::ZERO, (-6131, 14898));
        assert_eq!(tx, -6131);
        assert_eq!(tz, 14898);
        assert!((wx - (-6131.0 * 2048.0)).abs() < 1.0);
    }
}

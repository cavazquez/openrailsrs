//! Shared resources and fly camera for render3d.

use std::path::PathBuf;

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::objects;
use crate::terrain::TileGeometry;
use crate::track;

/// Información de un tile (geometría + offset en el espacio world).
#[derive(Clone)]
pub struct TileEntry {
    pub geometry: TileGeometry,
    pub world_offset: Vec3,
    pub track: track::TrackRibbon,
    pub objects: Vec<objects::ObjectMarker>,
}

#[derive(Resource)]
pub struct TilesToRender(pub Vec<TileEntry>);

#[derive(Resource)]
pub struct RouteDir(pub PathBuf);

#[derive(Resource)]
pub struct MstsRootDir(pub PathBuf);

#[derive(Resource, Clone)]
pub struct TdbTrackResource {
    pub ctx: crate::tdb_track::TdbContext,
    pub grid_radius: u32,
}

#[derive(Resource)]
pub struct SceneExtent {
    pub side_m: f32,
}

pub fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    speed: Res<crate::debug_hud::FlySpeed>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
) {
    let Ok(mut tf) = cam.single_mut() else {
        return;
    };

    if buttons.pressed(MouseButton::Right) {
        let mut delta = Vec2::ZERO;
        for ev in motion.read() {
            delta += ev.delta;
        }
        if delta != Vec2::ZERO {
            let sens = 0.003;
            let (mut yaw, mut pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
            yaw -= delta.x * sens;
            pitch = (pitch - delta.y * sens).clamp(-1.54, 1.54);
            tf.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        }
    } else {
        motion.clear();
    }

    let mut dir = Vec3::ZERO;
    let fwd = *tf.forward();
    let right = *tf.right();
    if keys.pressed(KeyCode::KeyW) {
        dir += fwd;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= fwd;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        dir -= Vec3::Y;
    }
    if dir != Vec3::ZERO {
        let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            4.0
        } else {
            1.0
        };
        tf.translation += dir.normalize() * speed.0 * boost * time.delta_secs();
    }
}

pub fn quit_on_esc(
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
    _windows: Query<&Window, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

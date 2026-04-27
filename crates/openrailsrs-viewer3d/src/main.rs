//! Binary entry point for the experimental 3D viewer.
//!
//! Controls:
//!
//! - `F1`         — orbit camera (default).
//! - `F2`         — fly camera.
//! - Right mouse  — orbit: rotate / fly: mouse-look (hold; cursor oculto y confinado a la ventana).
//! - Middle mouse — orbit: pan focus.
//! - Mouse wheel  — orbit: zoom.
//! - `WASD`       — fly: move on the camera horizontal plane.
//! - `Q` / `E`    — fly: move down / up (world Y).
//! - `Space`      — fly: move up (alias for `E`).
//! - `Shift`      — fly: x4 boost. `Ctrl` — x0.25 slow.
//! - `Esc`        — quit.

use bevy::prelude::*;
use openrailsrs_viewer3d::ViewerPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "openrailsrs-viewer3d".to_string(),
                resolution: (1280u32, 720u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(ViewerPlugin)
        .add_systems(Update, exit_on_esc)
        .run();
}

fn exit_on_esc(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}

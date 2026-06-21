//! Shared Bevy test helpers for scenery crates.

#![cfg(test)]

use crate::OrSceneryPlugins;
use bevy::asset::AssetPlugin;
use bevy::prelude::*;

pub fn minimal_scenery_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(crate::shared_asset_plugin())
        .add_plugins(OrSceneryPlugins);
    app
}

pub fn minimal_scenery_app_with_assets() -> App {
    let mut app = minimal_scenery_app();
    app.add_plugins(AssetPlugin::default());
    app
}

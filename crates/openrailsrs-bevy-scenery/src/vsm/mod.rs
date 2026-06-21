//! VSM shadow modes (Open Rails parity).

mod cascade;
pub mod debug_settings;
mod mode;
#[cfg(feature = "vsm")]
pub mod render;

pub use cascade::*;
pub use debug_settings::*;
pub use mode::*;
#[cfg(feature = "vsm")]
pub use render::{
    OrMomentAtlasImage, OrMomentBlurTempImage, OrMomentPreviewImage, OrVsmRenderPlugin,
    OrVsmRenderSettings,
};

use bevy::prelude::*;

/// Registers VSM moment render pass (feature `vsm`).
pub struct OrVsmPlugins;

impl Plugin for OrVsmPlugins {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "vsm")]
        app.add_plugins(OrVsmRenderPlugin);
    }
}

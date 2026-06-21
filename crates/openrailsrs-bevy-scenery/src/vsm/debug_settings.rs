//! VSM debug settings shared with the moment render pass.

use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResource;

use super::OrVsmMode;

pub const OR_DEBUG_CASCADE_TINT: f32 = 1.0;
pub const OR_VSM_ATLAS_LAYERS: u32 = 4;

/// Runtime VSM debug state (F4–F9 in render3d).
#[derive(Resource, Clone, Debug, ExtractResource)]
pub struct OrVsmDebugSettings {
    pub mode: OrVsmMode,
    pub cascade_tint: bool,
    pub atlas_preview: bool,
    pub atlas_layer: u32,
}

impl Default for OrVsmDebugSettings {
    fn default() -> Self {
        Self {
            mode: OrVsmMode::from_env(),
            cascade_tint: false,
            atlas_preview: false,
            atlas_layer: 0,
        }
    }
}

impl OrVsmDebugSettings {
    pub fn apply_debug_preset(&mut self) {
        self.mode = OrVsmMode::Exact;
        self.cascade_tint = true;
        self.atlas_preview = true;
        self.atlas_layer = 0;
    }

    pub fn debug_flags(&self) -> f32 {
        if self.cascade_tint {
            OR_DEBUG_CASCADE_TINT
        } else {
            0.0
        }
    }
}

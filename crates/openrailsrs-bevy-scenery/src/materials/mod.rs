//! Custom OR WGSL materials (terrain, scenery, cab, forest).

pub mod cab;
pub mod forest;
pub mod or_terrain;
pub mod scenery;
pub mod terrain;
pub mod terrain_flags;

pub use cab::*;
pub use forest::*;
pub use or_terrain::*;
pub use scenery::*;
pub use terrain::*;
pub use terrain_flags::*;

/// At most 64 paired wheel segments shared by snow receivers; no growing GPU buffer.
#[derive(Clone, Debug, PartialEq, bevy::render::render_resource::ShaderType)]
pub struct SnowTrackData {
    pub meta: bevy::prelude::Vec4,
    pub bounds: bevy::prelude::Vec4,
    pub segments: [bevy::prelude::Vec4; 128],
}
impl Default for SnowTrackData {
    fn default() -> Self {
        Self {
            meta: bevy::prelude::Vec4::ZERO,
            bounds: bevy::prelude::Vec4::ZERO,
            segments: [bevy::prelude::Vec4::ZERO; 128],
        }
    }
}

//! Canonical terrain material pipeline flags (#121).
//!
//! Both [`super::TerrainMaterial`] (viewer) and [`super::OrTerrainMaterial`]
//! (render3d) sample the same base + microtex slots. Differences are gated by
//! these flags — same textures/holes/UV scale should match except where a flag
//! intentionally diverges (notably VSM / night / overlay blend path).
//!
//! # Shader status
//! Shared half-Lambert lives in `assets/shaders/terrain_common.wgsl`. Full
//! fragment unification is deferred: viewer uses alpha-weighted microtex mix +
//! fog (#39); render3d uses OR `overlay*2` multiply + `lit`/`night` uniforms +
//! VSM-compatible lighting. See TODOs in those WGSL files.

/// Explicit pipeline feature bits for terrain materials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainPipelineFlags {
    /// Apply directional half-Lambert (+ cascade shadow when enabled).
    pub lit: bool,
    /// Apply night color modifier (OR `PSTerrain` night path).
    pub night: bool,
    /// App uses OR VSM / custom shadow path (render3d). Documented only — does
    /// not change bind-group layout; shadow sampling still goes through Bevy
    /// directional cascades when the light flag is set.
    pub vsm: bool,
    /// Apply Bevy distance fog in the fragment shader (`DISTANCE_FOG`).
    pub fog: bool,
}

impl TerrainPipelineFlags {
    /// viewer3d `TerrainMaterial`: always lit, fog on, no night/VSM uniforms.
    pub const VIEWER: Self = Self {
        lit: true,
        night: false,
        vsm: false,
        fog: true,
    };

    /// render3d `OrTerrainMaterial` daytime lit path (VSM stack active in-app).
    pub const RENDER3D_LIT: Self = Self {
        lit: true,
        night: false,
        vsm: true,
        fog: true,
    };

    /// Compact suffix for material cache keys (`terrain_material_cache_key`).
    pub fn cache_suffix(self) -> String {
        format!(
            "lit={}|night={}|vsm={}|fog={}",
            self.lit, self.night, self.vsm, self.fog
        )
    }
}

impl Default for TerrainPipelineFlags {
    fn default() -> Self {
        Self::VIEWER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_and_render3d_flags_differ_on_vsm() {
        assert!(!TerrainPipelineFlags::VIEWER.vsm);
        assert!(TerrainPipelineFlags::RENDER3D_LIT.vsm);
        assert_eq!(
            TerrainPipelineFlags::VIEWER.cache_suffix(),
            "lit=true|night=false|vsm=false|fog=true"
        );
    }
}

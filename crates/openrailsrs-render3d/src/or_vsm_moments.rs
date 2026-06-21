//! Runtime VSM estilo OR: momentos (z, z²) y límites de cascada para el shader de escenario.

use bevy::light::CascadeShadowConfig;
use bevy::prelude::*;

use crate::loading::AppState;
use crate::or_cascade::{
    OR_SHADOW_CASCADE_COUNT, cascade_shadow_config_from_or_limits, or_limits_from_view_distance,
    or_max_shadow_view_distance,
};
use crate::or_scenery_material::OrSceneryMaterial;
use crate::or_vsm::OrVsmMode;
use openrailsrs_bevy_scenery::vsm::OrVsmDebugSettings;

/// Límites de distancia de cascada OR (`ShadowMapLimit` en SM3).
#[derive(Resource, Clone, Debug)]
pub struct OrVsmCascadeLimits {
    pub limits: [f32; 4],
}

impl Default for OrVsmCascadeLimits {
    fn default() -> Self {
        Self {
            limits: or_limits_from_view_distance(2048.0),
        }
    }
}

impl OrVsmCascadeLimits {
    pub fn from_view_distance(view_distance_m: f32) -> Self {
        Self {
            limits: or_limits_from_view_distance(view_distance_m),
        }
    }
}

/// Texturas de momentos empaquetados (Rg32-equivalente en GPU).
#[derive(Resource, Clone, Debug, Default)]
pub struct OrMomentMaps {
    pub cascades: usize,
    pub resolution: u32,
    pub ready: bool,
}

/// Sincroniza estado VSM al entrar en juego o cambiar modo debug.
pub fn sync_or_vsm_runtime(
    mut moments: ResMut<OrMomentMaps>,
    mut limits: ResMut<OrVsmCascadeLimits>,
    settings: Res<OrVsmDebugSettings>,
    extent: Option<Res<crate::SceneExtent>>,
) {
    let mode = settings.mode;
    let side = extent.as_ref().map(|e| e.side_m).unwrap_or(2048.0);
    *limits = OrVsmCascadeLimits::from_view_distance(or_max_shadow_view_distance(side));

    match mode {
        OrVsmMode::Exact => {
            moments.cascades = OR_SHADOW_CASCADE_COUNT;
            moments.resolution = 2048;
            moments.ready = true;
        }
        OrVsmMode::Approx => {
            moments.cascades = 1;
            moments.resolution = 2048;
            moments.ready = true;
        }
        OrVsmMode::PcfOr => {
            *moments = OrMomentMaps::default();
        }
    }
}

/// Propaga `ShadowMapLimit` OR a todos los materiales de escenario.
pub fn sync_or_material_cascade_limits(
    limits: Res<OrVsmCascadeLimits>,
    mut materials: ResMut<Assets<OrSceneryMaterial>>,
) {
    for (_, mat) in materials.iter_mut() {
        mat.params.shadow_map_limit_x = limits.limits[0];
        mat.params.shadow_map_limit_y = limits.limits[1];
        mat.params.shadow_map_limit_z = limits.limits[2];
        mat.params.shadow_map_limit_w = limits.limits[3];
    }
}

/// Aplica límites OR a la config de cascadas del sol (Bevy).
pub fn sync_or_cascade_shadow_config(
    limits: Res<OrVsmCascadeLimits>,
    mut lights: Query<&mut CascadeShadowConfig, With<DirectionalLight>>,
) {
    let config = cascade_shadow_config_from_or_limits(limits.limits, 0.5, 0.2);
    for mut cascade in &mut lights {
        *cascade = config.clone();
    }
}

pub struct OrVsmPlugin;

impl Plugin for OrVsmPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrVsmCascadeLimits>()
            .init_resource::<OrMomentMaps>()
            .add_systems(
                Update,
                (
                    sync_or_vsm_runtime,
                    sync_or_cascade_shadow_config.after(sync_or_vsm_runtime),
                    sync_or_material_cascade_limits.after(sync_or_vsm_runtime),
                )
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_limits_scale_with_view() {
        let l = OrVsmCascadeLimits::from_view_distance(1000.0);
        assert!(l.limits[3] >= l.limits[0]);
    }
}

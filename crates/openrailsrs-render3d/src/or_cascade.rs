//! Cascadas de sombra estilo Open Rails (`ShadowMapLimit` + 4 splits log/uniforme).

use bevy::light::CascadeShadowConfig;

pub const OR_SHADOW_CASCADE_COUNT: usize = 4;

/// Distancia máxima de sombra OR a partir del lado del tile (como `ViewingDistance` en OR).
pub fn or_max_shadow_view_distance(side_m: f32) -> f32 {
    (side_m * 1.5).clamp(120.0, 2500.0)
}

/// Near plane OR para el cálculo de splits (`RenderProcess.InitializeShadowMapLocations`).
pub const OR_SHADOW_NEAR: f32 = 0.5;

/// Calcula `ShadowMapLimit` OR (distancias acumuladas por cascada).
///
/// Réplica de `RenderProcess.cs`: mezcla logarítmica y uniforme
/// `C = (3 * Clog + Cuniform) / 4` con `i = shadowMapIndex + 1`.
pub fn compute_or_shadow_map_limits(near: f32, far: f32, num_cascades: usize) -> [f32; 4] {
    let m = num_cascades.max(1) as f32;
    let far = far.max(near + 1.0);
    let mut limits = [f32::MAX; 4];
    for (idx, slot) in limits.iter_mut().enumerate().take(num_cascades.min(4)) {
        let i = (idx + 1) as f32;
        let clog = near * (far / near).powf(i / m);
        let cuniform = near + (far - near) * i / m;
        *slot = (3.0 * clog + cuniform) / 4.0;
    }
    limits
}

/// Configura cascadas Bevy con los límites OR.
pub fn cascade_shadow_config_from_or_limits(
    limits: [f32; 4],
    minimum_distance: f32,
    overlap_proportion: f32,
) -> CascadeShadowConfig {
    CascadeShadowConfig {
        bounds: limits[..OR_SHADOW_CASCADE_COUNT.min(limits.len())].to_vec(),
        overlap_proportion,
        minimum_distance,
    }
}

/// Límites OR a partir de la distancia de vista del tile (como OR `ViewingDistance`).
pub fn or_limits_from_view_distance(view_distance_m: f32) -> [f32; 4] {
    let far = view_distance_m.max(256.0);
    compute_or_shadow_map_limits(OR_SHADOW_NEAR, far, OR_SHADOW_CASCADE_COUNT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_monotonic() {
        let l = compute_or_shadow_map_limits(0.5, 2000.0, 4);
        assert!(l[0] < l[1]);
        assert!(l[1] < l[2]);
        assert!(l[2] < l[3]);
        assert!(l[3] <= 2000.0);
    }

    #[test]
    fn matches_or_first_split_formula() {
        let near = 0.5_f32;
        let far = 1000.0_f32;
        let m = 4.0_f32;
        let i = 1.0_f32;
        let expected = (3.0 * near * (far / near).powf(i / m) + near + (far - near) * i / m) / 4.0;
        let limits = compute_or_shadow_map_limits(near, far, 4);
        assert!((limits[0] - expected).abs() < 0.001);
    }
}

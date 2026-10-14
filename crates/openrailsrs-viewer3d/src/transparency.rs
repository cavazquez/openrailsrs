//! Preserve original alpha modes; Bevy sorts overlapping blended fragments.
//! OIT is bounded to 64 MiB of fragment storage and remains optional.
use bevy::core_pipeline::oit::OrderIndependentTransparencySettings;
use bevy::prelude::*;
pub fn update(
    mut commands: Commands,
    settings: Res<crate::player_settings::PlayerSettings>,
    memory: Res<crate::gpu_memory::ResidencyPolicy>,
    cameras: Query<
        (
            Entity,
            &Camera,
            Option<&OrderIndependentTransparencySettings>,
        ),
        (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
    >,
) {
    for (entity, camera, current) in &cameras {
        let pixels = camera
            .physical_target_size()
            .map(|s| u64::from(s.x) * u64::from(s.y));
        let layers = pixels
            .map(|n| (64. * 1024. * 1024. / (n.max(1) as f64 * 12.)).min(4.) as f32)
            .unwrap_or(0.);
        let enabled = settings.improved_transparency && !memory.pressured && layers >= 2.;
        if enabled && current.is_none_or(|s| (s.fragments_per_pixel_average - layers).abs() > 0.05)
        {
            commands
                .entity(entity)
                .insert(OrderIndependentTransparencySettings {
                    sorted_fragment_max_count: 8,
                    fragments_per_pixel_average: layers,
                    alpha_threshold: 0.001,
                });
        } else if !enabled && current.is_some() {
            commands
                .entity(entity)
                .remove::<OrderIndependentTransparencySettings>();
        }
    }
}

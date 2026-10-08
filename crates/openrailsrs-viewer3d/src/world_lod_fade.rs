//! Bounded LOD crossfades using Bevy's native complementary visibility dithering.
//! Both meshes share textures/materials, and the outgoing part lives at most .35s.
use bevy::{camera::visibility::VisibilityRange, prelude::*};

pub const DURATION_S: f32 = 0.35;
pub const MAX_ACTIVE: usize = 64;
const DISTANCE_STEP_M: f32 = 8.0;
pub const MAX_DISTANCE_M: f32 = 8192.0;
const FADE_STEPS: usize = 24;
const MARGIN_M: f32 = 256.0;

/// Initial/offscreen selection needs no duplicate mesh. Very distant changes
/// are immediate so the finite range palette also bounds Bevy's lifetime cache.
pub fn should_fade(initializing: bool, visible: bool, animated: bool, distance: f32) -> bool {
    !initializing && visible && !animated && distance.is_finite() && distance <= MAX_DISTANCE_M
}

#[derive(Component)]
pub struct LodFade {
    pub outgoing: Entity,
    pub elapsed_s: f32,
}

/// Same range on outgoing.end and incoming.start yields complementary pixels.
pub fn ranges(distance: f32, fraction: f32) -> (VisibilityRange, VisibilityRange) {
    // Bevy 0.19 retains each distinct range for the lifetime of the renderer,
    // with a u16 index. Per-frame floats would exhaust that table during a trip.
    // This palette has at most 51,250 fade ranges; rounding changes opacity by
    // less than 4%, while the two meshes still use complementary dithering.
    let distance =
        (distance.clamp(0.0, MAX_DISTANCE_M) / DISTANCE_STEP_M).round() * DISTANCE_STEP_M;
    let fraction = (fraction.clamp(0.0, 1.0) * FADE_STEPS as f32).round() / FADE_STEPS as f32;
    let start = distance - fraction * MARGIN_M;
    let transition = start..start + MARGIN_M;
    let outgoing = VisibilityRange {
        start_margin: -4.0..-3.0,
        end_margin: transition.clone(),
        use_aabb: false,
    };
    let incoming = VisibilityRange {
        start_margin: transition,
        end_margin: 1.0e8..1.0e8,
        use_aabb: false,
    };
    (outgoing, incoming)
}

pub fn tick(
    mut commands: Commands,
    // Camera/visual transitions continue while the train simulation is paused.
    time: Res<Time<Real>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut fades: Query<(Entity, &GlobalTransform, &mut LodFade)>,
) {
    let Ok(camera) = camera.single() else { return };
    for (entity, transform, mut fade) in &mut fades {
        fade.elapsed_s += time.delta_secs();
        if fade.elapsed_s >= DURATION_S {
            commands.entity(fade.outgoing).try_despawn();
            commands
                .entity(entity)
                .remove::<LodFade>()
                .insert(VisibilityRange {
                    start_margin: -4.0..-3.0,
                    end_margin: 1.0e8..1.0e8,
                    use_aabb: false,
                });
        } else {
            let distance = transform.translation().distance(camera.translation());
            let (outgoing, incoming) = ranges(distance, fade.elapsed_s / DURATION_S);
            commands.entity(fade.outgoing).try_insert(outgoing);
            commands.entity(entity).insert(incoming);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_are_complementary_at_every_fraction_and_distance() {
        for distance in [0.0, 10.0, 3000.0] {
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let (old, new) = ranges(distance, fraction);
                assert_eq!(old.end_margin, new.start_margin);
                let weight = (distance - new.start_margin.start) / MARGIN_M;
                assert!((weight - fraction).abs() < 0.04);
            }
        }
    }
    #[test]
    fn range_palette_cannot_exhaust_bevys_lifetime_u16_table() {
        let mut palette = std::collections::HashSet::new();
        for bucket in 0..=(MAX_DISTANCE_M / DISTANCE_STEP_M) as usize {
            for phase in 0..=FADE_STEPS {
                let (old, new) = ranges(
                    bucket as f32 * DISTANCE_STEP_M,
                    phase as f32 / FADE_STEPS as f32,
                );
                palette.insert(old);
                palette.insert(new);
            }
        }
        assert!(palette.len() < u16::MAX as usize - 1);
        // Moving within a distance bucket and phase reuses the same slots.
        assert!(ranges(800.0, 0.5) == ranges(802.0, 0.501));
    }
    #[test]
    fn only_visible_rigid_parts_crossfade_after_initial_selection() {
        assert!(!should_fade(true, true, false, 10.0));
        assert!(!should_fade(false, false, false, 10.0));
        assert!(!should_fade(false, true, true, 10.0));
        assert!(!should_fade(false, true, false, MAX_DISTANCE_M + 1.0));
        assert!(!should_fade(false, true, false, f32::NAN));
        assert!(should_fade(false, true, false, 10.0));
    }
    #[test]
    fn paused_simulation_still_finishes_visual_fades() {
        let mut app = App::new();
        let mut real = Time::<Real>::default();
        real.update_with_duration(std::time::Duration::ZERO);
        real.update_with_duration(std::time::Duration::from_secs_f32(DURATION_S + 0.01));
        let mut virtual_time = Time::<Virtual>::default();
        virtual_time.pause();
        app.insert_resource(real)
            .insert_resource(virtual_time)
            .add_systems(Update, tick);
        app.world_mut()
            .spawn((Camera3d::default(), GlobalTransform::default()));
        let old = app.world_mut().spawn_empty().id();
        let new = app
            .world_mut()
            .spawn((
                GlobalTransform::default(),
                LodFade {
                    outgoing: old,
                    elapsed_s: 0.0,
                },
            ))
            .id();
        app.update();
        assert!(app.world().get_entity(old).is_err());
        assert!(app.world().get::<LodFade>(new).is_none());
    }
}

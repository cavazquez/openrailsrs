//! Schedules apply only to author-named, self-lit WINDOW matrix parts.
//! Facade textures and reflective glass keep their original material.
use crate::world::{WorldSceneryLod, WorldShapeLodCache};
use bevy::prelude::*;
use std::collections::HashMap;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BuildingUse {
    Home,
    Office,
    Station,
}
pub fn activity(use_kind: BuildingUse, seconds: f64, seed: u64) -> f32 {
    if use_kind == BuildingUse::Station {
        return 1.;
    }
    let hour = (seconds / 3600.).rem_euclid(24.);
    let offset = ((seed >> 8) % 120) as f64 / 60.;
    let (start, end) = match use_kind {
        BuildingUse::Home => (18.0 - offset * 0.3, 23.0 + offset * 0.5),
        BuildingUse::Office => (6.5 + offset * 0.2, 19.0 + offset * 0.5),
        BuildingUse::Station => (0., 24.),
    };
    if use_kind == BuildingUse::Home && seed.is_multiple_of(5) {
        return 0.;
    }
    let warm = ((hour - start) * 60.).clamp(0., 1.);
    let cool = ((end - hour) * 60.).clamp(0., 1.);
    (warm.min(cool) as f32).clamp(0., 1.)
}
fn named_window(
    shape: &openrailsrs_formats::ShapeFile,
    part: &crate::shapes::ShapePartAsset,
) -> bool {
    let index = crate::rolling_stock_anim::matrix_idx_for_prim_state(shape, part.prim_state_idx);
    shape
        .matrices
        .get(index)
        .is_some_and(|m| m.name.to_ascii_uppercase().contains("WINDOW"))
}
fn seed(name: &str, seed: u64) -> u64 {
    let mut h = 0xcbf29ce484222325 ^ seed;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
struct Window {
    source: Handle<StandardMaterial>,
    material: Handle<StandardMaterial>,
    use_kind: BuildingUse,
    seed: u64,
    last: f32,
}
#[derive(Resource, Default)]
pub struct WindowSchedules {
    windows: HashMap<Entity, Window>,
    pub changed: u64,
    seed: u64,
}
impl WindowSchedules {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({"controlled_windows":self.windows.len(),"changes":self.changed,"bright":self.windows.values().filter(|w|w.last>0.5).count(),"seed":self.seed})
    }
}
pub fn prepare(mut state: ResMut<WindowSchedules>, live: Option<Res<crate::live::LiveDrive>>) {
    *state = default();
    state.seed = live
        .and_then(|l| openrailsrs_scenarios::load_scenario(l.scenario_path()).ok())
        .map_or(1, |s| s.simulation.seed);
}
pub fn update(
    mut commands: Commands,
    live: Res<crate::live::LiveDrive>,
    cache: Option<Res<WorldShapeLodCache>>,
    mut state: ResMut<WindowSchedules>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    parts: Query<(Entity, &WorldSceneryLod, &MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(cache) = cache else {
        return;
    };
    state.windows.retain(|e, _| parts.get(*e).is_ok());
    for (entity, lod, current) in &parts {
        if !state.windows.contains_key(&entity) {
            if state.windows.len() >= 2048 {
                break;
            }
            let Some(shape) = cache.shapes.get(&lod.shape_path) else {
                continue;
            };
            let Some(asset) = cache
                .assets_by_lod
                .get(&lod.shape_path)
                .and_then(|a| a.get(lod.lod_idx))
                .or_else(|| {
                    // A single-band shape is kept only in the primary spawn
                    // cache; it has no alternate LOD assets to consult.
                    (lod.lod_idx == 0)
                        .then(|| cache.shape_assets.get(&lod.shape_path))
                        .flatten()
                })
            else {
                continue;
            };
            let Some((_, part)) = crate::world::shape_lod_part_by_identity(
                asset,
                lod.sub_object_idx,
                lod.prim_state_idx,
            ) else {
                continue;
            };
            let Some(base) = materials.get(&current.0).cloned() else {
                continue;
            };
            if !(named_window(shape, part)
                || asset.has_night_subobj
                    && part.sub_object_idx == 1
                    && ["house", "building", "office", "station"].iter().any(|n| {
                        lod.shape_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(n)
                    }))
                || (!base.unlit && base.emissive == LinearRgba::BLACK)
            {
                continue;
            }
            let lower = lod
                .shape_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let use_kind = if lower.contains("station") || lower.contains("platform") {
                BuildingUse::Station
            } else if lower.contains("office") || lower.contains("factory") {
                BuildingUse::Office
            } else {
                BuildingUse::Home
            };
            let identity = format!("{lower}:{}:{}", part.sub_object_idx, part.prim_state_idx);
            let original = current.0.clone();
            let clone = materials.add(base.clone());
            let source_seed = state.seed;
            state.windows.insert(
                entity,
                Window {
                    source: original,
                    material: clone.clone(),
                    use_kind,
                    seed: seed(&identity, source_seed),
                    last: -1.,
                },
            );
            commands.entity(entity).insert(MeshMaterial3d(clone));
        }
        let Some(window) = state.windows.get_mut(&entity) else {
            continue;
        };
        // A LOD replacement has another original material. Re-register next frame.
        if current.0 != window.material && current.0 != window.source {
            state.windows.remove(&entity);
            continue;
        }
        let factor = activity(window.use_kind, live.clock_time_s(), window.seed);
        if (factor - window.last).abs() < 0.001 {
            continue;
        }
        let Some(original) = materials.get(&window.source).cloned() else {
            continue;
        };
        if let Some(mut material) = materials.get_mut(&window.material) {
            material.emissive = original.emissive * factor;
            if original.unlit {
                let c = LinearRgba::from(original.base_color);
                let brightness = 0.04 + 0.96 * factor;
                material.base_color = Color::linear_rgba(
                    c.red * brightness,
                    c.green * brightness,
                    c.blue * brightness,
                    c.alpha,
                );
            }
        }
        window.last = factor;
        state.changed += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn use_clock_and_seed_are_stable_at_pause_and_midnight() {
        assert_eq!(activity(BuildingUse::Home, 21. * 3600., 1), 1.);
        assert_eq!(activity(BuildingUse::Home, 21. * 3600., 5), 0.);
        assert_eq!(activity(BuildingUse::Office, 3. * 3600., 1), 0.);
        assert_eq!(activity(BuildingUse::Station, 3. * 3600., 1), 1.);
        for t in 0..86400 {
            let a = activity(BuildingUse::Home, t as f64, 3);
            let b = activity(BuildingUse::Home, t as f64 + 0.1, 3);
            assert!((a - b).abs() < 0.002);
        }
        assert_eq!(
            activity(BuildingUse::Home, 3600., 8),
            activity(BuildingUse::Home, 90000., 8)
        );
    }
}

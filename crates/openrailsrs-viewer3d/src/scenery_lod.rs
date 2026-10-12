//! Conservative silhouette policy for author LODs. Missing roofs/decks retain
//! the nearest complete authored level instead of replacement geometry.
use bevy::prelude::*;
use openrailsrs_formats::ShapeFile;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
#[derive(Resource, Default)]
pub struct Silhouettes {
    levels: HashMap<PathBuf, Vec<bool>>,
}
fn bounds(shape: &ShapeFile, index: usize) -> Option<(Vec3, Vec3)> {
    let level = shape.lod_controls.first()?.distance_levels.get(index)?;
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for sub in &level.sub_objects {
        for prim in &sub.primitives {
            let matrix =
                crate::rolling_stock_anim::matrix_idx_for_prim_state(shape, prim.prim_state_idx);
            let transform =
                openrailsrs_or_shader::coordinates::static_hierarchy_chain_transform(shape, matrix);
            for index in &prim.vertex_indices {
                let point = if sub.vertices.is_empty() {
                    Some(*index as usize)
                } else {
                    sub.vertices
                        .get(*index as usize)
                        .and_then(|v| usize::try_from(v.point_idx).ok())
                }
                .and_then(|i| shape.points.get(i));
                if let Some(p) = point {
                    let p =
                        transform.transform_point(Vec3::new(p.x as f32, p.y as f32, p.z as f32));
                    if p.is_finite() {
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
        }
    }
    min.is_finite().then_some((min, max))
}
pub fn safe_levels(shape: &ShapeFile) -> Vec<bool> {
    let Some(levels) = shape.lod_controls.first().map(|c| &c.distance_levels) else {
        return vec![true];
    };
    if levels.len() <= 1 {
        return vec![true; levels.len()];
    }
    let Some((min, max)) = bounds(shape, 0) else {
        return vec![true; levels.len()];
    };
    let extent = (max - min).max(Vec3::splat(0.02));
    let used = |level: &openrailsrs_formats::DistanceLevel| {
        level
            .sub_objects
            .iter()
            .flat_map(|s| &s.primitives)
            .filter(|p| !p.vertex_indices.is_empty())
            .map(|p| p.prim_state_idx)
            .collect::<std::collections::BTreeSet<_>>()
    };
    let base_materials = used(&levels[0]);
    levels
        .iter()
        .enumerate()
        .map(|(i, level)| {
            i == 0
                || base_materials.is_subset(&used(level))
                    && bounds(shape, i).is_some_and(|(a, b)| {
                        (((a - min) / extent).abs().max_element() <= 0.05)
                            && (((b - max) / extent).abs().max_element() <= 0.05)
                    })
        })
        .collect()
}
impl Silhouettes {
    pub fn choose(&mut self, path: &Path, shape: &ShapeFile, desired: usize) -> usize {
        if self.levels.len() > 4096 {
            self.levels.clear();
        }
        let safe = self
            .levels
            .entry(path.into())
            .or_insert_with(|| safe_levels(shape));
        (0..=desired.min(safe.len().saturating_sub(1)))
            .rev()
            .find(|i| safe[*i])
            .unwrap_or(0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_bridge_deck_or_roof_keeps_complete_source_level() {
        use openrailsrs_formats::{DistanceLevel, LodControl, Primitive, SubObject, Vec3 as Point};
        let shape = ShapeFile {
            points: vec![
                Point {
                    x: 0.,
                    y: 0.,
                    z: 0.,
                },
                Point {
                    x: 20.,
                    y: 0.,
                    z: 5.,
                },
                Point {
                    x: 0.,
                    y: 4.,
                    z: 0.,
                },
                Point {
                    x: 0.,
                    y: 2.,
                    z: 0.,
                },
            ],
            lod_controls: vec![LodControl {
                distance_levels: vec![
                    DistanceLevel {
                        sub_objects: vec![SubObject {
                            primitives: vec![Primitive {
                                prim_state_idx: 0,
                                vertex_indices: vec![0, 1, 2],
                            }],
                            ..default()
                        }],
                        ..default()
                    },
                    DistanceLevel {
                        sub_objects: vec![SubObject {
                            primitives: vec![Primitive {
                                prim_state_idx: 0,
                                vertex_indices: vec![0, 1, 3],
                            }],
                            ..default()
                        }],
                        ..default()
                    },
                ],
            }],
            ..default()
        };
        assert_eq!(safe_levels(&shape), vec![true, false]);
        assert_eq!(
            Silhouettes::default().choose(Path::new("bridge.s"), &shape, 1),
            0
        );
    }
}

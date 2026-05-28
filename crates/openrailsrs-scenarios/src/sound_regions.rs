//! Runtime tracker for `[[sound_regions]]` defined in `scenario.toml`.
//!
//! Compares the player train's `(edge_id, position_m)` against every region
//! and emits `Enter` / `Leave` transitions on edge crossings.

use std::collections::HashSet;

use crate::SoundRegionDef;

/// One region transition produced by [`RegionTracker::step`].
#[derive(Debug, Clone, PartialEq)]
pub enum RegionTransition {
    Enter {
        id: String,
        kind: String,
        base_volume: f32,
    },
    Leave {
        id: String,
    },
}

/// Tracks which sound regions are currently active for the player train.
#[derive(Debug, Default)]
pub struct RegionTracker {
    regions: Vec<SoundRegionDef>,
    active: HashSet<String>,
}

impl RegionTracker {
    /// Build a tracker from the regions declared in `scenario.toml`.
    pub fn new(regions: Vec<SoundRegionDef>) -> Self {
        Self {
            regions,
            active: HashSet::new(),
        }
    }

    /// Returns the set of currently active region ids (mainly for tests).
    #[cfg(test)]
    pub fn active_ids(&self) -> &HashSet<String> {
        &self.active
    }

    /// Update the tracker with the current train pose and return any
    /// `Enter`/`Leave` transitions triggered by this step.
    pub fn step(&mut self, edge_id: &str, position_m: f64) -> Vec<RegionTransition> {
        let mut transitions = Vec::new();
        let mut new_active: HashSet<String> = HashSet::new();

        for region in &self.regions {
            if region.edge_id == edge_id
                && (position_m - region.position_m).abs() <= region.radius_m
            {
                new_active.insert(region.id.clone());
            }
        }

        for region in &self.regions {
            if new_active.contains(&region.id) && !self.active.contains(&region.id) {
                transitions.push(RegionTransition::Enter {
                    id: region.id.clone(),
                    kind: region.kind.clone(),
                    base_volume: region.base_volume,
                });
            }
        }

        for id in self.active.iter() {
            if !new_active.contains(id) {
                transitions.push(RegionTransition::Leave { id: id.clone() });
            }
        }

        self.active = new_active;
        transitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(id: &str, edge: &str, pos: f64, radius: f64) -> SoundRegionDef {
        SoundRegionDef {
            id: id.into(),
            edge_id: edge.into(),
            position_m: pos,
            radius_m: radius,
            kind: "ambient".into(),
            base_volume: 0.4,
        }
    }

    #[test]
    fn outside_region_emits_no_transition() {
        let mut t = RegionTracker::new(vec![region("a", "e1", 100.0, 10.0)]);
        assert!(t.step("e1", 50.0).is_empty());
        assert!(t.active_ids().is_empty());
    }

    #[test]
    fn entering_region_emits_enter_once() {
        let mut t = RegionTracker::new(vec![region("a", "e1", 100.0, 20.0)]);
        let evs = t.step("e1", 90.0);
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], RegionTransition::Enter { id, .. } if id == "a"));
        assert!(t.step("e1", 95.0).is_empty());
    }

    #[test]
    fn leaving_region_emits_leave() {
        let mut t = RegionTracker::new(vec![region("a", "e1", 100.0, 20.0)]);
        t.step("e1", 90.0);
        let evs = t.step("e1", 200.0);
        assert_eq!(evs.len(), 1);
        assert!(matches!(&evs[0], RegionTransition::Leave { id } if id == "a"));
    }

    #[test]
    fn switching_edge_leaves_active_regions() {
        let mut t = RegionTracker::new(vec![
            region("a", "e1", 100.0, 20.0),
            region("b", "e2", 50.0, 10.0),
        ]);
        t.step("e1", 95.0);
        let evs = t.step("e2", 50.0);
        let mut kinds: Vec<&str> = evs
            .iter()
            .map(|e| match e {
                RegionTransition::Enter { id, .. } => id.as_str(),
                RegionTransition::Leave { id } => id.as_str(),
            })
            .collect();
        kinds.sort();
        assert_eq!(kinds, vec!["a", "b"]);
    }
}

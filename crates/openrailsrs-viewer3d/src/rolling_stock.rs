//! Rolling stock visuals from scenario consists (order 10 / issue #8).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_train::{Consist, Vehicle, consist_asset_root, load_consist_with_asset_root};

/// One vehicle in the consist ready for 3D spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsistVehicleVisual {
    pub name: String,
    pub shape_file: Option<String>,
    pub length_m: f32,
    /// Metres behind the train head along the travel axis (negative X local).
    pub offset_m: f32,
}

/// Loaded consists keyed by replay track label (`primary`, extra `id`, …).
#[derive(Resource, Clone, Default)]
pub struct TrainConsistScene {
    pub scenario_dir: Option<PathBuf>,
    pub by_label: HashMap<String, Vec<ConsistVehicleVisual>>,
}

impl TrainConsistScene {
    pub fn is_empty(&self) -> bool {
        self.by_label.is_empty()
    }

    pub fn vehicles_for(&self, label: &str) -> &[ConsistVehicleVisual] {
        self.by_label
            .get(label)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn track_count(&self) -> usize {
        self.by_label.len()
    }

    pub fn total_vehicles(&self) -> usize {
        self.by_label.values().map(|v| v.len()).sum()
    }

    pub fn shape_search_dirs<'a>(&'a self, route_dir: &'a Path) -> Vec<&'a Path> {
        let mut dirs = vec![route_dir];
        if let Some(scenario_dir) = self.scenario_dir.as_deref() {
            if scenario_dir != route_dir {
                dirs.push(scenario_dir);
            }
        }
        dirs
    }
}

/// Build vehicle visuals from a parsed consist.
pub fn vehicles_from_consist(consist: &Consist) -> Vec<ConsistVehicleVisual> {
    let lengths: Vec<f32> = consist
        .vehicles
        .iter()
        .map(|vehicle| match vehicle {
            Vehicle::Loco(l) => l.length_m as f32,
            Vehicle::Wagon(w) => w.length_m as f32,
        })
        .collect();
    let offsets = longitudinal_offsets_m(&lengths);

    consist
        .vehicles
        .iter()
        .zip(offsets)
        .map(|(vehicle, offset_m)| match vehicle {
            Vehicle::Loco(l) => ConsistVehicleVisual {
                name: l.name.clone(),
                shape_file: l.wagon_shape.clone(),
                length_m: l.length_m as f32,
                offset_m,
            },
            Vehicle::Wagon(w) => ConsistVehicleVisual {
                name: w.name.clone(),
                shape_file: w.wagon_shape.clone(),
                length_m: w.length_m as f32,
                offset_m,
            },
        })
        .collect()
}

/// Load vehicles from a `.con` path relative to the scenario directory.
pub fn try_load_consist_vehicles(
    scenario_dir: &Path,
    consist_rel: &str,
) -> Option<Vec<ConsistVehicleVisual>> {
    let con_path = scenario_dir.join(consist_rel);
    let asset_root = consist_asset_root(&con_path);
    let consist = load_consist_with_asset_root(&con_path, asset_root).ok()?;
    let vehicles = vehicles_from_consist(&consist);
    if vehicles.is_empty() {
        None
    } else {
        Some(vehicles)
    }
}

/// Longitudinal offsets from the train head (first vehicle at 0, followers negative).
pub fn longitudinal_offsets_m(lengths: &[f32]) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(lengths.len());
    let mut behind = 0.0_f32;
    for (i, &len) in lengths.iter().enumerate() {
        if i == 0 {
            offsets.push(0.0);
        } else {
            offsets.push(-behind);
        }
        behind += len;
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_chain_vehicles_nose_to_tail() {
        let offsets = longitudinal_offsets_m(&[18.0, 14.0]);
        assert_eq!(offsets, vec![0.0, -18.0]);
    }

    #[test]
    fn smoke_freight_consist_has_shapes() {
        let scenario_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let vehicles =
            try_load_consist_vehicles(&scenario_dir, "consists/freight.con").expect("freight.con");
        assert_eq!(vehicles.len(), 2);
        assert_eq!(vehicles[0].shape_file.as_deref(), Some("test.s"));
        assert_eq!(vehicles[1].shape_file.as_deref(), Some("test.s"));
        assert_eq!(vehicles[1].offset_m, -18.0);
    }

    #[test]
    fn multi_train_labels_use_toml_consist_paths() {
        let scenario_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let mut scene = TrainConsistScene {
            scenario_dir: Some(scenario_dir.clone()),
            by_label: HashMap::new(),
        };
        if let Some(v) = try_load_consist_vehicles(&scenario_dir, "consists/freight.con") {
            scene.by_label.insert("primary".into(), v);
        }
        if let Some(v) = try_load_consist_vehicles(&scenario_dir, "consists/freight.con") {
            scene.by_label.insert("express".into(), v);
        }
        assert_eq!(scene.track_count(), 2);
        assert_eq!(scene.vehicles_for("primary").len(), 2);
        assert_eq!(scene.vehicles_for("express").len(), 2);
        assert!(scene.vehicles_for("missing").is_empty());
    }
}

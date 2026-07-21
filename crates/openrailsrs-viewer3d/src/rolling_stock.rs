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
    /// Primary consist path from `scenario.toml` (`train.consist`).
    pub primary_consist_rel: Option<String>,
    pub by_label: HashMap<String, Vec<ConsistVehicleVisual>>,
    /// `examples/.../trains/*/SHAPES` (synced trainset meshes).
    trainset_shape_dirs: Vec<PathBuf>,
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

    pub fn shape_search_dirs(&self, route_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![route_dir.to_path_buf()];
        if let Some(scenario_dir) = self.scenario_dir.as_deref() {
            if scenario_dir != route_dir {
                dirs.push(scenario_dir.to_path_buf());
            }
            dirs.extend(self.trainset_shape_dirs.iter().cloned());
        }
        dirs
    }

    pub fn set_scenario_dir(&mut self, scenario_dir: PathBuf) {
        self.trainset_shape_dirs = collect_trainset_shape_dirs(&scenario_dir);
        self.scenario_dir = Some(scenario_dir);
    }
}

fn collect_trainset_shape_dirs(scenario_dir: &Path) -> Vec<PathBuf> {
    let trains = scenario_dir.join("trains");
    let Ok(entries) = std::fs::read_dir(trains) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            // Return the vehicle ROOT directory (not the SHAPES subdir) so that
            // resolve_shape_path can correctly append "SHAPES/" to build the shape
            // path, and the texture root (vehicle_root/TEXTURES/) is also correct.
            let path = entry.path();
            let shapes = path.join("SHAPES");
            shapes.is_dir().then_some(path)
        })
        .collect()
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
    fn trainset_shape_dirs_return_vehicle_roots_not_shapes_subdir() {
        // collect_trainset_shape_dirs must return the vehicle ROOT directory (e.g.
        // trains/RF_Blue_Pullman/) so that resolve_shape_path can correctly append
        // "SHAPES/" to build the full path.  If it returned the SHAPES/ subdir,
        // resolve_shape_path would produce "SHAPES/SHAPES/file.s" and find nothing.
        let scenario_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let dirs = collect_trainset_shape_dirs(&scenario_dir);
        // At least one dir should be found for the Blue Pullman.
        assert!(!dirs.is_empty(), "expected at least one trainset dir");
        for dir in &dirs {
            // Each returned dir must NOT end in "SHAPES" — it must be a vehicle root.
            let last = dir.file_name().unwrap_or_default().to_string_lossy();
            assert_ne!(
                last.to_uppercase(),
                "SHAPES",
                "returned SHAPES subdir instead of vehicle root: {dir:?}"
            );
            // The SHAPES subdir must exist under the returned vehicle root.
            assert!(
                dir.join("SHAPES").is_dir(),
                "vehicle root has no SHAPES/ subdir: {dir:?}"
            );
        }
    }

    #[test]
    fn offsets_chain_vehicles_nose_to_tail() {
        // Size / length_m drive consist spacing (coupler offsets), not mesh scale (#68).
        let lengths = [18.0_f32, 14.0];
        let offsets = longitudinal_offsets_m(&lengths);
        assert_eq!(offsets, vec![0.0, -18.0]);
        assert!((offsets[1] + lengths[0]).abs() < 1e-4);
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
        let mut scene = TrainConsistScene::default();
        scene.set_scenario_dir(scenario_dir.clone());
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

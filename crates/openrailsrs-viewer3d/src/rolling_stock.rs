//! Rolling stock visuals from scenario consists (order 10 / issue #8, PR1).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_train::{Consist, Vehicle};

/// One vehicle in the consist ready for 3D spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsistVehicleVisual {
    pub name: String,
    pub shape_file: Option<String>,
    pub length_m: f32,
    /// Metres behind the train head along the travel axis (negative X local).
    pub offset_m: f32,
}

/// Loaded consist for the primary scenario train (empty when not launched from `.toml`).
#[derive(Resource, Clone, Default)]
pub struct TrainConsistScene {
    pub scenario_dir: Option<PathBuf>,
    pub vehicles: Vec<ConsistVehicleVisual>,
}

impl TrainConsistScene {
    pub fn is_empty(&self) -> bool {
        self.vehicles.is_empty()
    }

    pub fn from_consist(consist: &Consist, scenario_dir: PathBuf) -> Self {
        let lengths: Vec<f32> = consist
            .vehicles
            .iter()
            .map(|vehicle| match vehicle {
                Vehicle::Loco(l) => l.length_m as f32,
                Vehicle::Wagon(w) => w.length_m as f32,
            })
            .collect();
        let offsets = longitudinal_offsets_m(&lengths);

        let vehicles = consist
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
            .collect();

        Self {
            scenario_dir: Some(scenario_dir),
            vehicles,
        }
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
    use openrailsrs_train::{consist_asset_root, load_consist_with_asset_root};

    #[test]
    fn offsets_chain_vehicles_nose_to_tail() {
        let offsets = longitudinal_offsets_m(&[18.0, 14.0]);
        assert_eq!(offsets, vec![0.0, -18.0]);
    }

    #[test]
    fn smoke_freight_consist_has_shapes() {
        let scenario_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let con_path = scenario_dir.join("consists/freight.con");
        let asset_root = consist_asset_root(&con_path);
        let consist =
            load_consist_with_asset_root(&con_path, asset_root).expect("freight.con loads");
        let scene = TrainConsistScene::from_consist(&consist, scenario_dir);
        assert_eq!(scene.vehicles.len(), 2);
        assert_eq!(scene.vehicles[0].shape_file.as_deref(), Some("test.s"));
        assert_eq!(scene.vehicles[1].shape_file.as_deref(), Some("test.s"));
        assert_eq!(scene.vehicles[1].offset_m, -18.0);
    }
}

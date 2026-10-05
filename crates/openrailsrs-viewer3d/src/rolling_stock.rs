//! Rolling stock visuals from scenario consists (order 10 / issue #8).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use openrailsrs_train::{Consist, Vehicle, consist_asset_root, load_consist_with_asset_root};

/// Stable consist slot on player and traffic car roots.
#[derive(Component, Clone, Copy, Debug)]
pub struct ConsistCarIndex(pub usize);

/// One vehicle in the consist ready for 3D spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsistVehicleVisual {
    pub name: String,
    pub shape_file: Option<String>,
    /// Authored ENG/WAG directory, retaining identity when filenames are shared.
    pub asset_dir: Option<PathBuf>,
    pub length_m: f32,
    /// Metres behind the train head along the travel axis (negative X local).
    pub offset_m: f32,
    /// `.con` Flip — Y 180° on the vehicle shape; consist order unchanged (#130).
    pub flipped: bool,
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
            for vehicle in self.vehicles_for("primary").iter().chain(
                self.by_label
                    .iter()
                    .filter(|(label, _)| label.as_str() != "primary")
                    .flat_map(|(_, cars)| cars.iter()),
            ) {
                if let Some(root) = &vehicle.asset_dir
                    && !dirs.contains(root)
                {
                    dirs.push(root.clone());
                }
            }
            dirs.extend(self.trainset_shape_dirs.iter().cloned());
            if let Some(relative) = &self.primary_consist_rel {
                let path = scenario_dir.join(relative);
                let root = consist_asset_root(&path);
                if root != scenario_dir {
                    dirs.push(root.to_path_buf());
                    dirs.extend(collect_trainset_shape_dirs(root));
                }
            }
        }
        dirs
    }

    pub fn set_scenario_dir(&mut self, scenario_dir: PathBuf) {
        self.trainset_shape_dirs = collect_trainset_shape_dirs(&scenario_dir);
        self.scenario_dir = Some(scenario_dir);
    }
}

fn collect_trainset_shape_dirs(scenario_dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<_> = [scenario_dir.join("trains"), scenario_dir.join("TRAINSET")]
        .into_iter()
        .flat_map(|trains| std::fs::read_dir(trains).into_iter().flatten())
        .flatten()
        .filter_map(|entry| {
            // Return the vehicle ROOT directory (not the SHAPES subdir) so that
            // resolve_shape_path can correctly append "SHAPES/" to build the shape
            // path, and the texture root (vehicle_root/TEXTURES/) is also correct.
            let path = entry.path();
            let shapes = path.join("SHAPES");
            shapes.is_dir().then_some(path)
        })
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
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
                asset_dir: None,
                length_m: l.length_m as f32,
                offset_m,
                flipped: l.flipped,
            },
            Vehicle::Wagon(w) => ConsistVehicleVisual {
                name: w.name.clone(),
                shape_file: w.wagon_shape.clone(),
                asset_dir: None,
                length_m: w.length_m as f32,
                offset_m,
                flipped: w.flipped,
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
    let mut vehicles = vehicles_from_consist(&consist);
    if let Ok(entries) = openrailsrs_formats::read_msts_file_to_string(&con_path)
        .and_then(|text| openrailsrs_formats::parse_vehicle_text(&text))
        .and_then(|ast| openrailsrs_formats::ConsistFile::from_ast(&ast))
    {
        for (visual, entry) in vehicles.iter_mut().zip(entries.entries) {
            let stock = openrailsrs_train::resolve_consist_entry_path(asset_root, entry.path());
            visual.asset_dir = stock.parent().map(Path::to_path_buf);
        }
    }
    if vehicles.is_empty() {
        None
    } else {
        Some(vehicles)
    }
}

/// Resolve from the authored stock first; shared filenames must not select a
/// model belonging to another formation. Original Content still overrides fixtures.
pub fn resolve_consist_vehicle_shape_path(
    dirs: &[&Path],
    vehicle: &ConsistVehicleVisual,
    route: &Path,
) -> Option<PathBuf> {
    let name = vehicle.shape_file.as_deref()?;
    if let Some(root) = &vehicle.asset_dir {
        if root.ancestors().any(|p| {
            p.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("TRAINSET"))
        }) && let Some(path) = crate::shapes::resolve_shape_path(root, name)
        {
            return Some(path);
        }
        let trainset = root.file_name()?.to_string_lossy();
        for native in crate::shapes::or_content_trainset_roots(route, &trainset) {
            if let Some(path) = crate::shapes::resolve_shape_path(&native, name) {
                return Some(path);
            }
        }
        if let Some(path) = crate::shapes::resolve_shape_path(root, name) {
            return Some(path);
        }
    }
    crate::shapes::resolve_vehicle_shape_path(dirs, name, route)
}

/// Authored vehicle centres measured behind the physical train head.
/// Native ENG/WAG `Size` gives the full length; successive centres are separated
/// by the half-lengths of both vehicles, including mixed locomotive/car sizes.
pub fn longitudinal_offsets_m(lengths: &[f32]) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(lengths.len());
    let mut behind = 0.0_f32;
    for &len in lengths {
        offsets.push(-behind - len * 0.5);
        behind += len;
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_native_directory_wins_over_same_named_model_in_other_stock() {
        let temp = tempfile::tempdir().unwrap();
        let wrong = temp.path().join("wrong");
        let right = temp.path().join("TRAINS/TRAINSET/specific-stock");
        std::fs::create_dir_all(&wrong).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        std::fs::write(wrong.join("shared.s"), b"wrong geometry").unwrap();
        std::fs::write(right.join("shared.s"), b"authored geometry").unwrap();
        let vehicle = ConsistVehicleVisual {
            name: "native".into(),
            shape_file: Some("shared.s".into()),
            asset_dir: Some(right.clone()),
            length_m: 20.0,
            offset_m: 0.0,
            flipped: false,
        };
        assert_eq!(
            resolve_consist_vehicle_shape_path(&[wrong.as_path()], &vehicle, temp.path()),
            Some(right.join("shared.s"))
        );
    }

    #[test]
    fn consist_visuals_keep_the_stock_directory_for_every_vehicle() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let cars = try_load_consist_vehicles(&root, "consists/birmingham_pullman.con").unwrap();
        assert_eq!(cars.len(), 8);
        assert!(cars.iter().all(|car| {
            car.asset_dir
                .as_ref()
                .is_some_and(|p| p.ends_with("RF_Blue_Pullman"))
        }));
    }

    #[test]
    fn relative_consist_in_another_scenario_keeps_its_trainset_assets() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut scene = TrainConsistScene::default();
        scene.set_scenario_dir(root.join("chiltern_local"));
        scene.primary_consist_rel = Some("../chiltern/consists/birmingham_pullman.con".into());
        let dirs = scene.shape_search_dirs(&root.join("chiltern_local"));
        assert!(
            dirs.iter().any(|dir| dir.join("SHAPES").is_dir()),
            "physics and rendering must resolve the same relative consist root"
        );
    }

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
        assert_eq!(offsets, vec![-9.0, -25.0]);
        assert_eq!(offsets[0] + lengths[0] * 0.5, 0.0);
        assert_eq!(offsets[0] - lengths[0] * 0.5, offsets[1] + lengths[1] * 0.5);
    }

    #[test]
    fn flip_flags_preserve_consist_order() {
        // #130: Flip is per-vehicle orientation only; lead→tail order matches `.con`.
        use openrailsrs_train::{Consist, DavisCoefficients, Vehicle, Wagon};
        let consist = Consist {
            vehicles: vec![
                Vehicle::Wagon(Wagon {
                    name: "a".into(),
                    mass_kg: 1.0,
                    max_brake_force_n: 0.0,
                    length_m: 10.0,
                    davis: DavisCoefficients::default(),
                    wagon_shape: Some("a.s".into()),
                    brake_shoe_type: Default::default(),
                    brake_shoe_friction: None,
                    brake_profile: Default::default(),
                    flipped: false,
                }),
                Vehicle::Wagon(Wagon {
                    name: "b".into(),
                    mass_kg: 1.0,
                    max_brake_force_n: 0.0,
                    length_m: 10.0,
                    davis: DavisCoefficients::default(),
                    wagon_shape: Some("b.s".into()),
                    brake_shoe_type: Default::default(),
                    brake_shoe_friction: None,
                    brake_profile: Default::default(),
                    flipped: true,
                }),
            ],
            davis: DavisCoefficients::default(),
        };
        let visuals = vehicles_from_consist(&consist);
        assert_eq!(visuals.len(), 2);
        assert_eq!(visuals[0].name, "a");
        assert!(!visuals[0].flipped);
        assert_eq!(visuals[1].name, "b");
        assert!(visuals[1].flipped);
        assert!((visuals[1].offset_m + 15.0).abs() < 1e-4);
    }

    #[test]
    fn smoke_freight_consist_has_shapes() {
        let scenario_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let vehicles =
            try_load_consist_vehicles(&scenario_dir, "consists/freight.con").expect("freight.con");
        assert_eq!(vehicles.len(), 2);
        assert_eq!(vehicles[0].shape_file.as_deref(), Some("test.s"));
        assert_eq!(vehicles[1].shape_file.as_deref(), Some("test.s"));
        assert_eq!(vehicles[1].offset_m, -25.0);
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

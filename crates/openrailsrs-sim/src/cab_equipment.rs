//! Finite washer pump and independently operated cab apertures on the physics clock.
use crate::{LiveDriveSession, TrainSimState};
use openrailsrs_scenarios::cab_equipment::CabEquipmentDef;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CabEquipmentState {
    pub config: CabEquipmentDef,
    pub windows: [f64; 2],
    pub window_targets: [bool; 2],
    pub door: f64,
    pub door_target: bool,
    pub water_l: f64,
    pub used_l: f64,
    pub pump_command: bool,
    pub spraying: bool,
}
impl CabEquipmentState {
    pub fn new(config: CabEquipmentDef) -> Self {
        Self {
            water_l: config.washer_capacity_l,
            config,
            windows: [0.; 2],
            window_targets: [false; 2],
            door: 0.,
            door_target: false,
            used_l: 0.,
            pump_command: false,
            spraying: false,
        }
    }
    pub fn valid(&self) -> bool {
        self.config.valid()
            && self
                .windows
                .iter()
                .chain([&self.door])
                .all(|v| (0.0..=1.).contains(v))
            && self.water_l.is_finite()
            && (0.0..=self.config.washer_capacity_l).contains(&self.water_l)
            && self.used_l.is_finite()
            && (0.0..=self.config.washer_capacity_l + 1e-8).contains(&self.used_l)
            && (!self.spraying || self.pump_command)
    }
}
#[derive(Clone, Copy, Debug)]
pub enum CabOperation {
    LeftWindow,
    RightWindow,
    Door,
    Washer,
}
pub fn advance(state: &mut TrainSimState, dt: f64) {
    for cab in &mut state.cab_equipment {
        let powered = state
            .power_supply
            .cars
            .iter()
            .find(|s| s.vehicle == cab.config.vehicle)
            .is_some_and(|s| s.supply.low_voltage);
        let move_to = |value: f64, target: bool| {
            (value + if target { dt / 1.5 } else { -dt / 1.5 }).clamp(0., 1.)
        };
        for (i, declared) in [cab.config.left_window, cab.config.right_window]
            .into_iter()
            .enumerate()
        {
            if declared {
                cab.windows[i] = move_to(cab.windows[i], cab.window_targets[i]);
            }
        }
        if cab.config.cab_door {
            cab.door = move_to(cab.door, cab.door_target);
        }
        cab.spraying =
            cab.pump_command && powered && cab.water_l > 0. && cab.config.washer_capacity_l > 0.;
        if cab.spraying {
            let used = (cab.config.washer_flow_lps * dt).min(cab.water_l);
            cab.water_l = (cab.water_l - used).max(0.);
            cab.used_l += used;
        }
    }
}
impl LiveDriveSession {
    pub fn cab_equipment(&self) -> Option<&CabEquipmentState> {
        self.state
            .cab_equipment
            .iter()
            .find(|c| c.config.vehicle == self.cab_vehicle())
    }
    pub fn operate_cab(&mut self, operation: CabOperation) -> Result<(), String> {
        let vehicle = self.cab_vehicle();
        let stopped = self.velocity_mps().abs() < 0.1;
        let cab = self
            .state
            .cab_equipment
            .iter_mut()
            .find(|c| c.config.vehicle == vehicle)
            .ok_or("Esta cabina no declara este equipo")?;
        match operation {
            CabOperation::LeftWindow if cab.config.left_window => {
                cab.window_targets[0] = !cab.window_targets[0]
            }
            CabOperation::RightWindow if cab.config.right_window => {
                cab.window_targets[1] = !cab.window_targets[1]
            }
            CabOperation::Door if cab.config.cab_door => {
                if !cab.door_target && !stopped {
                    return Err("Detené el tren antes de abrir la puerta de cabina".into());
                }
                cab.door_target = !cab.door_target;
            }
            CabOperation::Washer if cab.config.washer_capacity_l > 0. => {
                if !cab.pump_command && cab.water_l <= 0. {
                    return Err("Depósito de lavaparabrisas vacío".into());
                }
                cab.pump_command = !cab.pump_command;
                if !cab.pump_command {
                    cab.spraying = false;
                }
            }
            _ => return Err("El vehículo original no declara este equipo".into()),
        }
        Ok(())
    }
}
pub fn load(
    con: &std::path::Path,
    declared: &[CabEquipmentDef],
    count: usize,
) -> Result<Vec<CabEquipmentState>, String> {
    use openrailsrs_formats::*;
    let mut equipment: std::collections::BTreeMap<usize, CabEquipmentDef> =
        std::collections::BTreeMap::new();
    if let Ok(parsed) = read_msts_file_to_string(con)
        .and_then(|s| parse_vehicle_text(&s))
        .and_then(|a| ConsistFile::from_ast(&a))
    {
        for (vehicle, entry) in parsed.entries.iter().enumerate() {
            let ConsistEntry::Engine { path, .. } = entry else {
                continue;
            };
            let path = openrailsrs_train::resolve_consist_entry_path(
                openrailsrs_train::consist_asset_root(con),
                path,
            );
            let Ok(MstsFile::Engine(engine)) = parse_msts_file(&path) else {
                continue;
            };
            let root = path.parent().unwrap_or(std::path::Path::new("."));
            let paths = resolve_cab_assets(root, &engine.cab)
                .map(|a| a.cvf_path)
                .into_iter()
                .chain(resolve_cab_view_path(root, &engine.cab));
            let mut config = CabEquipmentDef {
                vehicle,
                ..Default::default()
            };
            for path in paths {
                if let Ok(cvf) = read_msts_file_to_string(&path)
                    .and_then(|s| parse_cab_view_text(&s))
                    .and_then(|a| CabViewFile::from_ast(&a))
                {
                    for name in cvf.control_type_names() {
                        let name = name.to_ascii_uppercase();
                        config.left_window |= matches!(
                            name.as_str(),
                            "ORTS_LEFTWINDOW" | "ORTS_2DEXTERNALLEFTWINDOW"
                        );
                        config.right_window |= matches!(
                            name.as_str(),
                            "ORTS_RIGHTWINDOW" | "ORTS_2DEXTERNALRIGHTWINDOW"
                        );
                        config.cab_door |= name == "ORTS_CABDOOR";
                    }
                }
            }
            if config.left_window || config.right_window || config.cab_door {
                equipment.insert(vehicle, config);
            }
        }
    }
    for config in declared {
        if !config.valid() || config.vehicle >= count {
            return Err("Equipo de cabina fuera de la formación".into());
        }
        equipment.insert(config.vehicle, config.clone());
    }
    Ok(equipment
        .into_values()
        .map(CabEquipmentState::new)
        .collect())
}

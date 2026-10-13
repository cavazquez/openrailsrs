//! Shared physical equipment and commands for CLI, player and automatic trains.
use crate::{LiveDriveSession, state::TrainSimState};
use openrailsrs_scenarios::vehicle_systems::{DoorSide, PlatformSideDef, VehicleSystemsDef};
use serde::{Deserialize, Serialize};

pub fn configure(
    state: &mut TrainSimState,
    con: &std::path::Path,
    definitions: &[VehicleSystemsDef],
    platforms: &[PlatformSideDef],
    count: usize,
) -> Result<(), String> {
    use openrailsrs_formats::{
        ConsistEntry, ConsistFile, parse_vehicle_text, read_msts_file_to_string,
    };
    let ast = parse_vehicle_text(&read_msts_file_to_string(con).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let con_data = ConsistFile::from_ast(&ast).map_err(|e| e.to_string())?;
    let base = openrailsrs_train::consist_asset_root(con);
    let mut merged = std::collections::BTreeMap::new();
    for (vehicle, entry) in con_data.entries.iter().enumerate() {
        let relative = match entry {
            ConsistEntry::Engine { path, .. } | ConsistEntry::Wagon { path, .. } => path,
        };
        let file = openrailsrs_train::resolve_consist_entry_path(base, relative);
        let text =
            openrailsrs_formats::read_msts_text_with_includes(&file).map_err(|e| e.to_string())?;
        let Ok(ast) = parse_vehicle_text(&text) else {
            continue;
        };
        if con
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains("tilted"))
        {
            merged
                .entry(vehicle)
                .or_insert_with(|| VehicleSystemsDef {
                    vehicle,
                    ..Default::default()
                })
                .tilt = Some(openrailsrs_scenarios::vehicle_systems::TiltDef {
                native_response: true,
                maximum_rad: 8_f64.to_radians(),
                time_constant_s: 1.,
                max_rate_rad_s: 0.2,
                compensation: 1.,
                minimum_speed_mps: 50. / 3.6,
            });
        }
        let delay = openrailsrs_formats::typed::vehicle_equipment::parse_native_doors(&ast)
            .map_err(|e| e.to_string())?;
        let shape = match entry {
            ConsistEntry::Engine { .. } => openrailsrs_formats::EngineFile::from_ast(&ast)
                .ok()
                .and_then(|e| e.wagon_shape),
            ConsistEntry::Wagon { .. } => openrailsrs_formats::WagonFile::from_ast(&ast)
                .ok()
                .and_then(|e| e.wagon_shape),
        };
        if let Some(brake) =
            openrailsrs_formats::typed::vehicle_equipment::parse_native_dynamic_brake(&ast)
                .map_err(|e| e.to_string())?
        {
            merged
                .entry(vehicle)
                .or_insert_with(|| VehicleSystemsDef {
                    vehicle,
                    ..Default::default()
                })
                .electric_brake = Some(openrailsrs_scenarios::vehicle_systems::ElectricBrakeDef {
                native_response: true,
                force_ramp_n_s: brake.force_ramp_n_s,
                power_ramp_w_s: brake.power_ramp_w_s,
                maximum_current_a: brake.maximum_current_a,
                force_curve: brake.force_curve,
                notches: brake.notches,
                rheostat_max_w: 1e12,
                regenerative_max_w: 0.,
                receptivity_w: 0.,
                efficiency: 1.,
                apply_delay_s: brake.delay_s,
                blend_with_service: false,
            });
        }
        let native_motors =
            openrailsrs_formats::typed::vehicle_equipment::parse_native_motors(&ast)
                .map_err(|e| e.to_string())?;
        let explicit_circuits = definitions
            .iter()
            .any(|d| d.vehicle == vehicle && d.diesel_circuits.is_some() && d.motors.is_empty());
        if (native_motors.len() > 1 || explicit_circuits)
            && let Ok(e) = openrailsrs_formats::EngineFile::from_ast(&ast)
        {
            let count = native_motors.len().max(1);
            let tank = e.diesel_operation.as_ref().map_or(0., |p| p.capacity_l);
            let motors = if native_motors.is_empty() {
                vec![openrailsrs_formats::typed::vehicle_equipment::NativeMotor {
                    power_curve: e.diesel_power_tab.iter().map(|(a, b)| [*a, *b]).collect(),
                    throttle_rpm_curve: e
                        .diesel_throttle_rpm_tab
                        .iter()
                        .map(|(a, b)| [*a, *b])
                        .collect(),
                    rate_up_rpm_s2: e.diesel_rate_of_change_up_rpm_pss.max(40.),
                    rate_down_rpm_s2: e.diesel_rate_of_change_down_rpm_pss.max(40.),
                    idle_rpm: e.diesel_idle_rpm.max(300.),
                    maximum_rpm: e.diesel_max_rpm.max(e.diesel_idle_rpm + 1.).max(600.),
                    rise_rpm_s: e.diesel_change_up_rpm_ps.max(40.),
                    fall_rpm_s: e.diesel_change_down_rpm_ps.max(40.),
                    power_w: e.max_power_w,
                }]
            } else {
                native_motors
            };
            let motors = motors
                .into_iter()
                .enumerate()
                .map(
                    |(index, m)| openrailsrs_scenarios::vehicle_systems::MotorDef {
                        native_governor: Some(
                            openrailsrs_scenarios::vehicle_systems::NativeMotorGovernor {
                                power_curve: m.power_curve.clone(),
                                throttle_rpm_curve: m.throttle_rpm_curve.clone(),
                                rate_up_rpm_s2: m.rate_up_rpm_s2,
                                rate_down_rpm_s2: m.rate_down_rpm_s2,
                            },
                        ),
                        id: format!("Diesel {}", index + 1),
                        idle_rpm: m.idle_rpm,
                        maximum_rpm: m.maximum_rpm,
                        rise_rpm_s: m.rise_rpm_s,
                        fall_rpm_s: m.fall_rpm_s,
                        rated_power_w: if m.power_w > 0. {
                            m.power_w
                        } else {
                            e.max_power_w / count as f64
                        },
                        fuel_l: tank / count as f64,
                        idle_lph: e
                            .diesel_operation
                            .as_ref()
                            .and_then(|p| p.consumption_lph.first())
                            .map_or(0., |p| p.1)
                            / count as f64,
                        full_lph: e
                            .diesel_operation
                            .as_ref()
                            .and_then(|p| p.consumption_lph.last())
                            .map_or(0., |p| p.1)
                            / count as f64,
                    },
                )
                .collect();
            merged
                .entry(vehicle)
                .or_insert_with(|| VehicleSystemsDef {
                    vehicle,
                    ..Default::default()
                })
                .motors = motors;
        }
        let mut sides = [false; 2];
        if !definitions
            .iter()
            .any(|p| p.vehicle == vehicle && p.doors.is_some())
            && let Some(name) = shape
        {
            let root = file.parent().unwrap_or(base);
            let name = name.replace('\\', "/");
            for shape in [
                root.join(&name),
                root.join("SHAPES").join(&name),
                root.join("Shapes").join(&name),
            ] {
                if !std::fs::metadata(&shape)
                    .is_ok_and(|m| m.is_file() && m.len() <= 32 * 1024 * 1024)
                {
                    continue;
                }
                if let Ok(shape) =
                    openrailsrs_formats::ShapeFile::from_path_limited(&shape, 64 * 1024 * 1024)
                {
                    for matrix in &shape.matrices {
                        if let Some(side) =
                            openrailsrs_formats::typed::vehicle_equipment::door_matrix_side(
                                &matrix.name,
                            )
                        {
                            sides[side] = true;
                        }
                    }
                    break;
                }
            }
        }
        if sides != [false; 2] || delay.is_some() {
            let (open_s, close_s) = delay
                .as_ref()
                .map_or((0., 0.), |p| (p.opening_s, p.closing_s));
            if sides == [false; 2] {
                sides = [true; 2];
            }
            let tilt = merged.get(&vehicle).and_then(|d| d.tilt.clone());
            let electric_brake = merged.get(&vehicle).and_then(|d| d.electric_brake.clone());
            let motors = merged
                .get(&vehicle)
                .map(|d| d.motors.clone())
                .unwrap_or_default();
            merged.insert(
                vehicle,
                VehicleSystemsDef {
                    vehicle,
                    motors,
                    electric_brake,
                    tilt,
                    doors: Some(openrailsrs_scenarios::vehicle_systems::DoorsDef {
                        left: sides[0],
                        right: sides[1],
                        open_s,
                        close_s,
                        traction_interlock: true,
                    }),
                    ..Default::default()
                },
            );
        }
    }
    for definition in definitions {
        let mut definition = definition.clone();
        if let Some(native) = merged.get(&definition.vehicle) {
            definition.doors = definition.doors.or_else(|| native.doors.clone());
            definition.tilt = definition.tilt.or_else(|| native.tilt.clone());
            definition.electric_brake = definition
                .electric_brake
                .or_else(|| native.electric_brake.clone());
            if definition.motors.is_empty() {
                definition.motors = native.motors.clone();
            }
        }
        merged.insert(definition.vehicle, definition);
    }
    install(
        state,
        &merged.into_values().collect::<Vec<_>>(),
        platforms,
        count,
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleSystemState {
    #[serde(default)]
    pub aux: crate::auxiliary_work::AuxiliaryState,
    #[serde(default)]
    pub mu: crate::multiple_unit::MultipleUnitState,
    #[serde(default)]
    pub tilt: crate::tilting::TiltState,
    #[serde(default)]
    pub motors: Vec<crate::engine_equipment::MotorState>,
    #[serde(default)]
    pub brakes: crate::brake_equipment::BrakeEquipmentState,
    #[serde(default)]
    pub energy: crate::traction_energy::TractionEnergyState,
    #[serde(default)]
    pub crosswind: crate::crosswind::WindRollState,
    pub definition: VehicleSystemsDef,
    pub door_fraction: [f64; 2],
    pub door_target: [bool; 2],
}
impl VehicleSystemState {
    pub fn new(definition: VehicleSystemsDef) -> Self {
        Self {
            motors: definition
                .motors
                .iter()
                .map(|p| {
                    crate::engine_equipment::MotorState::new(p, definition.diesel_circuits.as_ref())
                })
                .collect(),
            aux: crate::auxiliary_work::AuxiliaryState::new(definition.auxiliaries.as_ref()),
            mu: Default::default(),
            tilt: Default::default(),
            brakes: Default::default(),
            energy: crate::traction_energy::TractionEnergyState::new(definition.battery.as_ref()),
            crosswind: Default::default(),
            definition,
            door_fraction: [0.; 2],
            door_target: [false; 2],
        }
    }
    pub fn valid(&self) -> bool {
        self.definition.valid()
            && self.aux.valid()
            && self.aux.initial_battery_j
                == self
                    .definition
                    .auxiliaries
                    .as_ref()
                    .map_or(0., |p| p.battery_kwh * 3600000.)
            && self.mu.valid()
            && self.tilt.valid()
            && (self.definition.tilt.is_some() || self.tilt == Default::default())
            && self.brakes.valid()
            && self.motors.len() == self.definition.motors.len()
            && self
                .motors
                .iter()
                .zip(&self.definition.motors)
                .all(|(s, p)| s.valid(p, self.definition.diesel_circuits.as_ref()))
            && self.energy.valid(self.definition.battery.as_ref())
            && self.crosswind.valid()
            && (self.definition.crosswind.is_some() || self.crosswind == Default::default())
            && self
                .door_fraction
                .iter()
                .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && (self.definition.doors.is_some()
                || self.door_fraction == [0.; 2] && self.door_target == [false; 2])
            && self.definition.doors.as_ref().is_none_or(|p| {
                (p.left || self.door_fraction[0] == 0. && !self.door_target[0])
                    && (p.right || self.door_fraction[1] == 0. && !self.door_target[1])
            })
    }
}

pub fn install(
    state: &mut TrainSimState,
    definitions: &[VehicleSystemsDef],
    platforms: &[PlatformSideDef],
    count: usize,
) -> Result<(), String> {
    if !openrailsrs_scenarios::vehicle_systems::valid_definitions(definitions, platforms)
        || definitions.iter().any(|p| p.vehicle >= count)
    {
        return Err("Equipos declarados fuera de la formación o inválidos".into());
    }
    state.vehicle_systems = definitions
        .iter()
        .cloned()
        .map(VehicleSystemState::new)
        .collect();
    state.platform_sides = platforms.to_vec();
    Ok(())
}

pub fn advance_doors(state: &mut TrainSimState, count: usize, dt: f64) {
    if !dt.is_finite() || dt <= 0. {
        return;
    }
    for car in &mut state.vehicle_systems {
        if car.definition.vehicle >= count {
            continue;
        }
        let Some(p) = &car.definition.doors else {
            continue;
        };
        for side in 0..2 {
            if ![p.left, p.right][side] {
                continue;
            }
            let target = car.door_target[side];
            let duration = if target { p.open_s } else { p.close_s };
            let rate = if duration <= 0. { 1. } else { dt / duration };
            car.door_fraction[side] =
                (car.door_fraction[side] + if target { rate } else { -rate }).clamp(0., 1.);
        }
    }
}

pub fn doors_allow_traction(state: &TrainSimState, count: usize) -> bool {
    state
        .vehicle_systems
        .iter()
        .filter(|s| s.definition.vehicle < count)
        .all(|s| {
            s.definition.doors.as_ref().is_none_or(|p| {
                !p.traction_interlock || s.door_fraction == [0.; 2] && s.door_target == [false; 2]
            })
        })
}

impl LiveDriveSession {
    pub fn platform_door_side(&self) -> Option<DoorSide> {
        let next = self.gameplay.next_stop_idx;
        self.state
            .platform_sides
            .iter()
            .find(|p| p.stop == next)
            .map(|p| {
                if self.state.path_reversed {
                    p.side.flipped()
                } else {
                    p.side
                }
            })
    }
    pub fn command_doors(
        &mut self,
        vehicle: Option<usize>,
        side: DoorSide,
        open: bool,
    ) -> Result<(), String> {
        if open && self.velocity_mps().abs() > 0.1 {
            return Err("Detené el tren antes de abrir puertas".into());
        }
        if open && let Some(platform) = self.platform_door_side() {
            let near = self
                .gameplay
                .stop_targets
                .get(self.gameplay.next_stop_idx)
                .is_some_and(|p| (p.cum_dist_m - self.gameplay_chainage_m()).abs() <= 75.);
            if !near {
                return Err("El tren no está junto al andén indicado".into());
            }
            if platform != DoorSide::Both && side != platform {
                return Err("Elegí el lado del andén habilitado".into());
            }
        }
        let flipped = self
            .consist
            .vehicles
            .iter()
            .map(|v| match v {
                openrailsrs_train::Vehicle::Loco(l) => l.flipped,
                openrailsrs_train::Vehicle::Wagon(w) => w.flipped,
            })
            .collect::<Vec<_>>();
        let mut applied = false;
        for car in &mut self.state.vehicle_systems {
            let index = car.definition.vehicle;
            if index >= self.formation.coupled_count || vehicle.is_some_and(|i| i != index) {
                continue;
            }
            let Some(p) = &car.definition.doors else {
                continue;
            };
            let local = if flipped.get(index) == Some(&true) {
                side.flipped()
            } else {
                side
            };
            for side in 0..2 {
                if local.includes(side) && [p.left, p.right][side] {
                    car.door_target[side] = open;
                    applied = true;
                }
            }
        }
        if !applied {
            return Err("El vehículo no declara puertas habilitadas de ese lado".into());
        }
        self.exterior.set_door(if open {
            crate::DoorState::Opening
        } else {
            crate::DoorState::Closing
        });
        Ok(())
    }
    pub fn equipped_door_state(&self) -> Option<crate::DoorState> {
        let cars = self
            .state
            .vehicle_systems
            .iter()
            .filter(|c| {
                c.definition.vehicle < self.formation.coupled_count && c.definition.doors.is_some()
            })
            .collect::<Vec<_>>();
        if cars.is_empty() {
            return None;
        }
        if cars
            .iter()
            .all(|c| c.door_fraction == [0.; 2] && c.door_target == [false; 2])
        {
            return Some(crate::DoorState::Closed);
        }
        if cars.iter().any(|c| {
            c.door_target
                .iter()
                .enumerate()
                .any(|(i, t)| *t && c.door_fraction[i] < 1.)
        }) {
            return Some(crate::DoorState::Opening);
        }
        if cars.iter().any(|c| {
            c.door_target
                .iter()
                .enumerate()
                .any(|(i, t)| !*t && c.door_fraction[i] > 0.)
        }) {
            return Some(crate::DoorState::Closing);
        }
        Some(crate::DoorState::Open)
    }
}

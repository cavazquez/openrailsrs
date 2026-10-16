//! Physical portions share the same vehicle systems; only their grouping and
//! directed track anchors change. No operation relocates a wagon to a branch.
use crate::{CouplerState, LiveDriveSession, path_data::PathData};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CouplingEnd {
    Front,
    #[default]
    Rear,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetachedSection {
    pub id: String,
    pub start: usize,
    pub count: usize,
    /// Coordinate of the nominal leading coupler, on this immutable path.
    pub anchor_m: f64,
    pub path_edges: Vec<String>,
    pub reversed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DetachedSectionInfo {
    pub id: String,
    pub start: usize,
    pub count: usize,
    pub vehicle_ids: Vec<String>,
    pub front: Option<(String, f64)>,
    pub rear: Option<(String, f64)>,
}

fn reordered<T: Clone>(values: &mut Vec<T>, order: &[usize], total: usize) {
    if values.len() == total {
        *values = order.iter().map(|&i| values[i].clone()).collect();
    }
}
fn indexed<T>(
    values: &mut Vec<T>,
    inverse: &[Option<usize>],
    index: impl Fn(&mut T) -> &mut usize,
) {
    let mut entries = Vec::new();
    for mut value in values.drain(..) {
        let i = index(&mut value);
        if let Some(Some(next)) = inverse.get(*i) {
            *i = *next;
            entries.push((*next, value));
        }
    }
    entries.sort_by_key(|(i, _)| *i);
    *values = entries.into_iter().map(|(_, v)| v).collect();
}

impl LiveDriveSession {
    pub(crate) fn normalize_boiler_inventory(&mut self) {
        if self.state.steam_boilers.is_empty()
            && let Some(boiler) = &self.state.boiler_state
            && let Some((vehicle, params)) =
                self.consist
                    .vehicles
                    .iter()
                    .enumerate()
                    .find_map(|(i, v)| match v {
                        openrailsrs_train::Vehicle::Loco(l) => l.steam.as_ref().map(|p| (i, p)),
                        _ => None,
                    })
        {
            self.state
                .steam_boilers
                .push(crate::steam_equipment::VehicleBoiler {
                    vehicle,
                    params: params.clone(),
                    boiler: boiler.clone(),
                });
        }
    }
    /// Prepare transactions without touching clocks, controls or the live game.
    /// A managed C# host cannot be cloned and remains an explicit compatibility limit.
    pub(crate) fn clone_for_sections(&self) -> Result<Self, String> {
        if self.script_tcs.is_some() {
            return Err("La transferencia de porciones requiere TCS Rust".into());
        }
        Ok(Self {
            activity_external_groups: self.activity_external_groups.clone(),
            scenario_name: self.scenario_name.clone(),
            formation: self.formation.clone(),
            curve_parameters: self.curve_parameters.clone(),
            intake_points: self.intake_points.clone(),
            refilling: self.refilling.clone(),
            signal_overrides: self.signal_overrides.clone(),
            service_id: self.service_id.clone(),
            native_signals: self.native_signals.clone(),
            external_track_occupancy: self.external_track_occupancy.clone(),
            dispatcher: self.dispatcher.clone(),
            own_track_reservations: self.own_track_reservations.clone(),
            external_track_reservations: self.external_track_reservations.clone(),
            external_occupancy: self.external_occupancy.clone(),
            traffic_brake_assistance: self.traffic_brake_assistance,
            consist: self.consist.clone(),
            original_physics: self.original_physics.clone(),
            original_couplers: self.original_couplers.clone(),
            authored_couplers: self.authored_couplers.clone(),
            original_cab_equipment: self.original_cab_equipment.clone(),
            original_vehicle_systems: self.original_vehicle_systems.clone(),
            cab_max_current_a: self.cab_max_current_a,
            cab_vehicle_index: self.cab_vehicle_index,
            content_signature: self.content_signature.clone(),
            stop_offsets: self.stop_offsets.clone(),
            operational_legs: self.operational_legs.clone(),
            operational_leg: self.operational_leg,
            operational_operation: self.operational_operation,
            operational_prefix_m: self.operational_prefix_m,
            operational_start_m: self.operational_start_m,
            operational_wait_started: self.operational_wait_started,
            parked_path_data: self
                .parked_path_data
                .as_ref()
                .map(|_| PathData::from_path(&self.formation.parked_path_edges, &self.graph)),
            section_path_data: self.section_path_data.clone(),
            service_clock_start_s: self.service_clock_start_s,
            state: self.state.clone(),
            physics: self.physics.clone(),
            path_data: PathData::from_path(&self.state.path_edges, &self.graph),
            graph: self.graph.clone(),
            dt: self.dt,
            assume_signals_clear: self.assume_signals_clear,
            signal_runtime: self.signal_runtime.clone(),
            gameplay: self.gameplay.clone(),
            region_tracker: self.region_tracker.clone(),
            driver_throttle: self.driver_throttle,
            driver_brake: self.driver_brake,
            driver_direction: self.driver_direction,
            exterior: self.exterior.clone(),
            horn_pressed_until_s: self.horn_pressed_until_s,
            wiper_active: self.wiper_active,
            wiper_mode: self.wiper_mode,
            headlights: self.headlights,
            cab_light: self.cab_light,
            speed_mul: self.speed_mul,
            sim_time_remainder: self.sim_time_remainder,
            previous_render_chainage_m: self.previous_render_chainage_m,
            signal_steps: self.signal_steps,
            arrived: self.arrived,
            start_chainage_m: self.start_chainage_m,
            script_tcs: None,
        })
    }
}

impl LiveDriveSession {
    pub(crate) fn vehicle_ids(&self) -> Vec<String> {
        if self.formation.vehicle_ids.len() == self.formation.cars.len() {
            self.formation.vehicle_ids.clone()
        } else {
            (0..self.formation.cars.len())
                .map(|i| format!("{}/car/{i}", self.service_id))
                .collect()
        }
    }
    pub fn vehicle_identities(&self) -> Vec<String> {
        self.vehicle_ids()
    }
    pub fn same_physical_section(&self, a: usize, b: usize) -> bool {
        if a < self.formation.coupled_count && b < self.formation.coupled_count {
            return true;
        }
        self.formation.sections.iter().any(|s| {
            (s.start..s.start + s.count).contains(&a) && (s.start..s.start + s.count).contains(&b)
        }) || self.formation.sections.is_empty()
            && a >= self.formation.coupled_count
            && b >= self.formation.coupled_count
    }
    pub(crate) fn ensure_section_metadata(&mut self) {
        self.formation.vehicle_ids = self.vehicle_ids();
        if self.formation.sections.is_empty()
            && let Some(head) = self.formation.parked_head_chainage_m
        {
            let start = self.formation.coupled_count;
            let offset = self.formation.offset_m(start);
            let reversed = self
                .formation
                .parked_path_reversed
                .unwrap_or(self.state.path_reversed);
            let full_length: f64 = self.formation.cars.iter().map(|c| c.length_m).sum();
            self.formation.section_serial = self.formation.section_serial.max(1);
            self.formation.sections.push(DetachedSection {
                id: format!(
                    "{}/section/{}",
                    self.service_id, self.formation.section_serial
                ),
                start,
                count: self.formation.cars.len() - start,
                anchor_m: head
                    + if reversed {
                        -full_length - offset
                    } else {
                        offset
                    },
                path_edges: if self.formation.parked_path_edges.is_empty() {
                    self.state.path_edges.clone()
                } else {
                    self.formation.parked_path_edges.clone()
                },
                reversed,
            });
        }
        self.rebuild_section_paths();
    }
    pub(crate) fn rebuild_section_paths(&mut self) {
        self.section_path_data = self
            .formation
            .sections
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    PathData::from_path(&s.path_edges, &self.graph),
                )
            })
            .collect();
    }
    pub(crate) fn sync_legacy_section_anchor(&mut self) {
        if let Some(s) = self.formation.sections.first() {
            let offset = self.formation.offset_m(s.start);
            let total: f64 = self.formation.cars.iter().map(|c| c.length_m).sum();
            self.formation.parked_head_chainage_m =
                Some(s.anchor_m - if s.reversed { -total - offset } else { offset });
            self.formation.parked_path_edges = s.path_edges.clone();
            self.formation.parked_path_reversed = Some(s.reversed);
            self.parked_path_data = Some(PathData::from_path(&s.path_edges, &self.graph));
        } else {
            self.formation.parked_head_chainage_m = None;
            self.formation.parked_path_edges.clear();
            self.formation.parked_path_reversed = None;
            self.parked_path_data = None;
        }
    }
    pub fn detached_sections(&self) -> Vec<DetachedSectionInfo> {
        let ids = self.vehicle_ids();
        self.formation
            .sections
            .iter()
            .map(|s| {
                let length: f64 = self.formation.cars[s.start..s.start + s.count]
                    .iter()
                    .map(|c| c.length_m)
                    .sum();
                let pd = PathData::from_path(&s.path_edges, &self.graph);
                DetachedSectionInfo {
                    id: s.id.clone(),
                    start: s.start,
                    count: s.count,
                    vehicle_ids: ids[s.start..s.start + s.count].to_vec(),
                    front: PathData::position_at_odometer(&s.path_edges, &pd.edges, s.anchor_m),
                    rear: PathData::position_at_odometer(
                        &s.path_edges,
                        &pd.edges,
                        s.anchor_m + if s.reversed { length } else { -length },
                    ),
                }
            })
            .collect()
    }
    pub fn section_vehicle_indices(&self, id: &str) -> Result<Vec<usize>, String> {
        let s = self
            .formation
            .sections
            .iter()
            .find(|s| s.id == id)
            .ok_or("Sección desconocida")?;
        Ok((s.start..s.start + s.count).collect())
    }
    pub fn section_track_occupancy(
        &self,
        id: &str,
    ) -> Result<Vec<crate::native_signals::TrackOccupancy>, String> {
        self.section_vehicle_indices(id)?;
        let owner = format!("{} · sección {}", self.service_id, id);
        Ok(self
            .own_track_occupancy()
            .into_iter()
            .filter(|p| p.owner == owner)
            .collect())
    }
    pub(crate) fn section_for_offset(&self, car_offset: f64) -> Option<&DetachedSection> {
        self.formation.sections.iter().find(|s| {
            let first = self.formation.offset_m(s.start);
            let last = self.formation.offset_m(s.start + s.count);
            car_offset <= first + 0.001 && car_offset > last + 0.001
        })
    }
    pub(crate) fn section_chainage(&self, section: &DetachedSection, offset: f64) -> f64 {
        let local = offset - self.formation.offset_m(section.start);
        section.anchor_m + if section.reversed { -local } else { local }
    }
    pub(crate) fn snapshot_order(
        &self,
        saved: &crate::SessionSnapshot,
    ) -> Result<Vec<usize>, String> {
        let current = self.vehicle_ids();
        let desired = if saved.formation.vehicle_ids.is_empty() {
            current.clone()
        } else {
            saved.formation.vehicle_ids.clone()
        };
        if desired.len() != current.len()
            || desired.iter().collect::<HashSet<_>>().len() != desired.len()
        {
            return Err("Identidades de vehículos guardadas duplicadas o incompletas".into());
        }
        desired
            .iter()
            .map(|id| {
                current
                    .iter()
                    .position(|v| v == id)
                    .ok_or_else(|| "La partida contiene un vehículo de otra formación".into())
            })
            .collect()
    }
    pub(crate) fn validate_sections(
        &self,
        formation: &crate::FormationState,
    ) -> Result<(), String> {
        let ids = if formation.vehicle_ids.is_empty() {
            self.vehicle_ids()
        } else {
            formation.vehicle_ids.clone()
        };
        if formation.native_bearing_c.len() > ids.len()
            || formation.native_axles.len() > ids.len()
            || formation
                .native_bearing_c
                .iter()
                .any(|(id, t)| !ids.contains(id) || !t.is_finite() || !(-100. ..=500.).contains(t))
            || formation.native_axles.iter().any(|(id, a)| {
                !ids.contains(id) || !a.speed_mps.is_finite() || a.speed_mps.abs() > 1000.
            })
        {
            return Err("Estado nativo de una sección guardada inválido".into());
        }
        if formation.sections.is_empty() {
            return Ok(());
        }
        if formation.sections.len() > 256 || formation.section_serial > 1_000_000_000 {
            return Err("Cantidad de secciones guardadas inválida".into());
        }
        let mut next = formation.coupled_count;
        let mut ids = HashSet::new();
        for s in &formation.sections {
            if s.id.trim().is_empty()
                || s.id.len() > 256
                || !ids.insert(&s.id)
                || s.start != next
                || s.count == 0
                || s.start
                    .checked_add(s.count)
                    .is_none_or(|n| n > formation.cars.len())
                || !s.anchor_m.is_finite()
                || s.anchor_m < 0.
                || s.path_edges.is_empty()
                || s.path_edges.len() > 50000
            {
                return Err("Identidad, rango o posición de sección inválidos".into());
            }
            for pair in s.path_edges.windows(2) {
                if self
                    .graph
                    .edge(&pair[0])
                    .zip(self.graph.edge(&pair[1]))
                    .is_none_or(|(a, b)| a.to != b.from)
                {
                    return Err("La sección guardada salta entre vías".into());
                }
            }
            if s.path_edges.iter().any(|id| self.graph.edge(id).is_none()) {
                return Err("Vía de sección guardada inexistente".into());
            }
            let pd = PathData::from_path(&s.path_edges, &self.graph);
            let length: f64 = formation.cars[s.start..s.start + s.count]
                .iter()
                .map(|c| c.length_m)
                .sum();
            let rear = s.anchor_m + if s.reversed { length } else { -length };
            if s.anchor_m > pd.total_length_m() + 1.
                || rear < -1.
                || rear > pd.total_length_m() + 1.
            {
                return Err("La sección guardada está fuera de su vía".into());
            }
            next += s.count;
        }
        if next != formation.cars.len() {
            return Err("Faltan vehículos en las secciones guardadas".into());
        }
        Ok(())
    }

    /// Change immutable indexing only. Useful on a disposable validation view;
    /// the live state's pressure, fuel and kinematics are supplied by the save.
    pub(crate) fn reorder_hardware(&mut self, order: &[usize]) -> Result<(), String> {
        let total = self.consist.vehicles.len();
        let mut inverse = vec![None; total];
        for (next, &old) in order.iter().enumerate() {
            if old >= total || inverse[old].replace(next).is_some() {
                return Err("Orden de formación inválido".into());
            }
        }
        if order.is_empty() {
            return Err("La formación no puede quedar vacía".into());
        }
        let old_couplers = self.original_couplers.clone();
        let old_diesel = self.original_physics.diesel_vehicle_indices.clone();
        reordered(&mut self.consist.vehicles, order, total);
        reordered(&mut self.curve_parameters, order, total);
        reordered(&mut self.intake_points, order, total);
        reordered(&mut self.authored_couplers, order, total);
        indexed(&mut self.original_vehicle_systems, &inverse, |v| {
            &mut v.vehicle
        });
        indexed(&mut self.state.freight, &inverse, |v| &mut v.vehicle);
        indexed(&mut self.original_cab_equipment, &inverse, |v| {
            &mut v.vehicle
        });
        let p = &mut self.original_physics;
        let previous_mass = p.mass_kg;
        let previous_davis = p.davis.clone();
        reordered(&mut p.bogies, order, total);
        reordered(&mut p.vehicle_davis, order, total);
        reordered(&mut p.vehicle_lengths_m, order, total);
        reordered(&mut p.power_supply.bus_connected, order, total);
        reordered(&mut p.power_supply.passenger_loads, order, total);
        indexed(&mut p.power_supply.cars, &inverse, |v| &mut v.vehicle);
        indexed(&mut p.diesel.cars, &inverse, |v| &mut v.vehicle);
        indexed(&mut p.electric.cars, &inverse, |v| &mut v.vehicle);
        p.electric.fallback_cars.retain_mut(|(i, _)| {
            if let Some(Some(j)) = inverse.get(*i) {
                *i = *j;
                true
            } else {
                false
            }
        });
        p.electric.fallback_cars.sort_by_key(|(i, _)| *i);
        if let Some(native) = &mut p.native {
            reordered(&mut native.vehicles, order, total);
            reordered(&mut native.vehicle_masses_kg, order, total);
            native.control_vehicle_index = inverse
                .get(native.control_vehicle_index)
                .and_then(|i| *i)
                .unwrap_or(0);
        }
        if let Some(rail) = &mut p.rail_adhesion {
            reordered(&mut rail.vehicles, order, total);
        }
        let engine_order: Vec<_> = order
            .iter()
            .filter_map(|i| old_diesel.iter().position(|j| j == i))
            .collect();
        reordered(&mut p.diesel_engines, &engine_order, old_diesel.len());
        p.diesel_vehicle_indices = self.consist.diesel_vehicle_indices();
        p.mass_kg = self.consist.total_mass_kg();
        p.max_power_w = self.consist.total_max_power_w();
        p.max_tractive_effort_n = self.consist.total_max_tractive_effort_n();
        p.max_brake_n = self.consist.total_max_brake_n();
        p.electric.length_m = self.consist.vehicle_lengths_m().iter().sum();
        let mut offset = 0.;
        for (i, v) in self.consist.vehicles.iter().enumerate() {
            for c in &mut p.electric.cars {
                if c.vehicle == i {
                    c.offset_m = offset;
                }
            }
            offset -= match v {
                openrailsrs_train::Vehicle::Loco(l) => l.length_m,
                openrailsrs_train::Vehicle::Wagon(w) => w.length_m,
            };
        }
        p.steam_params = self.consist.aggregate_steam_params();
        p.tractive = self.consist.aggregate_tractive_curve();
        if order.len() != total {
            p.davis = p.vehicle_davis.iter().fold(
                openrailsrs_train::DavisCoefficients {
                    a_n: 0.,
                    b_n_per_mps: 0.,
                    c_n_per_mps2: 0.,
                },
                |a, b| a.sum(b),
            );
        }
        if order.len() != total
            && p.davis.a_n == 0.
            && p.davis.b_n_per_mps == 0.
            && p.davis.c_n_per_mps2 == 0.
            && previous_mass > 0.
        {
            let fraction = p.mass_kg / previous_mass;
            p.davis = openrailsrs_train::DavisCoefficients {
                a_n: previous_davis.a_n * fraction,
                b_n_per_mps: previous_davis.b_n_per_mps * fraction,
                c_n_per_mps2: previous_davis.c_n_per_mps2 * fraction,
            };
        }
        p.regen_factor = self.consist.regen_factor();
        p.diesel_sfc_g_per_kwh = self.consist.diesel_sfc_g_per_kwh();
        self.original_couplers = if old_couplers.is_empty() {
            vec![]
        } else {
            order
                .windows(2)
                .enumerate()
                .map(|(i, pair)| {
                    if pair[1] == pair[0] + 1 {
                        old_couplers[pair[0]].clone()
                    } else {
                        self.authored_couplers
                            .get(i)
                            .zip(self.authored_couplers.get(i + 1))
                            .and_then(|(a, b)| CouplerState::from_vehicle_ends(a, b))
                            .unwrap_or_else(|| {
                                let mut c = old_couplers
                                    .iter()
                                    .find(|c| c.native_profile.is_none())
                                    .cloned()
                                    .unwrap_or_else(CouplerState::passenger);
                                c.extension_m = 0.;
                                c.memory = Default::default();
                                c.broken = false;
                                c
                            })
                    }
                })
                .collect()
        };
        self.cab_vehicle_index = inverse
            .get(self.cab_vehicle_index)
            .and_then(|i| *i)
            .unwrap_or(0);
        self.state.articulated_joints = self
            .state
            .articulated_joints
            .iter()
            .filter_map(|p| {
                inverse[p[0]]
                    .zip(inverse[p[1]])
                    .map(|(a, b)| [a.min(b), a.max(b)])
            })
            .collect();
        Ok(())
    }

    fn native_active_diesel_indices(&self) -> Vec<usize> {
        let cab = self.cab_vehicle();
        self.original_physics
            .diesel_vehicle_indices
            .iter()
            .copied()
            .filter(|&i| {
                i < self.formation.coupled_count
                    && self
                        .formation
                        .cars
                        .get(i)
                        .is_some_and(|c| c.power_on && c.battery_on && (i == cab || c.mu_connected))
            })
            .collect()
    }
    pub(crate) fn remember_native_vehicle_states(&mut self) {
        let ids = self.vehicle_ids();
        let diesel = self.physics.diesel_vehicle_indices.clone();
        if let Some(d) = &self.state.native_dynamics {
            for (id, &temperature) in ids.iter().zip(&d.bearing_c) {
                self.formation
                    .native_bearing_c
                    .insert(id.clone(), temperature);
            }
            for (&vehicle, axle) in diesel.iter().zip(&d.axles) {
                if let Some(id) = ids.get(vehicle) {
                    self.formation.native_axles.insert(id.clone(), axle.clone());
                }
            }
        }
    }
    pub(crate) fn align_native_vehicle_states(&mut self) {
        let ids = self.vehicle_ids();
        self.formation
            .native_bearing_c
            .retain(|id, _| ids.contains(id));
        self.formation.native_axles.retain(|id, _| ids.contains(id));
        let diesel = self.native_active_diesel_indices();
        if let Some(d) = &mut self.state.native_dynamics {
            let ambient = self
                .original_physics
                .native
                .as_ref()
                .map_or(self.state.ambient_temperature_c, |p| {
                    p.environment.ambient_c
                });
            d.bearing_c = ids
                .iter()
                .take(self.formation.coupled_count)
                .map(|id| {
                    self.formation
                        .native_bearing_c
                        .get(id)
                        .copied()
                        .unwrap_or(ambient)
                })
                .collect();
            d.axles = diesel
                .iter()
                .map(|&i| {
                    self.formation.native_axles.get(&ids[i]).cloned().unwrap_or(
                        crate::native_dynamics::NativeAxleState {
                            speed_mps: self.state.velocity_mps,
                        },
                    )
                })
                .collect();
        }
        self.physics = self.original_physics.clone();
        self.physics.diesel_vehicle_indices = diesel;
    }

    pub(crate) fn permute_catalog(&mut self, order: &[usize], active: usize) -> Result<(), String> {
        self.remember_native_vehicle_states();
        let total = self.formation.cars.len();
        if active == 0 || active > order.len() {
            return Err("Cantidad activa de coches inválida".into());
        }
        let mut inverse = vec![None; total];
        for (next, &old) in order.iter().enumerate() {
            if old >= total || inverse[old].replace(next).is_some() {
                return Err("Orden de coches inválido".into());
            }
        }
        let old_diesel = self.original_physics.diesel_vehicle_indices.clone();
        let engine_order: Vec<_> = order
            .iter()
            .filter_map(|i| old_diesel.iter().position(|j| j == i))
            .collect();
        let mut brakes = self.state.brake_system.cylinders.clone();
        brakes.extend(self.formation.parked_brakes.clone());
        let mut bodies = self.state.vehicles.clone();
        bodies.extend(self.formation.parked_vehicles.clone());
        let mut joints = self.state.couplers.clone();
        joints.extend(self.formation.parked_couplers.clone());
        let mut bogies = self.state.bogies.clone();
        bogies.extend(self.formation.parked_bogies.clone());
        let old_ids = self.vehicle_ids();
        self.reorder_hardware(order)?;
        reordered(&mut self.formation.cars, order, total);
        self.formation.vehicle_ids = order.iter().map(|&i| old_ids[i].clone()).collect();
        reordered(&mut brakes, order, total);
        let mut offset = 0.;
        for (i, c) in brakes.iter_mut().enumerate() {
            c.position_m = offset;
            offset += self.formation.cars[i].length_m;
        }
        self.formation.parked_brakes = brakes.split_off(active);
        self.state.brake_system.cylinders = brakes;
        if !bodies.is_empty() {
            reordered(&mut bodies, order, total);
            let next_joints: Vec<_> = order
                .windows(2)
                .enumerate()
                .map(|(i, pair)| {
                    if pair[1] == pair[0] + 1 {
                        joints[pair[0]].clone()
                    } else {
                        self.original_couplers[i].clone()
                    }
                })
                .collect();
            let mut next_joints = next_joints;
            self.formation.parked_couplers = next_joints.split_off(active - 1);
            self.state.couplers = next_joints;
            self.formation.parked_vehicles = bodies.split_off(active);
            self.state.vehicles = bodies;
            self.state.vehicle_masses = self.formation.cars[..active]
                .iter()
                .map(|c| c.mass_kg)
                .collect();
        }
        let s = &mut self.state;
        s.active_cab_vehicle = s
            .active_cab_vehicle
            .and_then(|i| inverse.get(i).and_then(|j| *j));
        for values in [
            &mut s.diesel_rpm,
            &mut s.diesel_run_up,
            &mut s.diesel_motor_heat,
            &mut s.diesel_traction_force_n,
            &mut s.diesel_average_force_n,
            &mut s.diesel_apparent_throttle,
        ] {
            reordered(values, &engine_order, old_diesel.len());
        }
        reordered(&mut bogies, order, total);
        if !bogies.is_empty() {
            self.formation.parked_bogies = bogies.split_off(active);
            s.bogies = bogies;
        }
        reordered(&mut s.motor_force_n, order, total);
        reordered(&mut s.rail_force_n, order, total);
        indexed(&mut s.brake_system.circuits, &inverse, |v| &mut v.vehicle);
        indexed(&mut s.steam_boilers, &inverse, |v| &mut v.vehicle);
        indexed(&mut s.diesel.cars, &inverse, |v| &mut v.vehicle);
        indexed(&mut s.electric.cars, &inverse, |v| &mut v.vehicle);
        indexed(&mut s.power_supply.cars, &inverse, |v| &mut v.vehicle);
        s.power_supply.bus_source.clear();
        s.power_supply.source_hotel_w.clear();
        indexed(&mut s.vehicle_systems, &inverse, |v| {
            &mut v.definition.vehicle
        });
        indexed(&mut s.cab_equipment, &inverse, |v| &mut v.config.vehicle);
        if let Some(d) = &mut s.native_dynamics {
            reordered(&mut d.bearing_c, order, total);
            reordered(&mut d.axles, &engine_order, old_diesel.len());
            d.bearing_c.truncate(active);
        }
        if let Some(r) = &mut s.rail_adhesion {
            reordered(&mut r.cars, order, total);
        }
        if !s.steam_boilers.is_empty() {
            s.boiler_state = s
                .steam_boilers
                .iter()
                .find(|b| b.vehicle == s.active_cab_vehicle.unwrap_or(self.cab_vehicle_index))
                .map(|b| b.boiler.clone());
        }
        self.formation.coupled_count = active;
        if self.state.active_cab_vehicle.is_none()
            && matches!(
                self.consist.vehicles.get(self.cab_vehicle_index),
                Some(openrailsrs_train::Vehicle::Loco(_))
            )
        {
            self.state.active_cab_vehicle = Some(self.cab_vehicle_index);
        }
        self.align_native_vehicle_states();
        Ok(())
    }
}

impl LiveDriveSession {
    pub fn uncouple_after_named(&mut self, index: usize, id: &str) -> Result<(), String> {
        let keep = index.checked_add(1).ok_or("Coche inválido")?;
        let old = self.formation.coupled_count;
        if id.trim().is_empty()
            || id.len() > 256
            || self.formation.sections.iter().any(|s| s.id == id)
        {
            return Err("La identidad de sección está vacía o ya existe".into());
        }
        if self
            .state
            .articulated_joints
            .iter()
            .any(|p| p[0] == index && p[1] == keep)
        {
            return Err("No se pueden separar cuerpos que comparten un bogie Jacobs".into());
        }
        if keep <= self.cab_vehicle() {
            return Err(
                "Cambiá a una cabina de la sección que vas a conservar antes de desacoplar".into(),
            );
        }
        if self.velocity_mps().abs() > 0.1 {
            return Err("Detené el tren antes de desacoplar".into());
        }
        if keep >= old {
            return Err("Seleccioná un coche que tenga otro detrás".into());
        }
        if self.formation.cars[keep..old].iter().any(|c| !c.handbrake) {
            return Err("Aplicá los frenos de mano de todos los coches que vas a separar".into());
        }
        let head = self.head_chainage_m();
        let full_length = self.formation.length_m();
        let path_length = self.path_data.total_length_m();
        // A saved section needs real track under both of its ends. Check the
        // whole active footprint before remembering state or allocating its ID:
        // clipping a negative anchor would move the parked cars at the cut.
        if !head.is_finite()
            || !full_length.is_finite()
            || !path_length.is_finite()
            || self
                .path_data
                .edges
                .get(self.state.edge_index)
                .is_none_or(|edge| {
                    !self.state.pos_on_edge_m.is_finite()
                        || self.state.pos_on_edge_m < 0.
                        || self.state.pos_on_edge_m > edge.length_m
                })
            || full_length <= 0.
            || head - full_length < 0.
            || head > path_length
            || self.state.path_edges.is_empty()
            || self
                .state
                .path_edges
                .iter()
                .any(|id| self.graph.edge(id).is_none())
            || self.state.path_edges.windows(2).any(|pair| {
                self.graph
                    .edge(&pair[0])
                    .zip(self.graph.edge(&pair[1]))
                    .is_none_or(|(a, b)| a.to != b.from)
            })
        {
            return Err(
                "No hay vía suficiente bajo toda la formación para desacoplar sin desplazar coches"
                    .into(),
            );
        }
        let prefix: f64 = self.formation.cars[..keep].iter().map(|c| c.length_m).sum();
        let reversed = self.state.path_reversed;
        let section = DetachedSection {
            id: id.into(),
            start: keep,
            count: old - keep,
            anchor_m: head
                + if reversed {
                    -full_length + prefix
                } else {
                    -prefix
                },
            path_edges: self.state.path_edges.clone(),
            reversed,
        };
        self.remember_native_vehicle_states();
        self.ensure_section_metadata();
        self.driver_throttle = 0.;
        self.formation.section_serial += 1;
        self.formation.sections.push(section);
        self.formation.sections.sort_by_key(|s| s.start);
        if self.state.bogies.len() == old {
            let mut bogies = self.state.bogies.split_off(keep);
            bogies.append(&mut self.formation.parked_bogies);
            self.formation.parked_bogies = bogies;
        }
        let mut brakes = self.state.brake_system.cylinders.split_off(keep);
        brakes.append(&mut self.formation.parked_brakes);
        self.formation.parked_brakes = brakes;
        if !self.state.vehicles.is_empty() {
            let mut cars = self.state.vehicles.split_off(keep);
            cars.append(&mut self.formation.parked_vehicles);
            self.formation.parked_vehicles = cars;
            let mut couplers = self.state.couplers.split_off(keep - 1);
            if let Some(c) = couplers.first_mut() {
                c.extension_m = 0.;
                c.memory = Default::default();
            }
            couplers.append(&mut self.formation.parked_couplers);
            self.formation.parked_couplers = couplers;
            self.state.vehicle_masses.truncate(keep);
        }
        self.formation.coupled_count = keep;
        self.formation.cars[index].rear_cock_open = false;
        self.formation.cars[keep].front_cock_open = false;
        self.formation.cars[keep].hose_connected = false;
        if reversed {
            let odometer = self.state.odometer_m;
            crate::live_drive::apply_start_offset(
                &mut self.state,
                &self.path_data,
                head - (full_length - prefix),
            );
            self.state.odometer_m = odometer;
            self.previous_render_chainage_m = self.head_chainage_m();
        }
        self.sync_legacy_section_anchor();
        self.rebuild_section_paths();
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
        self.update_shunting();
        self.own_track_reservations.clear();
        self.native_signals.needs_refresh = true;
        Ok(())
    }

    fn physical_position_equal(&self, a: Option<(String, f64)>, b: Option<(String, f64)>) -> bool {
        a.zip(b).is_some_and(|(a, b)| {
            let normalize = |(id, p): (String, f64)| {
                self.graph.edge(&id).map(|e| {
                    (
                        id.strip_suffix("_r").unwrap_or(&id).to_string(),
                        if id.ends_with("_r") {
                            e.length_m - p
                        } else {
                            p
                        },
                    )
                })
            };
            normalize(a)
                .zip(normalize(b))
                .is_some_and(|(a, b)| a.0 == b.0 && (a.1 - b.1).abs() <= 1.)
        })
    }
    pub fn validate_section_pickup(&self, id: &str, end: CouplingEnd) -> Result<f64, String> {
        if self.velocity_mps().abs() > 0.1 || self.driver_throttle != 0. {
            return Err("Detené el tren y quitá la tracción antes de acoplar".into());
        }
        let section = self
            .formation
            .sections
            .iter()
            .find(|s| s.id == id)
            .ok_or("Sección desconocida")?;
        let n = self.formation.coupled_count;
        let prefix = self.formation.length_m();
        let length: f64 = self.formation.cars[section.start..section.start + section.count]
            .iter()
            .map(|c| c.length_m)
            .sum();
        if !self.formation.cars[section.start..section.start + section.count]
            .iter()
            .all(|c| c.handbrake)
        {
            return Err("Asegurá con freno de mano todos los coches de la sección".into());
        }
        let own_i = if end == CouplingEnd::Rear { n - 1 } else { 0 };
        let section_i = if end == CouplingEnd::Rear {
            section.start
        } else {
            section.start + section.count - 1
        };
        let own_brake = &self.state.brake_system.cylinders[own_i];
        let section_brake = &self.formation.parked_brakes[section_i - n];
        if !own_brake.connection_compatible(section_brake) {
            return Err("Los equipos de freno de ambos extremos no son compatibles".into());
        }
        let anchor = self.head_chainage_m()
            + match (end, self.state.path_reversed) {
                (CouplingEnd::Rear, true) | (CouplingEnd::Front, false) => length,
                _ => 0.,
            };
        let total = prefix + length;
        let mut local = 0.;
        for i in section.start..section.start + section.count {
            let car = &self.formation.cars[i];
            let offset = if end == CouplingEnd::Rear {
                -prefix + local - car.length_m * 0.5
            } else {
                local - car.length_m * 0.5
            };
            let next = PathData::position_at_odometer(
                &self.state.path_edges,
                &self.path_data.edges,
                anchor
                    + if self.state.path_reversed {
                        -total - offset
                    } else {
                        offset
                    },
            );
            let old_offset = self.formation.center_offset_m(i);
            let old = self.presentation_position_for_car(old_offset, old_offset, 0.);
            if !self.physical_position_equal(old, next) {
                return Err("Acercá los enganches a menos de 1 m: la vía actual debe conservar la posición de todos los coches".into());
            }
            local -= car.length_m;
        }
        Ok(anchor)
    }
    pub fn recouple_section(&mut self, id: &str, end: CouplingEnd) -> Result<(), String> {
        self.ensure_section_metadata();
        let anchor = self.validate_section_pickup(id, end)?;
        let section = self
            .formation
            .sections
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .clone();
        let n = self.formation.coupled_count;
        let total = self.formation.cars.len();
        let active = n + section.count;
        let mut order: Vec<_> = if end == CouplingEnd::Rear {
            (0..n)
                .chain(section.start..section.start + section.count)
                .collect()
        } else {
            (section.start..section.start + section.count)
                .chain(0..n)
                .collect()
        };
        order.extend(
            (n..total).filter(|i| *i < section.start || *i >= section.start + section.count),
        );
        let mut inverse = vec![0; total];
        for (next, &old) in order.iter().enumerate() {
            inverse[old] = next;
        }
        let mut remaining: Vec<_> = self
            .formation
            .sections
            .iter()
            .filter(|s| s.id != id)
            .cloned()
            .collect();
        for s in &mut remaining {
            s.start = inverse[s.start];
        }
        remaining.sort_by_key(|s| s.start);
        self.permute_catalog(&order, active)?;
        self.formation.sections = remaining;
        let boundary = if end == CouplingEnd::Rear {
            n - 1
        } else {
            section.count - 1
        };
        self.formation.cars[boundary].rear_cock_open = false;
        self.formation.cars[boundary + 1].front_cock_open = false;
        self.formation.cars[boundary + 1].hose_connected = false;
        self.formation.cars[boundary + 1].train_bus_connected = false;
        if !self.state.vehicles.is_empty() {
            let shift = self.state.vehicles[boundary].position_m
                - self.state.vehicles[boundary + 1].position_m;
            for c in &mut self.state.vehicles[boundary + 1..] {
                c.position_m += shift;
                c.velocity_mps = 0.;
            }
            self.state.couplers[boundary].extension_m = 0.;
            self.state.couplers[boundary].memory = Default::default();
        }
        let odometer = self.state.odometer_m;
        crate::live_drive::apply_start_offset(&mut self.state, &self.path_data, anchor);
        self.state.odometer_m = odometer;
        self.previous_render_chainage_m = anchor;
        self.sync_legacy_section_anchor();
        self.rebuild_section_paths();
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
        self.update_shunting();
        self.own_track_reservations.clear();
        self.native_signals.needs_refresh = true;
        Ok(())
    }
}

impl LiveDriveSession {
    pub(crate) fn project_portion(&mut self, indices: &[usize]) -> Result<(), String> {
        self.permute_catalog(indices, indices.len())?;
        self.formation.sections.clear();
        self.formation.parked_brakes.clear();
        self.formation.parked_vehicles.clear();
        self.formation.parked_couplers.clear();
        self.formation.parked_bogies.clear();
        self.sync_legacy_section_anchor();
        self.rebuild_section_paths();
        self.own_track_reservations.clear();
        self.dispatcher.own_locks.clear();
        self.refilling = None;
        if self.original_physics.steam_params.is_none() {
            self.state.boiler_state = None;
        }
        self.rebuild_formation_physics();
        self.sync_operating_brakes();
        Ok(())
    }
}

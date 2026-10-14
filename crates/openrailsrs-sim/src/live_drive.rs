//! Real-time single-train session for interactive viewers (`openrailsrs-viewer3d --live`).

use std::collections::HashMap;
use std::path::Path;

use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_scenarios::{RegionTracker, RegionTransition, ScenarioFile};
use openrailsrs_track::{NodeKind, SignalAspect, TrackGraph};
use openrailsrs_train::{DavisCoefficients, TractiveCurve, load_consist_with_asset_root};

use crate::SimError;
use crate::brake::BrakeSystem;
use crate::coupler::CouplerKind;
use crate::exterior::RollingStockExteriorState;
use crate::path::resolve_route_edges;
use crate::path_data::PathData;
use crate::physics::{TrainPhysics, max_partial_throttle_run_up_time_s, step};
use crate::runner::consist_root;
pub use crate::service::{LiveGameplay, LiveStopTarget};
use crate::service::{STOP_POSITION_TOLERANCE_M, STOP_SPEED_TOLERANCE_MPS, ServicePhase};
use crate::state::TrainSimState;

const BRAKE_PIPE_SPEED_MPS: f64 = 200.0;
/// Caution signals halve the effective speed limit on the signalled edge (same as headless runner).
const CAUTION_SPEED_FACTOR: f64 = 0.5;

fn build_brake_system(
    consist: &openrailsrs_train::Consist,
    train_air_lap_hold: bool,
    train_air_full_release_s: f64,
    brake_shoe_speed_factor: bool,
    brake_skid_limit: bool,
) -> BrakeSystem {
    let specs = crate::brake::vehicle_specs_from_consist(
        consist,
        brake_shoe_speed_factor,
        brake_skid_limit,
    );
    BrakeSystem::from_vehicle_specs(
        &specs,
        BRAKE_PIPE_SPEED_MPS,
        train_air_lap_hold,
        train_air_full_release_s,
    )
}

pub(crate) fn apply_start_offset(state: &mut TrainSimState, path_data: &PathData, offset_m: f64) {
    let mut remaining = offset_m.max(0.0);
    state.pos_on_edge_m = 0.0;
    state.odometer_m = 0.0;
    state.edge_index = 0;
    while remaining > 0.0 && state.edge_index < state.path_edges.len() {
        let Some(edge) = path_data.get(state.edge_index) else {
            break;
        };
        if edge.length_m <= 0.0 {
            state.edge_index += 1;
            continue;
        }
        if remaining <= edge.length_m {
            state.pos_on_edge_m = remaining;
            return;
        }
        remaining -= edge.length_m;
        state.edge_index += 1;
    }
}

fn init_signal_runtime(
    graph: &TrackGraph,
    assume_signals_clear: bool,
) -> HashMap<String, SignalAspect> {
    if assume_signals_clear {
        graph
            .signals()
            .map(|s| (s.id.clone(), SignalAspect::Clear))
            .collect()
    } else {
        graph.signals().map(|s| (s.id.clone(), s.aspect)).collect()
    }
}

fn build_live_gameplay(
    scenario: &ScenarioFile,
    graph: &TrackGraph,
    path_edges: &[String],
) -> Result<LiveGameplay, SimError> {
    let stops = &scenario.route.stops;
    let mut stop_targets = Vec::new();
    let mut cum = 0.0;
    let mut node_chainages = vec![(scenario.route.start.as_str(), 0.0)];
    for eid in path_edges {
        if let Some(edge) = graph.edge(eid) {
            cum += edge.length_m;
            node_chainages.push((edge.to.0.as_str(), cum));
        }
    }
    for stop in stops {
        let to_id = &stop.node;
        if let Some((_, distance)) = node_chainages.iter().find(|(n, _)| *n == to_id) {
            let target_distance = distance + stop.offset_m;
            if target_distance < 0.0 || target_distance > cum {
                return Err(SimError::Msg(format!(
                    "stop {to_id} lies outside the route"
                )));
            }
            let name = graph
                .node(to_id)
                .and_then(|n| {
                    if let NodeKind::Station { name } = &n.kind {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| to_id.clone());
            stop_targets.push(LiveStopTarget {
                node_id: to_id.clone(),
                cum_dist_m: target_distance,
                arrive_s: stop.arrive_s,
                depart_s: stop.depart_s,
                dwell_s: stop.dwell_s,
                name: stop.name.clone().unwrap_or(name),
                is_terminal: to_id == &scenario.route.destination,
                passengers_on: stop.passengers_on,
                passengers_off: stop.passengers_off,
            });
        } else {
            return Err(SimError::Msg(format!(
                "scheduled stop {to_id} is not on the resolved path"
            )));
        }
    }
    for pair in stop_targets.windows(2) {
        if pair[0].cum_dist_m >= pair[1].cum_dist_m || pair[0].is_terminal {
            return Err(SimError::Msg(
                "stops must follow route order; the terminal stop must be last".into(),
            ));
        }
    }
    let mut gameplay = LiveGameplay::new(
        scenario.route.destination.clone(),
        scenario.gameplay.penalty_per_second_late,
        stop_targets,
    );
    gameplay.shunting_steps = scenario.gameplay.shunting.clone();
    Ok(gameplay)
}

/// Interactive session: same physics as headless `sim` / `cab`, stepped from a real-time loop.
pub struct LiveDriveSession {
    pub scenario_name: String,
    pub formation: crate::FormationState,
    pub curve_parameters: Vec<openrailsrs_formats::VehicleCurveParameters>,
    pub intake_points: Vec<Vec<openrailsrs_formats::IntakePoint>>,
    pub refilling: Option<crate::refill::RefillOperation>,
    pub signal_overrides: HashMap<String, SignalAspect>,
    pub service_id: String,
    pub native_signals: crate::native_signals::NativeSignalRuntime,
    pub external_track_occupancy: Vec<crate::native_signals::TrackOccupancy>,
    pub dispatcher: crate::DispatcherStatus,
    pub own_track_reservations: Vec<crate::native_signals::TrackOccupancy>,
    pub external_track_reservations: Vec<crate::native_signals::TrackOccupancy>,
    /// Rebuilt by the live traffic coordinator at every physics quantum.
    pub external_occupancy: HashMap<String, String>,
    /// Optional player aid; native TCS commands remain authoritative either way.
    pub traffic_brake_assistance: bool,
    pub(crate) consist: openrailsrs_train::Consist,
    pub(crate) original_physics: TrainPhysics,
    pub(crate) original_cab_equipment: Vec<openrailsrs_scenarios::cab_equipment::CabEquipmentDef>,
    pub(crate) original_vehicle_systems:
        Vec<openrailsrs_scenarios::vehicle_systems::VehicleSystemsDef>,
    pub(crate) cab_max_current_a: Option<f64>,
    pub(crate) cab_vehicle_index: usize,
    pub(crate) content_signature: String,
    pub(crate) stop_offsets: HashMap<String, f64>,
    pub(crate) operational_legs: Vec<openrailsrs_scenarios::model::RouteLeg>,
    pub(crate) operational_leg: usize,
    pub(crate) operational_operation: usize,
    pub(crate) operational_prefix_m: f64,
    pub(crate) operational_start_m: f64,
    pub(crate) operational_wait_started: Option<f64>,
    pub(crate) parked_path_data: Option<PathData>,
    pub(crate) service_clock_start_s: f64,
    pub state: TrainSimState,
    pub physics: TrainPhysics,
    pub path_data: PathData,
    pub graph: TrackGraph,
    pub dt: f64,
    pub assume_signals_clear: bool,
    /// Runtime signal aspects (updated each step; used by 3D markers).
    pub signal_runtime: HashMap<String, SignalAspect>,
    pub gameplay: LiveGameplay,
    pub region_tracker: RegionTracker,
    /// Driver notch [0, 1] (not yet written to `state` until step).
    pub driver_throttle: f64,
    pub driver_brake: f64,
    /// Reverser: 0 = REV, 0.5 = neutral, 1 = FWD (cab CVF / HUD).
    pub driver_direction: f64,
    /// Door / pantograph presentation for exterior shape keys (#81).
    pub exterior: RollingStockExteriorState,
    /// Sim time until which horn button appears pressed (cab M5).
    pub(crate) horn_pressed_until_s: f64,
    /// Wiper switch (cab CVF TWO_STATE / EXTERNALWIPERS).
    pub wiper_active: bool,
    pub wiper_mode: crate::wiper::WiperMode,
    /// OR headlight positions: 0 off, 1 dim, 2 bright.
    pub headlights: u8,
    pub cab_light: bool,
    pub speed_mul: f64,
    pub(crate) sim_time_remainder: f64,
    pub(crate) previous_render_chainage_m: f64,
    pub(crate) signal_steps: u64,
    pub arrived: bool,
    pub start_chainage_m: f64,
    pub(crate) script_tcs: Option<crate::etcs::ScriptTcsHost>,
}

impl LiveDriveSession {
    /// Includes actual remaining diesel and passengers; authored source masses
    /// and saved kinematics stay immutable.
    pub fn effective_mass_kg(&self) -> f64 {
        crate::consumable_mass::total_mass_kg(&self.physics, &self.state)
    }

    pub fn set_rail_weather(&mut self, weather: crate::adhesion::RailWeather) {
        self.set_rail_weather_contact(weather, None);
    }

    /// Continuous normalized grip target, without advancing the rail surface
    /// or consuming sand. Wetting/drying remains on the physics clock.
    pub fn set_rail_weather_factor(&mut self, weather: crate::adhesion::RailWeather, factor: f64) {
        if factor.is_finite() {
            self.set_rail_weather_contact(weather, Some(factor.clamp(0.5, 1.)));
        }
    }

    fn set_rail_weather_contact(
        &mut self,
        weather: crate::adhesion::RailWeather,
        target: Option<f64>,
    ) {
        let Some(config) = &self.physics.rail_adhesion else {
            return;
        };
        let speed = self.state.velocity_mps;
        let dynamics = self.state.native_dynamics.as_ref();
        let rail = self.state.rail_adhesion.get_or_insert_with(|| {
            let mut rail = crate::adhesion::RailAdhesionState::new(config, weather, speed);
            if let Some(target) = target {
                rail.weather_factor = target;
                for car in &mut rail.cars {
                    car.factor = target;
                }
            }
            if let Some(dynamics) = dynamics {
                for (axle, &index) in dynamics
                    .axles
                    .iter()
                    .zip(&self.physics.diesel_vehicle_indices)
                {
                    if let Some(car) = rail.cars.get_mut(index)
                        && config.vehicles.get(index).is_some_and(|v| !v.steam)
                    {
                        car.wheel_speed_mps = axle.speed_mps;
                    }
                }
            }
            rail
        });
        rail.weather = weather;
        rail.weather_target_factor = target;
    }

    pub fn toggle_sander(&mut self) -> Result<(), String> {
        if self.state.rail_adhesion.is_none() {
            self.set_rail_weather(crate::adhesion::RailWeather::Dry);
        }
        let rail = self
            .state
            .rail_adhesion
            .as_mut()
            .ok_or("Esta formación no tiene un arenador disponible")?;
        rail.sander_command = !rail.sander_command;
        Ok(())
    }

    /// Wheel phase correction uses the same interpolation fraction as the body.
    /// Parked detached cars retain their phase; neither render frames nor pause
    /// consume sand or integrate the axle a second time.
    pub fn render_wheel_slip_distance_m(&self, vehicle: usize, remainder_s: f64) -> f64 {
        let Some(car) = self
            .state
            .rail_adhesion
            .as_ref()
            .and_then(|r| r.cars.get(vehicle))
        else {
            return 0.;
        };
        if vehicle >= self.formation.coupled_count || self.arrived {
            return car.slip_distance_m;
        }
        let fraction = ((self.sim_time_remainder + remainder_s.max(0.) * self.speed_mul)
            / self.realtime_physics_dt())
        .clamp(0., 1.);
        car.previous_slip_distance_m
            + (car.slip_distance_m - car.previous_slip_distance_m) * fraction
    }

    pub fn steam_command(&mut self, command: crate::steam::SteamCommand) -> Result<(), String> {
        if !matches!(self.consist.vehicles.get(self.cab_vehicle()),Some(openrailsrs_train::Vehicle::Loco(l)) if l.steam.is_some())
        {
            return Err("El puesto activo no controla una locomotora de vapor".into());
        }
        self.state
            .boiler_state
            .as_mut()
            .ok_or("Esta formación no tiene caldera de vapor")?
            .command(command);
        Ok(())
    }

    pub fn toggle_diesel_engine(&mut self, vehicle: usize) -> Result<(), String> {
        if self
            .state
            .vehicle_systems
            .iter()
            .any(|c| c.definition.vehicle == vehicle && !c.motors.is_empty())
        {
            return self.operate_motor(
                vehicle,
                0,
                crate::engine_equipment::MotorOperation::StartStop,
            );
        }
        let car = self
            .physics
            .diesel
            .cars
            .iter()
            .find(|c| c.vehicle == vehicle)
            .ok_or("Este vehículo no tiene motor diésel")?;
        let state = self
            .state
            .diesel
            .cars
            .iter_mut()
            .find(|c| c.vehicle == vehicle)
            .ok_or("Motor diésel sin inicializar")?;
        if !state.command_running {
            if !car.battery {
                return Err("Conectá la batería para arrancar el motor".into());
            }
            if state.fuel_l <= 0. {
                return Err("El tanque diésel está vacío".into());
            }
            if self.driver_throttle > 0.001 {
                return Err("Cerrá el regulador antes de arrancar".into());
            }
        }
        state.command_running = !state.command_running;
        crate::diesel_operation::advance(
            &mut self.state.diesel,
            &self.physics.diesel,
            0.,
            0.,
            self.physics.native.is_some(),
        );
        Ok(())
    }

    pub fn traction_status(&self) -> Option<String> {
        if let Some(b) = &self.state.boiler_state {
            Some(format!(
                "Caldera {:.1} bar · agua {:.0}%\n{}",
                b.pressure_bar,
                b.water_kg / b.initial_water_kg * 100.,
                b.status()
            ))
        } else {
            self.state.diesel.cars.first().map(|c| {
                format!(
                    "Diésel {} · {:.1} L{}",
                    c.phase.label(),
                    c.fuel_l,
                    if c.fuel_l <= 0. {
                        " · tanque vacío"
                    } else {
                        ""
                    }
                )
            })
        }
    }

    /// Explain operating restrictions from physical state, without labelling
    /// a normal high-speed fall in tractive effort as a broken locomotive.
    pub fn traction_diagnosis(&self) -> String {
        if self.state.brake_system.emergency {
            return "Emergencia aplicada: tracción cortada; detenete y rearmá con N".into();
        }
        if (0.25..0.75).contains(&self.driver_direction) {
            return "Inversor en neutro".into();
        }
        if self.exterior.door != crate::DoorState::Closed {
            return "Puertas abiertas: cerralas para dar tracción".into();
        }
        if self.physics.max_tractive_effort_n <= 0.0 {
            return "Sin unidades de tracción conectadas".into();
        }
        let index = self.cab_vehicle();
        let vehicle = index + 1;
        if let Some(c) = self.state.diesel.car(index) {
            if c.fuel_l <= 0. {
                return format!(
                    "Vehículo {vehicle}: sin combustible; abastecé el tanque en un punto habilitado"
                );
            }
            if c.phase != crate::diesel_operation::EnginePhase::Running {
                return format!(
                    "Vehículo {vehicle}: motor {}; revisá batería y arranque en B",
                    c.phase.label().to_lowercase()
                );
            }
            let available = crate::diesel_operation::available_fraction(
                &self.state.diesel,
                &self.physics.diesel,
                index,
            );
            if available < 0.999 {
                return format!(
                    "Vehículo {vehicle}: Protección térmica a {:.1} °C; potencia {:.0}%. Bajá regulador y dejá refrigerar",
                    c.temperature_c,
                    available * 100.
                );
            }
        }
        if self
            .formation
            .cars
            .get(index)
            .is_some_and(|c| !c.battery_on || !c.power_on)
        {
            return format!(
                "Vehículo {vehicle}: mando desconectado; revisá batería y encendido en F9"
            );
        }
        if self
            .state
            .power_supply
            .cars
            .iter()
            .find(|c| c.vehicle == index)
            .is_some_and(|c| !c.supply.main)
            || !self.state.electric.power_available(index)
        {
            return format!(
                "Vehículo {vehicle}: alimentación de tracción cortada; comprobá contacto con la vía, pantógrafo y disyuntor en B"
            );
        }
        if self.state.brake_system.independent_command > 0.0 {
            return "Freno independiente aplicado".into();
        }
        if self.wheel_rail_brake_forces_n().iter().sum::<f64>() > 100.0 {
            return "Los cilindros aún frenan; esperá la descarga o revisá mangueras y frenos de mano en F9".into();
        }
        if self
            .state
            .rail_adhesion
            .as_ref()
            .is_some_and(|r| r.cars.get(index).is_some_and(|c| c.slipping))
        {
            return "Patinaje: reducí regulador o aplicá arena".into();
        }
        if self.driver_throttle <= 0. {
            return "Regulador cerrado: marcha por inercia".into();
        }
        let telemetry = self.cab_telemetry();
        let force = self
            .state
            .motor_force_n
            .get(index)
            .copied()
            .unwrap_or(0.)
            .max(0.);
        let current = telemetry.max_current_a.map_or(String::new(), |max| {
            format!(
                " · corriente {:.0} A",
                max * telemetry.traction_load_fraction
            )
        });
        let demand = self.state.power_supply.vehicle_load_w(index);
        let hotel = if demand > 0. {
            format!(" · auxiliares reservan {:.1} kW", demand / 1000.)
        } else {
            String::new()
        };
        format!(
            "Vehículo {vehicle}: {:.1} kN{current}{hotel}. La fuerza baja al aumentar la velocidad por el límite de potencia; no indica una avería",
            force / 1000.
        )
    }

    pub fn toggle_pantograph(&mut self) -> Result<(), String> {
        if !self
            .physics
            .electric
            .cars
            .iter()
            .any(|c| c.params.pickup == openrailsrs_core::electrification::ElectricPickup::Overhead)
        {
            return Err("Esta formación no usa pantógrafo".into());
        }
        self.exterior.toggle_pantograph();
        for car in &mut self.state.electric.cars {
            for p in &mut car.pantographs {
                p.command_up = None;
            }
        }
        self.state.electric.pantograph_command_up = self.exterior.pantograph_command_up;
        crate::electric::advance(&mut self.state, &self.path_data, &self.physics.electric, 0.);
        Ok(())
    }

    pub fn toggle_collector(&mut self, vehicle: usize, collector: usize) -> Result<(), String> {
        if vehicle >= self.formation.coupled_count {
            return Err("Acoplá el vehículo para operar su pantógrafo".into());
        }
        let command = self.state.electric.pantograph_command_up;
        let car = self
            .state
            .electric
            .cars
            .iter_mut()
            .find(|c| c.vehicle == vehicle)
            .ok_or("El vehículo no tiene alimentación eléctrica")?;
        let p = car
            .pantographs
            .get_mut(collector)
            .ok_or("El vehículo no declara ese pantógrafo independiente")?;
        p.command_up = Some(!p.command_up.unwrap_or(command));
        Ok(())
    }

    pub fn toggle_circuit_breaker(&mut self) -> Result<(), String> {
        if self.physics.electric.cars.is_empty() {
            return Err("Esta formación no tiene tracción eléctrica".into());
        }
        self.state.electric.breaker_command_closed = !self.state.electric.breaker_command_closed;
        crate::electric::advance(&mut self.state, &self.path_data, &self.physics.electric, 0.);
        Ok(())
    }

    pub fn electric_status(&self) -> Option<String> {
        self.state
            .electric
            .cars
            .first()
            .map(|c| format!("{:.0} V · {}", c.contact_voltage_v, c.loss.label()))
    }

    pub fn from_scenario(scenario_dir: &Path, scenario: &ScenarioFile) -> Result<Self, SimError> {
        let route_dir = scenario_dir.join(&scenario.route.path);
        let mut graph = load_track_graph_from_route_dir(&route_dir)?;
        crate::freight::cap_graph(&mut graph, scenario.train.freight.as_ref());
        crate::path::apply_route_switches(&mut graph, &scenario.route)?;
        for cap in &scenario.route.edge_speed_limits {
            graph.cap_edge_speed_limit_kmh(&cap.edge, cap.speed_limit_kmh);
        }

        let path_edges = resolve_route_edges(&graph, &scenario.route)?;
        let consist_path = scenario_dir.join(&scenario.train.consist);
        let mut consist = load_consist_with_asset_root(&consist_path, consist_root(&consist_path))?;
        crate::freight::apply(&mut consist, scenario.train.freight.as_ref())?;
        let customized = openrailsrs_train::vehicle_profiles::apply_with_davis(
            &consist_path,
            &mut consist,
            &scenario.train.vehicle_profiles,
            scenario
                .train
                .davis
                .as_ref()
                .map(|d| DavisCoefficients {
                    a_n: d.a_n,
                    b_n_per_mps: d.b_n_per_mps,
                    c_n_per_mps2: d.c_n_per_mps2,
                })
                .as_ref(),
        )?;
        let curve_parameters = openrailsrs_train::load_consist_curve_parameters(
            &consist_path,
            consist_root(&consist_path),
        )?;
        let intake_points = openrailsrs_train::from_ast::load_consist_intake_points(
            &consist_path,
            consist_root(&consist_path),
        )?;
        let davis_override = scenario
            .train
            .davis
            .as_ref()
            .filter(|_| !customized)
            .map(|d| DavisCoefficients {
                a_n: d.a_n,
                b_n_per_mps: d.b_n_per_mps,
                c_n_per_mps2: d.c_n_per_mps2,
            });
        let davis = davis_override
            .clone()
            .unwrap_or_else(|| consist.davis.clone());
        let vehicle_davis = consist.per_vehicle_davis(davis_override.as_ref());
        let diesel_engines = consist.diesel_traction_models();
        let raw_curve = consist.aggregate_tractive_curve();
        let tractive = if !diesel_engines.is_empty() {
            TractiveCurve::default()
        } else if raw_curve.points.is_empty() {
            TractiveCurve::from_power_and_effort(
                consist.total_max_power_w(),
                consist.total_max_tractive_effort_n(),
            )
        } else {
            raw_curve
        };
        let partial_throttle_run_up_time_s = max_partial_throttle_run_up_time_s(&diesel_engines);
        let (native_physics, cab_max_current_a) =
            crate::native_dynamics::NativeTrainPhysics::load_with_cab(
                &consist_path,
                consist_root(&consist_path),
                &consist,
                scenario.simulation.native_physics.as_ref(),
            )?;
        let cab_vehicle_index = consist
            .vehicles
            .iter()
            .position(|v| matches!(v, openrailsrs_train::Vehicle::Loco(_)))
            .unwrap_or(0);
        let physics = TrainPhysics {
            power_supply: crate::power_supply::PowerTrainConfig::from_consist_with_loads(
                &consist,
                &scenario.train.power_load,
            ),
            rail_adhesion: Some(crate::adhesion::RailAdhesionConfig::load(
                &consist_path,
                consist_root(&consist_path),
                &consist,
            )?),
            diesel: crate::diesel_operation::DieselTrainConfig::from_consist(&consist),
            electric: crate::electric::ElectricTrainConfig::load(
                &route_dir,
                scenario.route.electric_supply.as_ref(),
                &scenario.train.electric_pickups,
                &consist,
                &graph,
            )?,
            native: native_physics,
            mass_kg: consist.total_mass_kg(),
            max_power_w: consist.total_max_power_w(),
            max_tractive_effort_n: consist.total_max_tractive_effort_n(),
            max_brake_n: consist.total_max_brake_n(),
            davis,
            vehicle_davis,
            vehicle_lengths_m: consist.vehicle_lengths_m(),
            diesel_vehicle_indices: consist.diesel_vehicle_indices(),
            tractive,
            diesel_engines,
            regen_factor: consist.regen_factor(),
            diesel_sfc_g_per_kwh: consist.diesel_sfc_g_per_kwh(),
            steam_params: consist.aggregate_steam_params(),
            brake_mapping: scenario.brake_mapping(),
            legacy_power_cap: scenario.simulation.legacy_power_cap,
            brake_skid_limit: scenario.simulation.brake_skid_limit,
            multi_body_scalar_coast_below_v_mps: scenario
                .simulation
                .multi_body_scalar_coast_below_v_mps,
            partial_throttle_run_up_time_s,
            orts_inherit_partial_run_up: scenario.simulation.orts_inherit_partial_run_up,
        };

        let mut path_data = PathData::from_path(&path_edges, &graph);
        crate::freight::cap(&mut path_data, scenario.train.freight.as_ref());
        let mut state = TrainSimState::new(path_edges.clone());
        for vehicle in &scenario.train.presentation {
            if let Some(joint) = &vehicle.jacobs {
                if vehicle.vehicle.abs_diff(joint.neighbor) != 1
                    || joint.neighbor >= consist.vehicles.len()
                    || vehicle.vehicle >= consist.vehicles.len()
                {
                    return Err(SimError::Msg(
                        "Un bogie Jacobs debe unir cuerpos adyacentes existentes".into(),
                    ));
                }
                let pair = [
                    vehicle.vehicle.min(joint.neighbor),
                    vehicle.vehicle.max(joint.neighbor),
                ];
                if !state.articulated_joints.contains(&pair) {
                    state.articulated_joints.push(pair);
                }
            }
        }
        state.weather_track = scenario.simulation.weather_track.clone();
        crate::vehicle_systems::configure(
            &mut state,
            &consist_path,
            &scenario.train.systems,
            &scenario.train.platform_sides,
            consist.vehicles.len(),
        )
        .map_err(SimError::Msg)?;
        state.announcements.package = scenario.train.announcements.clone();
        state.announcements.package.root.clear();
        state.rail_operations.definition = scenario.simulation.rail_operations.clone();
        if !state.rail_operations.definition.valid() {
            return Err(crate::SimError::Msg(
                "Operaciones ferroviarias inválidas".into(),
            ));
        }
        state.cab_equipment = crate::cab_equipment::load(
            &consist_path,
            &scenario.train.cab_equipment,
            consist.vehicles.len(),
        )
        .map_err(SimError::Msg)?;
        if let Some(offset) = scenario.route.start_offset_m {
            apply_start_offset(&mut state, &path_data, offset);
        }
        crate::electric::advance(&mut state, &path_data, &physics.electric, 0.);
        state.brake_system = build_brake_system(
            &consist,
            scenario.simulation.train_air_lap_hold,
            scenario.simulation.train_air_full_release_s,
            scenario.simulation.brake_shoe_speed_factor,
            scenario.simulation.brake_skid_limit,
        );
        state.boiler_state = consist
            .aggregate_steam_params()
            .map(|p| crate::steam::BoilerState::from_params(&p));
        crate::steam_equipment::configure(&mut state, &consist);
        state.diesel.initialize(&physics.diesel);
        if !physics.diesel_engines.is_empty() {
            state.diesel_rpm = physics
                .diesel_engines
                .iter()
                .map(|e| e.idle_rpm())
                .collect();
            let n = physics.diesel_engines.len();
            state.diesel_run_up = vec![0.0; n];
            state.diesel_motor_heat = vec![0.0; n];
            state.diesel_traction_force_n = vec![0.0; n];
            state.diesel_average_force_n = vec![0.0; n];
            state.diesel_apparent_throttle = vec![0.0; n];
        }
        state.init_multi_body_if_enabled(
            &consist,
            scenario.simulation.multi_body,
            CouplerKind::parse(&scenario.simulation.coupler_kind),
        );

        let assume_signals_clear = scenario.route.assume_signals_clear;
        let signal_runtime = init_signal_runtime(&graph, assume_signals_clear);
        let native_signals = crate::native_signals::NativeSignalRuntime::from_graph(&graph)
            .map_err(crate::SimError::Msg)?;
        let mut gameplay = if scenario.route.legs.is_empty() {
            build_live_gameplay(scenario, &graph, &path_edges)?
        } else {
            let mut stops = vec![];
            let mut prefix = 0.0;
            for (i, leg) in scenario.route.legs.iter().enumerate() {
                if ![leg.start_offset_m, leg.end_chainage_m, leg.wait_s]
                    .iter()
                    .all(|n| n.is_finite() && *n >= 0.0)
                    || leg.end_chainage_m < leg.start_offset_m
                {
                    return Err(SimError::Msg("Invalid operational leg bounds".into()));
                }
                let mut sub = scenario.clone();
                sub.route.start = leg.start.clone();
                sub.route.destination = leg.destination.clone();
                sub.route.stops = leg.stops.clone();
                sub.route.start_offset_m = Some(leg.start_offset_m);
                let g = build_live_gameplay(&sub, &graph, &leg.edges)?;
                for mut stop in g.stop_targets {
                    stop.cum_dist_m = prefix + stop.cum_dist_m - leg.start_offset_m;
                    stop.is_terminal = i + 1 == scenario.route.legs.len() && stop.is_terminal;
                    stops.push(stop);
                }
                prefix += leg.end_chainage_m - leg.start_offset_m;
            }
            LiveGameplay::new(
                scenario.route.legs.last().unwrap().destination.clone(),
                scenario.gameplay.penalty_per_second_late,
                stops,
            )
        };
        gameplay.shunting_steps = scenario.gameplay.shunting.clone();
        for goal in &gameplay.shunting_steps {
            use openrailsrs_scenarios::model::ShuntingKind;
            if matches!(goal.kind, ShuntingKind::Cut | ShuntingKind::PickUp)
                && goal.count > consist.vehicles.len()
                || matches!(goal.kind, ShuntingKind::MoveClear | ShuntingKind::Depart)
                    && goal.position_m > path_data.edges.iter().map(|e| e.length_m).sum::<f64>()
                || goal.kind == ShuntingKind::Switch
                    && !graph
                        .node(&goal.node)
                        .is_some_and(|n| matches!(n.kind, NodeKind::Switch { .. }))
            {
                return Err(SimError::Msg(
                    "El objetivo de maniobra no corresponde a esta ruta o formación".into(),
                ));
            }
        }
        let start_chainage_m =
            path_data.chainage_at_edge_position(state.edge_index, state.pos_on_edge_m);
        let at_station = gameplay.stop_targets.first().is_some_and(|stop| {
            (stop.cum_dist_m - start_chainage_m).abs() <= crate::service::STOP_POSITION_TOLERANCE_M
        });
        let initial_brake = if at_station { 1.0 } else { 0.0 };
        if at_station {
            // New native gradients must not let a parked consist roll away and
            // accidentally complete its first station before the player acts.
            state.brake = initial_brake;
            state
                .brake_system
                .precharge(physics.brake_mapping.command_to_sim_fraction(initial_brake));
        }
        let region_tracker = RegionTracker::new(scenario.sound_regions.clone());

        let mut exterior = RollingStockExteriorState::new();
        exterior.set_pantograph_up(physics.electric.cars.iter().any(|c| {
            c.params.pickup == openrailsrs_core::electrification::ElectricPickup::Overhead
        }));
        if let Some(track) = &scenario.simulation.weather_track {
            for sector in &track.sectors {
                if graph
                    .edge(&sector.edge)
                    .is_none_or(|e| sector.end_m > e.length_m + 1e-6)
                {
                    return Err(SimError::Msg(format!(
                        "Sector meteorológico {} fuera de una vía válida",
                        sector.edge
                    )));
                }
            }
        }
        for w in &scenario.simulation.rail_operations.works {
            if graph
                .edge(&w.edge)
                .is_none_or(|e| w.end_m > e.length_m + 1e-6)
            {
                return Err(SimError::Msg(format!(
                    "Obra {} fuera de una vía válida",
                    w.id
                )));
            }
        }
        for c in &scenario.simulation.rail_operations.connections {
            if c.stop >= gameplay.stop_targets.len()
                || !scenario
                    .extra_trains
                    .iter()
                    .any(|t| t.id == c.service && c.service_stop < t.stops.len())
            {
                return Err(SimError::Msg(
                    "Conexión ferroviaria con estación o servicio inexistente".into(),
                ));
            }
        }
        for i in &scenario.simulation.rail_operations.incidents {
            if i.stop >= gameplay.stop_targets.len()
                || !scenario.extra_trains.iter().any(|t| t.id == i.service)
            {
                return Err(SimError::Msg(
                    "Incidente con estación o servicio inexistente".into(),
                ));
            }
        }
        let mut session = Self {
            scenario_name: scenario.scenario.name.clone(),
            formation: crate::FormationState::new(&consist),
            signal_overrides: HashMap::new(),
            service_id: "Jugador".into(),
            external_occupancy: HashMap::new(),
            traffic_brake_assistance: true,
            curve_parameters,
            intake_points,
            refilling: None,
            dispatcher: crate::DispatcherStatus::default(),
            own_track_reservations: vec![],
            external_track_reservations: vec![],
            original_physics: physics.clone(),
            original_cab_equipment: state
                .cab_equipment
                .iter()
                .map(|c| c.config.clone())
                .collect(),
            original_vehicle_systems: state
                .vehicle_systems
                .iter()
                .map(|c| c.definition.clone())
                .collect(),
            cab_max_current_a,
            cab_vehicle_index,
            content_signature: crate::operations::content_signature(
                &graph,
                scenario,
                &consist,
                &physics.electric,
            ),
            consist,
            stop_offsets: scenario
                .route
                .stops
                .iter()
                .map(|s| (s.node.clone(), s.offset_m))
                .collect(),
            state,
            physics,
            operational_legs: scenario.route.legs.clone(),
            operational_leg: 0,
            operational_operation: 0,
            operational_prefix_m: 0.0,
            operational_start_m: start_chainage_m,
            operational_wait_started: None,
            parked_path_data: None,
            service_clock_start_s: scenario.scenario.start_time_s.unwrap_or(0.0),
            path_data,
            graph,
            dt: scenario.simulation.time_step,
            assume_signals_clear,
            signal_runtime,
            native_signals,
            external_track_occupancy: Vec::new(),
            gameplay,
            region_tracker,
            driver_throttle: 0.0,
            driver_brake: initial_brake,
            driver_direction: 0.5,
            exterior,
            horn_pressed_until_s: 0.0,
            wiper_active: false,
            wiper_mode: default_wiper_mode(),
            headlights: 1,
            cab_light: false,
            speed_mul: 1.0,
            sim_time_remainder: 0.0,
            previous_render_chainage_m: start_chainage_m,
            signal_steps: 0,
            arrived: false,
            start_chainage_m,
            script_tcs: None,
        };
        session.evaluate_native_signals();
        // A static cab/capture needs real aspects at t=0. The traffic layer
        // supplies its positional footprints before the first physics step.
        session.native_signals.needs_refresh = true;
        Ok(session)
    }

    pub fn set_independent_brake(&mut self, command: f64) -> Result<(), String> {
        if self.state.brake_system.circuits.is_empty() {
            return Err("Esta formación no tiene freno independiente neumático".into());
        }
        if !command.is_finite() {
            return Err("Mando de freno independiente inválido".into());
        }
        self.state.brake_system.independent_command = command.clamp(0.0, 1.0);
        Ok(())
    }

    pub fn set_direction(&mut self, direction: f64) -> Result<(), String> {
        if (direction - self.driver_direction).abs() > 0.01
            && self.velocity_mps() > 0.1
            && direction != 0.5
        {
            return Err("Detené el tren antes de invertir el sentido".into());
        }
        self.driver_direction = direction.clamp(0.0, 1.0);
        Ok(())
    }

    /// OR's W/S controls advance one detent: reverse ↔ neutral ↔ forward.
    pub fn step_direction(&mut self, forward: bool) -> Result<(), String> {
        let current: f64 = if self.driver_direction >= 0.75 {
            1.0
        } else if self.driver_direction <= 0.25 {
            0.0
        } else {
            0.5
        };
        self.set_direction((current + if forward { 0.5 } else { -0.5 }).clamp(0.0, 1.0))
    }

    pub fn braking_intervention(&self) -> Option<&'static str> {
        if self.state.brake <= self.driver_brake + 0.01 {
            return None;
        }
        if self
            .script_tcs
            .as_ref()
            .is_some_and(|host| host.applies_brake())
        {
            Some("TCS · frenado automático")
        } else if self.state.shunt_authority.is_some() {
            Some("Límite de maniobra")
        } else {
            Some("Asistencia de tráfico")
        }
    }

    pub fn horn_active(&self) -> bool {
        self.time_s() < self.horn_pressed_until_s
    }

    pub fn stop_horn(&mut self) {
        self.horn_pressed_until_s = self.time_s();
    }

    pub fn trigger_horn(&mut self, hold_s: f64) {
        self.horn_pressed_until_s = self.time_s() + hold_s.max(0.05);
    }

    pub fn toggle_wiper(&mut self) {
        self.wiper_mode = self.selected_wiper_mode().next();
        self.wiper_active = self.wiper_mode != crate::wiper::WiperMode::Off;
    }

    pub fn time_s(&self) -> f64 {
        self.state.time_s()
    }

    pub fn velocity_mps(&self) -> f64 {
        self.state.velocity_mps
    }

    /// Original vehicle data remains available after disabling or separating
    /// a motor; consumers cannot mutate the simulator's consist through it.
    pub fn vehicle_definition(&self, index: usize) -> Option<&openrailsrs_train::Vehicle> {
        self.consist.vehicles.get(index)
    }

    pub fn current_edge_id(&self) -> Option<&str> {
        self.state.current_edge()
    }

    pub fn pos_on_edge_m(&self) -> f64 {
        self.state.pos_on_edge_m
    }

    /// Graph position of a point `offset_along_path_m` ahead of (or behind) the head.
    ///
    /// Used by rolling-stock bogie articulation (#69): car/bogie longitudinal
    /// offsets are metres along the path from the consist head.
    pub fn position_at_head_offset(&self, offset_along_path_m: f64) -> Option<(String, f64)> {
        let head_chainage_m = self
            .path_data
            .chainage_at_edge_position(self.state.edge_index, self.state.pos_on_edge_m);
        PathData::position_at_odometer(
            &self.state.path_edges,
            &self.path_data.edges,
            (head_chainage_m + self.oriented_car_offset(offset_along_path_m)).max(0.0),
        )
    }

    /// Presentation position between completed physics steps, along the same
    /// routed centreline as every car. This does not advance simulation state.
    /// `frame_remainder_s` is time left over by the host's fixed-step scheduler.
    pub fn render_position_at_head_offset(
        &self,
        offset_along_path_m: f64,
        frame_remainder_s: f64,
    ) -> Option<(String, f64)> {
        let interpolated =
            self.presentation_chainage_at_offset(offset_along_path_m, frame_remainder_s);
        PathData::position_at_odometer(
            &self.state.path_edges,
            &self.path_data.edges,
            interpolated.max(0.0),
        )
    }

    /// Signed routed distance on the same presentation clock used by the bodies,
    /// cameras and wheels. Stable during pause; resetting the session resets it.
    pub fn render_head_chainage_m(&self, frame_remainder_s: f64) -> f64 {
        let current = self.head_chainage_m();
        let fraction = if self.arrived {
            1.0
        } else {
            ((self.sim_time_remainder + frame_remainder_s.max(0.0) * self.speed_mul)
                / self.realtime_physics_dt())
            .clamp(0.0, 1.0)
        };
        self.previous_render_chainage_m + (current - self.previous_render_chainage_m) * fraction
    }

    pub fn speed_limit_mps(&self) -> f64 {
        let head = self.head_chainage_m();
        let length = self.formation.length_m();
        self.path_data
            .minimum_speed_limit_between((head - length).max(0.0), head)
    }

    /// Effective limit including caution signals on the current edge.
    pub fn effective_speed_limit_mps(&self) -> f64 {
        let base = self.temporary_speed_limit(self.speed_limit_mps());
        let base = self.passing_hold_chainage().map_or(base, |target| {
            base.min((0.9 * (target - self.head_chainage_m() - 1.).max(0.)).sqrt())
        });
        let base = self.state.shunt_authority.as_ref().map_or(base, |a| {
            let remaining = a.start_m + a.distance_m - self.head_chainage_m();
            base.min(a.speed_mps)
                .min((0.44 * (remaining - 2.).max(0.)).sqrt())
        });
        let Some(edge) = self.current_edge_id() else {
            return base;
        };
        let nearest = self
            .graph
            .signals_on_edge(edge)
            .filter(|signal| signal.position_m >= self.state.pos_on_edge_m)
            .filter(|signal| {
                signal
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_none_or(|native| native.function.eq_ignore_ascii_case("NORMAL"))
            })
            .min_by(|a, b| a.position_m.total_cmp(&b.position_m));
        let has_caution = nearest.is_some_and(|signal| {
            self.signal_runtime
                .get(&signal.id)
                .copied()
                .unwrap_or(signal.aspect)
                == SignalAspect::Caution
        });
        if has_caution {
            base * CAUTION_SPEED_FACTOR
        } else {
            base
        }
    }

    pub fn signal_aspect(&self, signal_id: &str) -> Option<SignalAspect> {
        let aspect = self.signal_runtime.get(signal_id).copied()?;
        let now = self.time_s();
        let work = self.graph.signal(signal_id).is_some_and(|s| {
            self.state.rail_operations.definition.works.iter().any(|w| {
                w.edge == s.edge_id
                    && w.start_m >= s.position_m
                    && now >= w.start_s
                    && now < w.end_s
                    && (w.direction == 0
                        || w.direction == if self.driver_direction <= 0.25 { -1 } else { 1 })
            })
        });
        Some(if work && aspect == SignalAspect::Clear {
            SignalAspect::Caution
        } else {
            aspect
        })
    }

    pub fn next_signal_ahead(&self) -> Option<(f64, SignalAspect)> {
        let mut before = -self.state.pos_on_edge_m;
        for (index, edge_id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let nearest = self
                .graph
                .signals_on_edge(edge_id)
                .filter(|signal| before + signal.position_m >= 0.0)
                .min_by(|a, b| a.position_m.total_cmp(&b.position_m));
            if let Some(signal) = nearest {
                return Some((
                    before + signal.position_m,
                    self.signal_runtime
                        .get(&signal.id)
                        .copied()
                        .unwrap_or(signal.aspect),
                ));
            }
            before += self.path_data.edges[index].length_m;
        }
        None
    }

    fn distance_to_red_signal_m(&self) -> Option<f64> {
        let mut before = -self.state.pos_on_edge_m;
        for (index, edge_id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let nearest = self
                .graph
                .signals_on_edge(edge_id)
                .filter(|signal| {
                    before + signal.position_m >= 0.0
                        && self
                            .signal_runtime
                            .get(&signal.id)
                            .copied()
                            .unwrap_or(signal.aspect)
                            == SignalAspect::Stop
                })
                .min_by(|a, b| a.position_m.total_cmp(&b.position_m));
            if let Some(signal) = nearest {
                return Some(before + signal.position_m);
            }
            before += self.path_data.edges[index].length_m;
        }
        None
    }

    /// The active service leg owns its destination even before its next departure.
    pub fn destination_display(&self) -> String {
        let leg = self.operational_legs.get(self.operational_leg);
        let node = leg.map_or(self.gameplay.destination_node.as_str(), |l| {
            l.destination.as_str()
        });
        let destination = leg
            .and_then(|l| {
                l.stops
                    .iter()
                    .find(|s| s.node == node)
                    .and_then(|s| s.name.as_deref())
            })
            .or_else(|| {
                self.gameplay
                    .stop_targets
                    .iter()
                    .find(|s| s.node_id == node)
                    .map(|s| s.name.as_str())
            })
            .or_else(|| {
                self.graph.node(node).and_then(|n| {
                    if let openrailsrs_track::NodeKind::Station { name } = &n.kind {
                        Some(name.as_str())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(node);
        let service = leg
            .and_then(|l| l.service_id.as_deref())
            .unwrap_or(&self.scenario_name);
        format!(
            "{}\n{}{}",
            destination,
            if self.state.path_reversed {
                "← "
            } else {
                "→ "
            },
            service
        )
    }
    pub fn next_stop_label(&self) -> Option<&str> {
        self.gameplay
            .stop_targets
            .get(self.gameplay.next_stop_idx)
            .map(|s| s.name.as_str())
    }

    /// Remaining distance to the next scheduled stop (m), if any remain.
    pub fn distance_to_next_stop_m(&self) -> Option<f64> {
        self.gameplay
            .stop_targets
            .get(self.gameplay.next_stop_idx)
            .map(|t| (t.cum_dist_m - self.gameplay_chainage_m()).max(0.0))
    }

    pub fn head_chainage_m(&self) -> f64 {
        self.path_data
            .chainage_at_edge_position(self.state.edge_index, self.state.pos_on_edge_m)
    }

    pub fn toggle_doors(&mut self) {
        if self.equipped_door_state().is_some() {
            let open = self.equipped_door_state() == Some(crate::DoorState::Closed)
                || self.equipped_door_state() == Some(crate::DoorState::Closing);
            let side = if open {
                self.platform_door_side().unwrap_or_default()
            } else {
                openrailsrs_scenarios::vehicle_systems::DoorSide::Both
            };
            let _ = self.command_doors(None, side, open);
            return;
        }
        if self.velocity_mps().abs() <= STOP_SPEED_TOLERANCE_MPS {
            self.exterior.toggle_door();
        }
    }

    /// Fraction of route distance travelled [0, 1].
    pub fn route_progress(&self) -> f64 {
        if !self.operational_legs.is_empty() {
            let remaining = self.operational_legs[self.operational_leg..]
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    l.end_chainage_m
                        - if i == 0 {
                            self.head_chainage_m()
                        } else {
                            l.start_offset_m
                        }
                })
                .sum::<f64>();
            return (self.state.odometer_m / (self.state.odometer_m + remaining).max(1.0))
                .clamp(0.0, 1.0);
        }
        let end = self
            .gameplay
            .stop_targets
            .last()
            .filter(|s| s.is_terminal)
            .map(|s| s.cum_dist_m)
            .unwrap_or_else(|| self.path_data.total_length_m());
        let total = end - self.start_chainage_m;
        if total > 0.0 {
            (self.state.odometer_m / total).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Cached TCS status. Reading this never performs IPC or executes a script.
    pub fn etcs_status(&self) -> crate::etcs::EtcsTcsStatus {
        use crate::etcs::{BasicEtcsTcs, EtcsTcs};
        let base = BasicEtcsTcs::default().compute(self);
        self.script_tcs
            .as_ref()
            .map_or_else(|| base.clone(), |host| host.status(base.clone()))
    }

    pub(crate) fn script_context(&self, dt_s: f64) -> crate::etcs::ScriptContext {
        use crate::etcs::{ScriptSignal, ScriptSpeedPost};
        let mut signals = vec![];
        let mut distance_signal = None;
        let mut speed_posts = vec![];
        let mut before = -self.state.pos_on_edge_m;
        let current_post_speed_limit_mps = self
            .path_data
            .get(self.state.edge_index)
            .map_or(self.speed_limit_mps(), |edge| {
                edge.speed_limit_at(self.state.pos_on_edge_m)
            });
        let mut previous_limit = current_post_speed_limit_mps;
        for (index, edge_id) in self
            .state
            .path_edges
            .iter()
            .enumerate()
            .skip(self.state.edge_index)
        {
            let mut heads: Vec<_> = self
                .graph
                .signals_on_edge(edge_id)
                .filter(|signal| before + signal.position_m >= 0.)
                .collect();
            heads.sort_by(|a, b| a.position_m.total_cmp(&b.position_m));
            for head in heads {
                let aspect = self.native_signal_aspect(&head.id).unwrap_or_else(|| {
                    match self
                        .signal_runtime
                        .get(&head.id)
                        .copied()
                        .unwrap_or(head.aspect)
                    {
                        SignalAspect::Stop => 0,
                        SignalAspect::Caution => 3,
                        SignalAspect::Clear => 7,
                    }
                });
                let signal = ScriptSignal {
                    distance_m: before + head.position_m,
                    aspect,
                };
                let distant = head
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|n| n.function.eq_ignore_ascii_case("DISTANCE"));
                let normal = head
                    .script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_none_or(|n| n.function.eq_ignore_ascii_case("NORMAL"));
                if distant {
                    if distance_signal.is_none() {
                        distance_signal = Some(signal);
                    }
                } else if normal && signals.len() < 32 {
                    signals.push(signal);
                }
            }
            let edge = &self.path_data.edges[index];
            let changes = std::iter::once((0., edge.speed_limit_mps)).chain(
                edge.profile
                    .speed_posts
                    .iter()
                    .map(|p| (p.position_m, p.speed_limit_kmh / 3.6)),
            );
            for (position, limit) in changes {
                let distance = before + position;
                if distance >= 0. && (limit - previous_limit).abs() > 1e-6 {
                    if speed_posts.len() < 32 {
                        speed_posts.push(ScriptSpeedPost {
                            distance_m: distance,
                            speed_limit_mps: limit,
                        });
                    }
                    previous_limit = limit;
                }
            }
            before += edge.length_m;
        }
        let train_max_speed_mps = self
            .consist
            .vehicles
            .iter()
            .filter_map(|v| match v {
                openrailsrs_train::Vehicle::Loco(l) if l.max_velocity_mps > 0. => {
                    Some(l.max_velocity_mps)
                }
                _ => None,
            })
            .reduce(f64::min)
            .unwrap_or(self.speed_limit_mps());
        crate::etcs::ScriptContext {
            time_s: self.time_s(),
            dt_s,
            speed_mps: self.velocity_mps().abs(),
            speed_limit_mps: self.effective_speed_limit_mps(),
            next_signal_distance_m: self.next_signal_ahead().map(|(d, _)| d),
            next_signal_stop: self
                .next_signal_ahead()
                .is_some_and(|(_, aspect)| aspect == SignalAspect::Stop),
            next_stop_distance_m: self.distance_to_next_stop_m(),
            train_max_speed_mps,
            current_post_speed_limit_mps,
            signals,
            distance_signal,
            speed_posts,
        }
    }

    pub fn attach_script_tcs(
        &mut self,
        config: &crate::etcs::ScriptHostConfig,
    ) -> Result<(), String> {
        self.script_tcs = Some(crate::etcs::ScriptTcsHost::launch(
            config,
            &self.script_context(0.0),
        )?);
        Ok(())
    }

    /// Select an optional provider explicitly at startup. Presentation callers
    /// do not construct .NET processes or parse their protocol.
    pub fn configure_tcs_from_env(&mut self) -> Result<(), String> {
        let Ok(script) = std::env::var("OPENRAILSRS_TCS_SCRIPT") else {
            return Ok(());
        };
        let assembly = std::env::var("OPENRAILSRS_TCS_HOST_DLL")
            .map_err(|_| "OPENRAILSRS_TCS_HOST_DLL is required for an explicit C# TCS")?;
        self.attach_script_tcs(&crate::etcs::ScriptHostConfig {
            executable: std::env::var("OPENRAILSRS_DOTNET")
                .unwrap_or_else(|_| "dotnet".into())
                .into(),
            arguments: vec![assembly],
            script: script.into(),
            type_name: std::env::var("OPENRAILSRS_TCS_TYPE")
                .unwrap_or_else(|_| "MinimalTcs".into()),
            timeout: std::time::Duration::from_millis(250),
        })
    }

    pub fn send_tcs_input(&mut self, input: crate::etcs::TcsInput) {
        if let Some(host) = &mut self.script_tcs {
            host.push_input(input);
        }
    }

    /// Brake force delivered to the rail after the wheel/rail grip limit.
    pub fn wheel_rail_brake_forces_n(&self) -> Vec<f64> {
        let mut forces = self
            .state
            .brake_system
            .cylinder_forces_n(self.state.velocity_mps);
        if let Some(config) = &self.physics.rail_adhesion {
            crate::adhesion::cap_brakes(&self.state, config, &mut forces);
        }
        forces
    }

    /// Snapshot for the live cab panel (Fase C3).
    pub fn cab_vehicle(&self) -> usize {
        self.state
            .active_cab_vehicle
            .unwrap_or(self.cab_vehicle_index)
    }

    /// A cab change transfers the controls, never the physical consist.
    pub fn select_cab(&mut self, vehicle: usize) -> Result<(), String> {
        if self.velocity_mps().abs() > 0.01
            || self.driver_throttle > 0.001
            || (self.driver_direction - 0.5).abs() > 0.01
        {
            return Err("Detené el tren, cerrá el regulador y poné el inversor en neutro para cambiar de cabina".into());
        }
        if vehicle >= self.formation.coupled_count
            || !matches!(
                self.consist.vehicles.get(vehicle),
                Some(openrailsrs_train::Vehicle::Loco(_))
            )
        {
            return Err("Ese vehículo no tiene un puesto de conducción conectado".into());
        }
        let previous_cab = self.cab_vehicle();
        if let Some(b) = self
            .state
            .steam_boilers
            .iter_mut()
            .find(|b| b.vehicle == previous_cab)
            && let Some(current) = &self.state.boiler_state
        {
            b.boiler = current.clone();
        }
        self.state.active_cab_vehicle = Some(vehicle);
        if !self.state.steam_boilers.is_empty() {
            self.state.boiler_state = self
                .state
                .steam_boilers
                .iter()
                .find(|b| b.vehicle == vehicle)
                .map(|b| b.boiler.clone());
        }
        self.rebuild_formation_physics();
        if let Some(native) = &mut self.physics.native {
            native.control_vehicle_index = vehicle;
        }
        self.stop_horn();
        Ok(())
    }

    pub fn cycle_cab(&mut self) -> Result<usize, String> {
        let cabs = (0..self.formation.coupled_count)
            .filter(|&i| {
                matches!(
                    self.consist.vehicles.get(i),
                    Some(openrailsrs_train::Vehicle::Loco(_))
                )
            })
            .collect::<Vec<_>>();
        if cabs.len() < 2 {
            return Err("La formación tiene un solo puesto de conducción".into());
        }
        let slot = cabs
            .iter()
            .position(|&i| i == self.cab_vehicle())
            .unwrap_or(0);
        let next = cabs[(slot + 1) % cabs.len()];
        self.select_cab(next)?;
        Ok(next)
    }

    pub fn effective_headlights(&self) -> u8 {
        if self
            .state
            .power_supply
            .cars
            .iter()
            .find(|c| c.vehicle == self.cab_vehicle())
            .map_or_else(
                || {
                    self.formation
                        .cars
                        .get(self.cab_vehicle())
                        .is_none_or(|c| c.battery_on)
                },
                |c| c.supply.low_voltage,
            )
        {
            self.headlights
        } else {
            0
        }
    }

    pub fn effective_cab_light(&self) -> bool {
        self.cab_light
            && self
                .state
                .power_supply
                .cars
                .iter()
                .find(|c| c.vehicle == self.cab_vehicle())
                .map_or_else(
                    || {
                        self.formation
                            .cars
                            .get(self.cab_vehicle())
                            .is_none_or(|c| c.battery_on && c.power_on)
                    },
                    |c| c.supply.cab,
                )
    }

    pub fn selected_wiper_mode(&self) -> crate::wiper::WiperMode {
        if !self.wiper_active {
            crate::wiper::WiperMode::Off
        } else if self.wiper_mode == crate::wiper::WiperMode::Off {
            crate::wiper::WiperMode::Slow
        } else {
            self.wiper_mode
        }
    }
    pub fn effective_wiper_active(&self) -> bool {
        self.selected_wiper_mode().moving(self.time_s())
            && self
                .state
                .power_supply
                .cars
                .iter()
                .find(|c| c.vehicle == self.cab_vehicle())
                .map_or_else(
                    || {
                        self.formation
                            .cars
                            .get(self.cab_vehicle())
                            .is_none_or(|c| c.battery_on)
                    },
                    |c| c.supply.low_voltage,
                )
    }

    pub fn cab_telemetry(&self) -> CabTelemetry {
        let speed_kmh = self.state.velocity_mps * 3.6;
        let limit_kmh = self.effective_speed_limit_mps() * 3.6;
        let brake_force_kn = self.wheel_rail_brake_forces_n().iter().sum::<f64>() / 1000.;
        let vehicle = self.cab_vehicle();
        let diesel_rpm = self
            .state
            .vehicle_systems
            .iter()
            .find(|c| c.definition.vehicle == vehicle && !c.motors.is_empty())
            .map(|c| c.motors.iter().map(|m| m.rpm).sum::<f64>() / c.motors.len() as f64)
            .or_else(|| self.state.diesel.car(vehicle).map(|c| c.rpm))
            .or_else(|| {
                self.physics
                    .diesel_vehicle_indices
                    .iter()
                    .position(|&i| i == vehicle)
                    .and_then(|i| self.state.diesel_rpm.get(i))
                    .copied()
            });
        let mut diesel_telemetry = self.state.diesel.car(vehicle).cloned();
        if let Some(equipment) = self
            .state
            .vehicle_systems
            .iter()
            .find(|c| c.definition.vehicle == vehicle && !c.motors.is_empty())
            && let Some(t) = &mut diesel_telemetry
        {
            t.rpm = diesel_rpm.unwrap_or(0.);
            t.fuel_l = equipment.motors.iter().map(|m| m.fuel_l).sum();
            t.consumed_l = equipment.motors.iter().map(|m| m.consumed_l).sum();
            t.command_running = equipment.motors.iter().any(|m| m.running);
            t.phase = if t.command_running {
                crate::diesel_operation::EnginePhase::Running
            } else {
                crate::diesel_operation::EnginePhase::Stopped
            };
            t.temperature_c = equipment
                .motors
                .iter()
                .map(|m| m.coolant_c)
                .fold(0., f64::max);
        }
        let is_steam = matches!(self.consist.vehicles.get(vehicle), Some(openrailsrs_train::Vehicle::Loco(l)) if l.steam.is_some());
        let boiler_bar = is_steam
            .then(|| self.state.boiler_state.as_ref().map(|b| b.pressure_bar))
            .flatten();
        let main_res_bar = self
            .state
            .brake_system
            .circuits
            .iter()
            .find(|c| c.vehicle == vehicle)
            .map_or(boiler_bar.unwrap_or(8.0), |c| c.reservoir_bar);
        let head_vented = self
            .state
            .brake_system
            .cylinders
            .get(vehicle)
            .is_some_and(|b| b.air_vented);
        let brake_pipe_bar = self
            .state
            .brake_system
            .cylinders
            .get(vehicle)
            .and_then(|c| c.pipe_pressure_bar())
            .unwrap_or_else(|| {
                if head_vented {
                    0.
                } else {
                    (5.0 - self.driver_brake * 3.5).max(0.0)
                }
            });
        let supply = self
            .state
            .power_supply
            .cars
            .iter()
            .find(|c| c.vehicle == vehicle)
            .map(|c| &c.supply);
        let cylinders = &self.state.brake_system.cylinders;
        // The driving cab reads the lead vehicle's cylinder, not the average of
        // trailer/motor pressures with different native full-pressure ratings.
        let brake_cyl_bar = cylinders
            .get(vehicle)
            .map_or(self.driver_brake * 4.5, |b| b.pressure_bar());
        CabTelemetry {
            electric_brake_fraction: self
                .state
                .vehicle_systems
                .iter()
                .find(|c| c.definition.vehicle == vehicle)
                .map_or(0., |c| {
                    c.definition.electric_brake.as_ref().map_or(0., |p| {
                        c.brakes.electric_force_n
                            / p.force_curve.iter().map(|p| p[1]).fold(1., f64::max)
                    })
                }),
            dynamic_max_current_a: self
                .state
                .vehicle_systems
                .iter()
                .find(|c| c.definition.vehicle == vehicle)
                .and_then(|c| c.definition.electric_brake.as_ref())
                .and_then(|p| p.maximum_current_a),
            dynamic_brake_pct: crate::multiple_unit::brake(&self.state, vehicle) * 100.,
            electric_brake_force_n: self
                .state
                .vehicle_systems
                .iter()
                .find(|c| c.definition.vehicle == vehicle)
                .map_or(0., |c| c.brakes.electric_force_n),
            cab_windows: self.cab_equipment().map_or([0.; 2], |c| c.windows),
            cab_door: self.cab_equipment().map_or(0., |c| c.door),
            washer_spraying: self.cab_equipment().is_some_and(|c| c.spraying),
            wheel_slip: self
                .state
                .rail_adhesion
                .as_ref()
                .is_some_and(|r| r.slipping()),
            sander_on: self
                .state
                .rail_adhesion
                .as_ref()
                .is_some_and(|r| r.sander_command),
            steam: if is_steam {
                self.state.boiler_state.clone()
            } else {
                None
            },
            diesel: diesel_telemetry,
            pantograph_fraction: self
                .state
                .electric
                .cars
                .iter()
                .find(|c| c.vehicle == vehicle)
                .map_or(0., |c| c.pantograph_fraction),
            line_voltage_v: self
                .state
                .electric
                .cars
                .iter()
                .find(|c| c.vehicle == vehicle)
                .map_or(0., |c| c.contact_voltage_v),
            circuit_breaker_state: self
                .state
                .electric
                .cars
                .iter()
                .find(|c| c.vehicle == vehicle)
                .map_or(0, |c| match c.breaker {
                    crate::electric::BreakerState::Open => 0,
                    crate::electric::BreakerState::Closing => 1,
                    crate::electric::BreakerState::Closed => 2,
                }),
            main_power: supply.is_some_and(|s| s.main),
            auxiliary_power: supply.is_some_and(|s| s.auxiliary),
            low_voltage_power: supply.is_some_and(|s| s.low_voltage),
            cab_power: supply.is_some_and(|s| s.cab),
            train_supply: supply.is_some_and(|s| s.train_supply),
            vacuum_brake: cylinders.get(vehicle).is_some_and(|c| c.is_vacuum()),
            speed_kmh,
            limit_kmh,
            throttle_pct: self.driver_throttle * 100.0,
            brake_pct: self.driver_brake * 100.0,
            direction: self.driver_direction.clamp(0.0, 1.0),
            horn_active: self.time_s() < self.horn_pressed_until_s,
            wiper_active: self.effective_wiper_active(),
            wiper_mode: self.selected_wiper_mode(),
            headlights: self.effective_headlights(),
            cab_light: self.effective_cab_light(),
            engine_failure: self
                .state
                .vehicle_systems
                .iter()
                .filter(|c| c.definition.vehicle == vehicle)
                .flat_map(|c| &c.motors)
                .any(|m| m.fault.is_some() || m.fuel_l <= 0.)
                || self
                    .state
                    .diesel
                    .car(self.cab_vehicle())
                    .is_some_and(|c| c.fuel_l <= 0.)
                || self
                    .state
                    .boiler_state
                    .as_ref()
                    .is_some_and(|b| b.low_water_failure),
            thermal_limiting: crate::diesel_operation::available_fraction(
                &self.state.diesel,
                &self.physics.diesel,
                self.cab_vehicle(),
            ) < 0.999,
            emergency: self.state.brake_system.emergency,
            independent_brake_pct: self.state.brake_system.independent_command * 100.,
            main_res_bar,
            brake_pipe_bar,
            brake_cyl_bar,
            brake_force_kn,
            diesel_rpm,
            boiler_bar,
            tractive_load_fraction: self
                .consist
                .vehicles
                .get(self.cab_vehicle())
                .and_then(|v| {
                    if let openrailsrs_train::Vehicle::Loco(l) = v {
                        Some(if l.max_tractive_effort_n > 0. {
                            self.state
                                .rail_force_n
                                .get(self.cab_vehicle())
                                .copied()
                                .unwrap_or(0.)
                                / l.max_tractive_effort_n
                        } else {
                            0.
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(0.),
            // OR 1.6.1 MSTSLocomotive constructs one axle even without Pacha physics.
            native_axles: matches!(
                self.consist.vehicles.get(self.cab_vehicle()),
                Some(openrailsrs_train::Vehicle::Loco(_))
            ),
            max_current_a: self
                .physics
                .native
                .as_ref()
                .and_then(|n| n.vehicles.get(vehicle))
                .and_then(|p| p.max_current_a)
                .or_else(|| {
                    (vehicle == self.cab_vehicle_index)
                        .then_some(self.cab_max_current_a)
                        .flatten()
                }),
            traction_load_fraction: self
                .consist
                .vehicles
                .get(self.cab_vehicle())
                .and_then(|v| {
                    if let openrailsrs_train::Vehicle::Loco(l) = v {
                        Some(if l.max_tractive_effort_n > 0. {
                            self.state
                                .motor_force_n
                                .get(self.cab_vehicle())
                                .copied()
                                .unwrap_or(0.)
                                / l.max_tractive_effort_n
                        } else {
                            0.
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or(0.),
            overspeed: self.gameplay.overspeed_active,
        }
    }
}

/// Driver-facing gauges for the 3D cab panel.
#[derive(Clone, Debug, PartialEq)]
pub struct CabTelemetry {
    pub dynamic_brake_pct: f64,
    pub electric_brake_force_n: f64,
    pub electric_brake_fraction: f64,
    pub dynamic_max_current_a: Option<f64>,
    pub cab_windows: [f64; 2],
    pub cab_door: f64,
    pub washer_spraying: bool,
    pub wheel_slip: bool,
    pub engine_failure: bool,
    pub thermal_limiting: bool,
    pub emergency: bool,
    pub independent_brake_pct: f64,
    pub sander_on: bool,
    pub steam: Option<crate::steam::BoilerState>,
    pub diesel: Option<crate::diesel_operation::DieselCarState>,
    pub pantograph_fraction: f64,
    pub line_voltage_v: f64,
    pub circuit_breaker_state: u8,
    pub main_power: bool,
    pub auxiliary_power: bool,
    pub low_voltage_power: bool,
    pub cab_power: bool,
    pub train_supply: bool,
    pub vacuum_brake: bool,
    pub speed_kmh: f64,
    pub limit_kmh: f64,
    pub throttle_pct: f64,
    pub brake_pct: f64,
    /// Reverser position 0–1 (0 = REV, 0.5 = neutral, 1 = FWD).
    pub direction: f64,
    pub horn_active: bool,
    pub wiper_active: bool,
    pub wiper_mode: crate::wiper::WiperMode,
    pub headlights: u8,
    pub cab_light: bool,
    pub main_res_bar: f64,
    pub brake_pipe_bar: f64,
    pub brake_cyl_bar: f64,
    pub brake_force_kn: f64,
    pub diesel_rpm: Option<f64>,
    pub boiler_bar: Option<f64>,
    pub traction_load_fraction: f64,
    pub max_current_a: Option<f64>,
    pub tractive_load_fraction: f64,
    pub native_axles: bool,
    pub overspeed: bool,
}

/// Max physics quantum when stepping from wall-clock (viewer / interactive).
///
/// Headless Chiltern scenarios use `time_step = 1.0` for OR compare; without a
/// cap the live train sits still for ~1 s then teleports — feels like jumps.
const LIVE_REALTIME_MAX_DT_S: f64 = 0.05;

impl LiveDriveSession {
    /// Physics dt used by [`Self::step_realtime`] (scenario dt, capped for smoothness).
    pub fn realtime_physics_dt(&self) -> f64 {
        self.dt.clamp(1e-4, LIVE_REALTIME_MAX_DT_S)
    }

    /// Advance simulation by `real_dt` seconds of wall-clock time (scaled by `speed_mul`).
    ///
    /// `on_region_transition` is invoked for each sound-region enter/leave (e.g. audio engine).
    pub fn step_realtime<F>(&mut self, real_dt: f64, mut on_region_transition: F)
    where
        F: FnMut(&RegionTransition),
    {
        self.step_with_controller(real_dt, None, &mut on_region_transition);
    }

    /// Deterministic demonstration driver; decisions run at each physics quantum.
    pub fn step_autodrive<F>(&mut self, real_dt: f64, throttle: f64, mut on_region_transition: F)
    where
        F: FnMut(&RegionTransition),
    {
        self.step_with_controller(
            real_dt,
            Some(throttle.clamp(0.0, 1.0)),
            &mut on_region_transition,
        );
    }

    fn step_with_controller<F>(
        &mut self,
        real_dt: f64,
        automatic: Option<f64>,
        on_region_transition: &mut F,
    ) where
        F: FnMut(&RegionTransition),
    {
        if self.arrived
            || !real_dt.is_finite()
            || real_dt <= 0.0
            || !self.speed_mul.is_finite()
            || self.speed_mul <= 0.0
        {
            return;
        }
        let mut budget = self.sim_time_remainder + real_dt * self.speed_mul;
        let dt = self.realtime_physics_dt();
        self.state.electric.pantograph_command_up = self.exterior.pantograph_command_up;
        while budget + 1e-12 >= dt {
            if let Some(throttle) = automatic {
                self.autodrive_inputs(throttle);
            }
            self.state.driver_reverser = self.driver_direction;
            self.state.throttle = if (self.driver_direction >= 0.75
                || self.driver_direction <= 0.25)
                && self.exterior.door == crate::exterior::DoorState::Closed
                && !self.gameplay.departure_hold
            {
                self.driver_throttle
            } else {
                0.0
            };
            let assistance_was_braking = self.state.brake > self.driver_brake + 0.01;
            self.state.brake = self.driver_brake;
            if self.state.brake_system.emergency {
                self.state.throttle = 0.0;
                self.state.brake = 1.0;
            }
            if self
                .script_tcs
                .as_ref()
                .is_some_and(|host| host.applies_brake())
            {
                self.state.throttle = 0.0;
                self.state.brake = 1.0;
            }
            if (automatic.is_some() || self.traffic_brake_assistance)
                && self.distance_to_occupied_block_m().is_some_and(|distance| {
                    let speed =
                        self.velocity_mps() + if assistance_was_braking { 0.6 } else { 0.0 };
                    distance < speed.powi(2) / 0.44 + 12.0
                })
            {
                self.state.throttle = 0.0;
                self.state.brake = 1.0;
            }
            if self.state.shunt_authority.as_ref().is_some_and(|a| {
                let remaining = a.start_m + a.distance_m - self.head_chainage_m();
                self.velocity_mps() > a.speed_mps
                    || remaining < self.velocity_mps().powi(2) / 0.44 + 2.
            }) {
                self.state.throttle = 0.;
                self.state.brake = 1.;
            }
            let red_distance = self.distance_to_red_signal_m();
            let previous_odometer = self.state.odometer_m;
            self.previous_render_chainage_m = self.head_chainage_m();
            let backwards = (self.driver_direction <= 0.25) != self.state.path_reversed;
            if let Some(rail) = &mut self.state.rail_adhesion {
                rail.backwards = self.driver_direction <= 0.25;
            }
            let res = if backwards {
                let previous = self.head_chainage_m();
                let old_odometer = self.state.odometer_m;
                let mut edge = self.path_data.edges[self.state.edge_index].clone();
                edge.length_m = 1.0e12;
                edge.grade_percent = -edge.grade_percent;
                for grade in &mut edge.profile.grades {
                    grade.grade_percent = -grade.grade_percent;
                }
                let source_edge = self
                    .state
                    .path_edges
                    .get(self.state.edge_index)
                    .cloned()
                    .unwrap_or_default();
                // The velocity calculation uses a virtual long edge; retain the
                // native position for spatial weather and grade evaluation.
                self.state.edge_index = 0;
                step(
                    &mut self.state,
                    &PathData {
                        edges: vec![edge],
                        edge_ids: vec![source_edge],
                    },
                    &self.physics,
                    dt,
                );
                let traveled = (self.state.odometer_m - old_odometer).min(previous);
                apply_start_offset(&mut self.state, &self.path_data, previous - traveled);
                self.state.odometer_m = old_odometer + traveled;
                if previous - traveled <= 0.001 {
                    self.state.velocity_mps = 0.0;
                    for vehicle in &mut self.state.vehicles {
                        vehicle.velocity_mps = 0.0;
                    }
                    self.driver_throttle = 0.0;
                    self.driver_brake = 1.0;
                }
                crate::physics::StepResult { arrived: false }
            } else {
                step(&mut self.state, &self.path_data, &self.physics, dt)
            };
            if !backwards
                && red_distance.is_some_and(|distance| {
                    self.state.odometer_m - previous_odometer > distance + 0.01
                })
            {
                self.gameplay.fail("Señal de parada rebasada");
            }
            self.update_work_occupancy();
            self.tick_after_physics_step(dt, on_region_transition);
            self.update_shunting();
            let physical_end = res.arrived && self.operational_legs.is_empty();
            if physical_end
                && self.gameplay.shunting_completed_s.len() < self.gameplay.shunting_steps.len()
            {
                self.gameplay
                    .fail("Maniobras incompletas al llegar al final");
            }
            let operational_end = self.tick_operational_path(automatic.is_some());
            if physical_end || operational_end {
                self.gameplay.finish_at_route_end(
                    self.time_s(),
                    self.gameplay_chainage_m(),
                    self.velocity_mps(),
                );
            }
            budget -= dt;
            if self.gameplay.is_finished() {
                self.arrived = true;
                break;
            }
            if physical_end {
                self.gameplay.phase = ServicePhase::Completed;
                self.arrived = true;
                break;
            }
        }
        self.sim_time_remainder = if self.arrived { 0.0 } else { budget };
    }

    fn autodrive_inputs(&mut self, notch: f64) {
        use crate::exterior::DoorState;
        self.driver_direction = if self.state.path_reversed { 0.0 } else { 1.0 };
        let v = self.velocity_mps();
        match self.gameplay.phase {
            ServicePhase::Boarding | ServicePhase::ReadyToDepart => {
                self.driver_throttle = 0.0;
                self.driver_brake = 1.0;
                if self.gameplay.phase == ServicePhase::ReadyToDepart {
                    if matches!(self.exterior.door, DoorState::Open | DoorState::Opening) {
                        if self.equipped_door_state().is_some() {
                            let _ = self.command_doors(
                                None,
                                openrailsrs_scenarios::vehicle_systems::DoorSide::Both,
                                false,
                            );
                        } else {
                            self.exterior.set_door(DoorState::Closing);
                        }
                    }
                } else if self.exterior.door == DoorState::Closed {
                    if self.equipped_door_state().is_some() {
                        let side = self.platform_door_side().unwrap_or_default();
                        let _ = self.command_doors(None, side, true);
                    } else {
                        self.exterior.set_door(DoorState::Opening);
                    }
                }
                return;
            }
            ServicePhase::Completed | ServicePhase::Failed => return,
            ServicePhase::Approaching => {}
        }
        // Conservative service-brake approach with a propagation/release margin.
        // The curve tightens to zero at the authored stop, without snapping physics.
        let stop_cap = self
            .distance_to_next_stop_m()
            .map(|distance| (2.0 * 0.22 * (distance - 4.0).max(0.0)).sqrt())
            .unwrap_or(f64::INFINITY);
        let route_cap = self.effective_speed_limit_mps() * 0.9;
        let signal_cap = self
            .distance_to_red_signal_m()
            .into_iter()
            .chain(self.distance_to_occupied_block_m())
            .reduce(f64::min)
            .map(|distance| (2.0 * 0.22 * (distance - 4.0).max(0.0)).sqrt())
            .unwrap_or(f64::INFINITY);
        let cap = route_cap.min(stop_cap).min(signal_cap).min(
            self.operational_distance_m()
                .map(|d| (2.0 * 0.22 * (d - 4.0).max(0.0)).sqrt())
                .unwrap_or(f64::INFINITY),
        );
        if self
            .distance_to_next_stop_m()
            .is_some_and(|d| d <= STOP_POSITION_TOLERANCE_M)
            && v <= STOP_SPEED_TOLERANCE_MPS
        {
            self.driver_throttle = 0.0;
            self.driver_brake = 1.0;
        } else if v > cap + 0.15 {
            self.driver_throttle = 0.0;
            self.driver_brake = if cap < 0.5 { 1.0 } else { 0.45 };
        } else if v < cap - 0.3 {
            self.driver_brake = 0.0;
            self.driver_throttle = notch;
        } else {
            self.driver_throttle = 0.0;
            self.driver_brake = 0.0;
        }
    }

    fn tick_after_physics_step<F>(&mut self, step_dt: f64, on_region_transition: &mut F)
    where
        F: FnMut(&RegionTransition),
    {
        self.tick_refill(step_dt);
        self.exterior.tick(step_dt);
        if let Some(door) = self.equipped_door_state() {
            self.exterior.door = door;
        }
        self.tick_signals(step_dt);
        self.tick_announcements();
        self.tick_gameplay(step_dt);
        self.finish_pending_door_closures();
        self.update_shunt_authority();
        self.tick_depot_work(step_dt);
        if self.script_tcs.is_some() {
            let context = self.script_context(step_dt);
            if let Some(host) = &mut self.script_tcs {
                host.tick(&context);
            }
        }

        if let Some(edge_id) = self.state.current_edge() {
            let transitions = self.region_tracker.step(edge_id, self.state.pos_on_edge_m);
            for t in &transitions {
                on_region_transition(t);
            }
        }
    }

    pub(crate) fn tick_signals(&mut self, step_dt: f64) {
        self.signal_steps += 1;
        let every = (1.0 / step_dt).round().max(1.0) as u64;
        if every > 0 && self.signal_steps.is_multiple_of(every) {
            let block_map = self.occupied_edges();
            self.graph.evaluate_signals(&block_map);
            for sig in self.graph.signals() {
                if let Some(aspect) = self.signal_overrides.get(&sig.id) {
                    let protection = self.graph.signal_occupancy_constraint(
                        &sig.id,
                        &self.external_occupancy,
                        None,
                    );
                    let aspect = match (aspect, protection) {
                        (_, Some(SignalAspect::Stop)) => SignalAspect::Stop,
                        (SignalAspect::Clear, Some(SignalAspect::Caution)) => SignalAspect::Caution,
                        _ => *aspect,
                    };
                    self.signal_runtime.insert(sig.id.clone(), aspect);
                    continue;
                }
                if self.external_occupancy.is_empty()
                    && sig
                        .clear_after_s
                        .is_some_and(|clear_t| self.state.time_s() >= clear_t)
                {
                    self.signal_runtime
                        .insert(sig.id.clone(), SignalAspect::Clear);
                    continue;
                }
                if self.external_occupancy.is_empty()
                    && self.assume_signals_clear
                    && sig.script.is_none()
                {
                    continue;
                }
                let governs_player = self
                    .state
                    .path_edges
                    .iter()
                    .enumerate()
                    .skip(self.state.edge_index)
                    .any(|(index, edge)| {
                        edge == &sig.edge_id
                            && (index > self.state.edge_index
                                || sig.position_m >= self.pos_on_edge_m())
                    });
                let aspect = if governs_player {
                    self.graph
                        .signal_aspect_for_occupancy(&sig.id, &block_map, Some(&self.service_id))
                        .unwrap_or(sig.aspect)
                } else {
                    sig.aspect
                };
                self.signal_runtime.insert(sig.id.clone(), aspect);
            }
            self.evaluate_native_signals();
        }
    }

    pub(crate) fn refresh_traffic_signals(&mut self) {
        let dt = self.realtime_physics_dt();
        self.signal_steps = (1.0 / dt).round().max(1.0) as u64 - 1;
        self.tick_signals(dt);
    }

    fn tick_gameplay(&mut self, step_dt: f64) {
        let limit = self.effective_speed_limit_mps();
        self.gameplay.overspeed_active =
            limit.is_finite() && self.state.velocity_mps > limit * 1.05;

        if let Some((off, on)) = self.gameplay.tick(
            self.time_s(),
            self.gameplay_chainage_m(),
            self.velocity_mps(),
            self.exterior.door,
            step_dt,
        ) {
            self.state.passengers = self.state.passengers.saturating_sub(off).saturating_add(on);
            self.state.extra_mass_kg =
                self.state.passengers as f64 * crate::runner::KG_PER_PASSENGER;
        }
        if let Some(mut lesson) = self.state.learning.take() {
            lesson.observe(self);
            self.state.learning = Some(lesson);
        }
        let mut report = std::mem::take(&mut self.state.driving_report);
        report.observe(self);
        self.state.driving_report = report;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_scenarios::load_scenario;
    use std::path::PathBuf;

    #[test]
    fn reverser_keys_visit_neutral_and_reject_reversing_a_moving_train() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let scenario = load_scenario(&path).unwrap();
        let mut s = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        s.set_direction(0.0).unwrap();
        for value in [0.5, 1.0, 1.0] {
            s.step_direction(true).unwrap();
            assert_eq!(s.driver_direction, value);
        }
        for value in [0.5, 0.0, 0.0] {
            s.step_direction(false).unwrap();
            assert_eq!(s.driver_direction, value);
        }
        s.set_direction(1.0).unwrap();
        s.state.velocity_mps = 10.0;
        s.step_direction(false).unwrap();
        assert_eq!(s.driver_direction, 0.5);
        assert!(s.step_direction(false).is_err());
        assert_eq!(s.driver_direction, 0.5);
    }

    #[test]
    fn traffic_brake_is_explicit_and_its_hysteresis_prevents_amp_chatter() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_extended/scenario.toml");
        let scenario = load_scenario(&path).unwrap();
        for assisted in [true, false] {
            let mut s = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
            s.traffic_brake_assistance = assisted;
            s.driver_direction = 1.0;
            s.driver_throttle = 1.0;
            s.driver_brake = 0.0;
            s.exterior.set_door(crate::exterior::DoorState::Closed);
            s.gameplay.stop_targets.clear();
            s.set_rail_weather(crate::adhesion::RailWeather::Storm);
            let mut traffic =
                crate::LiveTraffic::from_scenario(path.parent().unwrap(), &scenario).unwrap();
            let mut prev = 0.0;
            let mut jumps = 0;
            let mut interventions = 0;
            for _ in 0..6000 {
                traffic.advance(&mut s, 0.05, None, |_| {});
                let t = s.cab_telemetry();
                if s.time_s() > 60.0 && (t.traction_load_fraction - prev).abs() > 0.03 {
                    jumps += 1;
                }
                prev = t.traction_load_fraction;
                interventions += usize::from(s.braking_intervention().is_some());
                if s.arrived {
                    break;
                }
            }
            // Previously the same trip had 796 abrupt load jumps, while the driver
            // requested constant power and no brake. Native manual driving has none.
            assert!(
                jumps < 50,
                "assisted={assisted}: {jumps} abrupt AMP changes"
            );
            if assisted {
                assert!(interventions > 0);
            } else {
                assert_eq!(interventions, 0);
                assert_eq!(jumps, 0);
            }
        }
    }

    #[test]
    fn native_service_starts_with_brakes_holding_its_real_gradient() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern_extended/scenario.toml");
        let scenario = load_scenario(&path).unwrap();
        let mut session =
            LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario).unwrap();
        let cylinders = &session.state.brake_system.cylinders;
        assert_eq!(cylinders.len(), 8);
        assert!(cylinders[..7].iter().all(|c| c.ep_instant));
        assert!(!cylinders[7].ep_instant);
        for (index, pressure_psi) in [(0, 45.0), (1, 90.0), (7, 70.0)] {
            assert!(
                (cylinders[index].full_pressure_bar - pressure_psi * 0.0689475729).abs() < 1e-6
            );
        }
        assert_eq!(session.driver_brake, 1.0);
        session.step_realtime(8.0, |_| {});
        assert_eq!(session.velocity_mps(), 0.0);
        assert_eq!(session.gameplay.next_stop_idx, 0);
        assert!(session.gameplay.stop_results.is_empty());
    }
    #[test]
    fn live_session_advances_time_on_smoke_scenario() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let mut session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        session.driver_throttle = 1.0;
        session.driver_direction = 1.0;
        assert_eq!(session.time_s(), 0.0);
        session.step_realtime(5.0, |_| {});
        assert!(session.time_s() > 0.0);
    }

    #[test]
    fn live_realtime_caps_large_scenario_dt() {
        // Chiltern headless uses time_step=1.0; interactive must still advance every frame.
        let scenario_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/scenario_brake_coast.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let Ok(scenario) = load_scenario(&scenario_path) else {
            return;
        };
        let Ok(mut session) = LiveDriveSession::from_scenario(scenario_dir, &scenario) else {
            return;
        };
        assert!(
            (session.dt - 1.0).abs() < 1e-9,
            "fixture should keep headless dt=1; got {}",
            session.dt
        );
        assert!(
            (session.realtime_physics_dt() - LIVE_REALTIME_MAX_DT_S).abs() < 1e-12,
            "realtime dt must be capped"
        );
        session.driver_throttle = 1.0;
        session.driver_direction = 1.0;
        session.step_realtime(0.2, |_| {});
        assert!(
            session.time_s() >= 0.15,
            "0.2s wall-clock must advance sim (~0.2s), not wait for a 1s quantum; got {}",
            session.time_s()
        );
    }

    #[test]
    fn render_interpolation_moves_between_physics_steps_without_advancing_state() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let scenario = load_scenario(&scenario_path).unwrap();
        let mut session =
            LiveDriveSession::from_scenario(scenario_path.parent().unwrap(), &scenario).unwrap();
        session.state.velocity_mps = 10.0;
        let before = session.head_chainage_m();
        session.step_realtime(0.05, |_| {});
        let after = session.head_chainage_m();
        let physics_time = session.time_s();
        assert!(after > before);
        for index in 0..=10 {
            let (edge, position) = session
                .render_position_at_head_offset(0.0, f64::from(index) * 0.005)
                .unwrap();
            assert_eq!(edge, session.state.path_edges[0]);
            let expected = before + (after - before) * f64::from(index) / 10.0;
            assert!((position - expected).abs() < 1e-9);
        }
        assert_eq!(session.head_chainage_m(), after);
        assert_eq!(session.time_s(), physics_time);
        session.arrived = true;
        assert_eq!(
            session.render_position_at_head_offset(0.0, 0.0),
            session.position_at_head_offset(0.0)
        );
    }

    #[test]
    fn render_interpolation_keeps_consist_spacing_across_an_edge_boundary() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let scenario = load_scenario(&scenario_path).unwrap();
        let mut session =
            LiveDriveSession::from_scenario(scenario_path.parent().unwrap(), &scenario).unwrap();
        let boundary = session.path_data.edges[0].length_m;
        session.previous_render_chainage_m = boundary - 0.5;
        apply_start_offset(&mut session.state, &session.path_data, boundary + 0.5);
        for index in 0..=10 {
            let remainder = f64::from(index) * 0.005;
            let chainage = |offset| {
                let (edge, position) = session
                    .render_position_at_head_offset(offset, remainder)
                    .unwrap();
                let edge_index = session
                    .state
                    .path_edges
                    .iter()
                    .position(|id| id == &edge)
                    .unwrap();
                session
                    .path_data
                    .chainage_at_edge_position(edge_index, position)
            };
            let head = chainage(0.0);
            assert!((head - (boundary - 0.5 + f64::from(index) * 0.1)).abs() < 1e-9);
            assert!((head - chainage(-20.0) - 20.0).abs() < 1e-9);
        }
    }

    #[test]
    fn live_session_has_stop_target_for_smoke_mid() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        assert!(
            session
                .gameplay
                .stop_targets
                .iter()
                .any(|s| s.name == "mid"),
            "smoke scenario should schedule stop at node mid"
        );
    }

    #[test]
    fn consist_offsets_are_relative_to_head_after_scenario_start_offset() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        let head_edge = session.current_edge_id().expect("head edge");
        let head_pos = session.pos_on_edge_m();
        let (same_edge, same_pos) = session.position_at_head_offset(0.0).expect("head pose");
        assert_eq!(same_edge, head_edge);
        assert!((same_pos - head_pos).abs() < 1e-6);

        let (rear_edge, rear_pos) = session.position_at_head_offset(-100.0).expect("rear pose");
        assert_eq!(
            rear_edge, head_edge,
            "Paddington start has enough chainage behind the head"
        );
        assert!(
            (rear_pos - (head_pos - 100.0)).abs() < 1e-6,
            "rear must be 100 m behind the head, not clamped to path origin: \
             head={head_pos:.3}, rear={rear_pos:.3}"
        );
    }

    #[test]
    fn live_session_reaches_chiltern_destination_with_throttle() {
        // Short corridor (brake-coast), not the full PAT in scenario.toml (~4000 km waypoints).
        let scenario_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/scenario_brake_coast.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let Ok(mut scenario) = load_scenario(&scenario_path) else {
            return;
        };
        // This regression exercises propulsion and path arrival, without signals.
        scenario.route.assume_signals_clear = true;
        let Ok(mut session) = LiveDriveSession::from_scenario(scenario_dir, &scenario) else {
            return;
        };
        let start_odo = session.state.odometer_m;
        let path_m = session.path_data.total_length_m();
        // n3 → n10770 corridor is a few tens of km; 30 min at speed is enough to arrive.
        for _ in 0..1800 {
            session.driver_throttle = 1.0;
            session.driver_direction = 1.0;
            session.driver_brake = 0.0;
            session.step_realtime(1.0, |_| {});
            if session.arrived {
                break;
            }
        }
        assert!(
            session.arrived,
            "train should reach destination {} under full throttle (odo={:.0}m of {:.0}m)",
            scenario.route.destination, session.state.odometer_m, path_m,
        );
        assert!(
            session.state.odometer_m > start_odo,
            "odometer should advance toward the station"
        );
    }

    #[test]
    fn caution_behind_or_after_a_clear_head_does_not_slow_the_current_block() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let scenario = load_scenario(directory.join("scenario.toml")).unwrap();
        let mut train = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        let edge = train.current_edge_id().unwrap().to_string();
        let base = train.speed_limit_mps();
        let nearest_position = train
            .graph
            .signals_on_edge(&edge)
            .map(|signal| signal.position_m)
            .reduce(f64::min)
            .unwrap();
        train.state.pos_on_edge_m = nearest_position + 0.01;
        assert_eq!(
            train.effective_speed_limit_mps(),
            base,
            "a passed caution must not hold a train at half speed for the rest of a long vector"
        );
        train
            .graph
            .insert_signal(openrailsrs_track::TrackSignal {
                id: "closer-clear".into(),
                edge_id: edge.clone(),
                position_m: 100.0,
                aspect: SignalAspect::Clear,
                clear_after_s: None,
                script: None,
            })
            .unwrap();
        train
            .graph
            .insert_signal(openrailsrs_track::TrackSignal {
                id: "distant-caution".into(),
                edge_id: edge,
                position_m: 200.0,
                aspect: SignalAspect::Caution,
                clear_after_s: None,
                script: None,
            })
            .unwrap();
        train.state.pos_on_edge_m = 1.0;
        assert_eq!(
            train.effective_speed_limit_mps(),
            base,
            "a distant caution beyond a nearer clear head is not the current block"
        );
        train.state.pos_on_edge_m = 150.0;
        assert_eq!(
            train.effective_speed_limit_mps(),
            base * CAUTION_SPEED_FACTOR
        );
    }

    #[test]
    fn live_caution_signal_halves_effective_limit_on_e1() {
        let scenario_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        if !scenario_path.exists() {
            return;
        }
        let scenario_dir = scenario_path.parent().unwrap();
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let session =
            LiveDriveSession::from_scenario(scenario_dir, &scenario).expect("live session");
        assert_eq!(session.current_edge_id(), Some("e1"));
        let base = session.speed_limit_mps();
        let effective = session.effective_speed_limit_mps();
        assert!(
            (effective - base * CAUTION_SPEED_FACTOR).abs() < 1e-6,
            "caution on e1 should halve limit: base={base} effective={effective}"
        );
    }
}
#[cfg(test)]
mod native_lookahead_tests {
    use super::*;
    #[test]
    fn script_context_contains_directed_native_heads_posts_and_actual_train_maximum() {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern_extended");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
        let session = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        let context = session.script_context(0.05);
        assert!(context.signals.len() > 1 && context.signals.len() <= 32);
        assert!(
            context
                .signals
                .windows(2)
                .all(|w| w[0].distance_m <= w[1].distance_m)
        );
        assert!(context.distance_signal.is_some());
        assert!(!context.speed_posts.is_empty());
        assert!(
            context
                .speed_posts
                .windows(2)
                .all(|w| w[0].distance_m <= w[1].distance_m)
        );
        assert!(context.train_max_speed_mps > context.speed_limit_mps);
        assert!(context.current_post_speed_limit_mps >= context.speed_limit_mps);
        assert_eq!(session.curve_parameters.len(), session.formation.cars.len());
    }
}

fn default_wiper_mode() -> crate::wiper::WiperMode {
    crate::wiper::WiperMode::Off
}

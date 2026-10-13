//! Deterministic incident reproduction. Only the initial snapshot is restored;
//! subsequent frames apply commands and run physics, never expected positions.
use crate::{LiveDriveSession, LiveTraffic, SessionSnapshot, TrafficSnapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayHeader {
    pub schema: u32,
    pub engine_contract: String,
    pub application_version: String,
    pub open_rails_reference: String,
    pub binary_sha256: String,
    pub resources: BTreeMap<String, String>,
    pub seed: u64,
    pub scope: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayInput {
    pub command: serde_json::Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayOutput {
    pub clock_s: f64,
    pub speed_mps: f64,
    pub odometer_m: f64,
    pub sections: BTreeMap<String, String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayFrame {
    pub clock_s: f64,
    pub dt: f64,
    pub automatic: Option<f64>,
    pub input: ReplayInput,
    pub expected: ReplayOutput,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct IncidentReplay {
    pub header: ReplayHeader,
    pub player: SessionSnapshot,
    pub traffic: Vec<TrafficSnapshot>,
    pub frames: Vec<ReplayFrame>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ReplayReport {
    pub matched: bool,
    pub frames: usize,
    pub clock_s: f64,
    pub first_difference: Option<String>,
    pub original_binary_sha256: String,
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    let v = serde_json::to_value(value).map_err(|e| e.to_string())?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&v).map_err(|e| e.to_string())?)
    ))
}
impl ReplayInput {
    pub fn capture(session: &LiveDriveSession) -> Result<Self, String> {
        if session.refilling.is_some()
            || session.snapshot().script_state.is_some()
            || session.script_tcs.is_some()
        {
            return Err("La grabación Rust no cubre abastecimiento activo ni host C#; detené esa operación antes de grabar".into());
        }
        let s = session.snapshot();
        let c = &session.state;
        let cab=c.cab_equipment.iter().map(|v|serde_json::json!({"window_targets":v.window_targets,"door_target":v.door_target,"pump_command":v.pump_command})).collect::<Vec<_>>();
        let diesel=c.diesel.cars.iter().map(|v|serde_json::json!({"command_running":v.command_running,"cooling_on":v.cooling_on})).collect::<Vec<_>>();
        let weather=c.rail_adhesion.as_ref().map(|v|serde_json::json!({"weather":v.weather,"target":v.weather_target_factor,"sander":v.sander_command}));
        Ok(Self {
            command: serde_json::json!({"driver_throttle":s.driver_throttle,"driver_brake":s.driver_brake,"driver_direction":s.driver_direction,"speed_mul":s.speed_mul,
            "headlights":s.headlights,"cab_light":s.cab_light,"wiper_mode":s.wiper_mode,"wiper_active":s.wiper_active,"horn_until":s.horn_pressed_until_s,
            "equipment_commands":c.vehicle_systems.iter().map(|v|serde_json::json!({"vehicle":v.definition.vehicle,"source":v.energy.selection,"motors":v.motors.iter().map(|m|(m.running,m.isolated,m.fan_enabled)).collect::<Vec<_>>()})).collect::<Vec<_>>(),"electric_brake":c.electric_brake_command,"emergency":c.brake_system.emergency,"independent":c.brake_system.independent_command,"cars":s.formation.cars,"coupled":s.formation.coupled_count,
            "cab":cab,"diesel":diesel,"steam":c.boiler_state.as_ref().map(|b|&b.controls),"pantograph":c.electric.pantograph_command_up,"breaker":c.electric.breaker_command_closed,
            "electric_collectors":c.electric.cars.iter().map(|c|c.pantographs.iter().map(|p|(p.command_up,p.contact_enabled)).collect::<Vec<_>>()).collect::<Vec<_>>(),"active_cab":c.active_cab_vehicle,"vehicle_doors":c.vehicle_systems.iter().map(|c|c.door_target).collect::<Vec<_>>(),"surface_weather":c.surface_conditions.as_ref().map(|s|s.weather),"doors":s.exterior.door.anim_open_target(),"weather":weather,"ambient":c.ambient_temperature_c,"switches":s.switches,"signal_overrides":s.signal_overrides,
            "incident_decisions":c.rail_operations.decisions,"quick_stations":s.gameplay.quick_station_practice,"missed_policy":s.gameplay.missed_stop_policy,"traffic_assistance":s.traffic_brake_assistance}),
        })
    }
    pub fn apply(
        &self,
        session: &mut LiveDriveSession,
        traffic: &mut LiveTraffic,
    ) -> Result<(), String> {
        let v = &self.command;
        let number = |key: &str| {
            v[key]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("Comando inválido: {key}"))
        };
        for key in [
            "driver_throttle",
            "driver_brake",
            "driver_direction",
            "independent",
        ] {
            if !(0. ..=1.).contains(&number(key)?) {
                return Err(format!("Comando fuera de rango: {key}"));
            }
        }
        if let Some(command) = v["electric_brake"].as_f64() {
            if !(0. ..=1.).contains(&command) {
                return Err("Freno eléctrico grabado inválido".into());
            }
            session.state.electric_brake_command = command;
        }
        if let Some(commands) = v["equipment_commands"].as_array() {
            if commands.len() != session.state.vehicle_systems.len() {
                return Err("Equipos grabados incompatibles".into());
            }
            for (input, car) in commands.iter().zip(&mut session.state.vehicle_systems) {
                if input["vehicle"].as_u64() != Some(car.definition.vehicle as u64) {
                    return Err("Vehículo grabado incompatible".into());
                }
                car.energy.selection =
                    serde_json::from_value(input["source"].clone()).map_err(|e| e.to_string())?;
                let motors: Vec<(bool, bool, bool)> =
                    serde_json::from_value(input["motors"].clone()).map_err(|e| e.to_string())?;
                if motors.len() != car.motors.len() {
                    return Err("Motores grabados incompatibles".into());
                }
                for (m, (running, isolated, fan)) in car.motors.iter_mut().zip(motors) {
                    m.running = running;
                    m.isolated = isolated;
                    m.fan_enabled = fan;
                }
            }
        }
        let dt_speed = number("speed_mul")?;
        if !(0.25..=64.).contains(&dt_speed) {
            return Err("Velocidad de reproducción inválida".into());
        }
        let ambient = number("ambient")?;
        if !(-100. ..=65.).contains(&ambient) {
            return Err("Temperatura grabada inválida".into());
        }
        let cars: Vec<crate::operations::CarOperationState> =
            serde_json::from_value(v["cars"].clone()).map_err(|e| e.to_string())?;
        if cars.len() != session.formation.cars.len()
            || cars.len() > 64
            || cars.iter().zip(&session.formation.cars).any(|(a, b)| {
                a.name != b.name
                    || a.mass_kg != b.mass_kg
                    || a.length_m != b.length_m
                    || a.powered != b.powered
            })
        {
            return Err("Comandos de otra formación".into());
        }
        let count = v["coupled"].as_u64().ok_or("Cantidad acoplada inválida")? as usize;
        if count == 0 || count > cars.len() {
            return Err("Cantidad acoplada fuera de rango".into());
        }
        if count < session.formation.coupled_count {
            session.uncouple_after(count - 1)?;
        } else if count > session.formation.coupled_count {
            session.recouple()?;
            if session.formation.coupled_count != count {
                return Err("Acople grabado incompatible".into());
            }
        }
        use crate::operations::CarOperation as O;
        for (i, wanted) in cars.iter().enumerate() {
            for (desired, actual, op) in [
                (
                    wanted.train_bus_connected,
                    session.formation.cars[i].train_bus_connected,
                    O::TrainBus,
                ),
                (
                    wanted.handbrake,
                    session.formation.cars[i].handbrake,
                    O::Handbrake,
                ),
                (
                    wanted.hose_connected,
                    session.formation.cars[i].hose_connected,
                    O::Hose,
                ),
                (
                    wanted.front_cock_open,
                    session.formation.cars[i].front_cock_open,
                    O::FrontCock,
                ),
                (
                    wanted.rear_cock_open,
                    session.formation.cars[i].rear_cock_open,
                    O::RearCock,
                ),
                (
                    wanted.power_on,
                    session.formation.cars[i].power_on,
                    O::Power,
                ),
                (
                    wanted.battery_on,
                    session.formation.cars[i].battery_on,
                    O::Battery,
                ),
                (
                    wanted.train_supply_switch_on,
                    session.formation.cars[i].train_supply_switch_on,
                    O::TrainSupply,
                ),
                (
                    wanted.mu_connected,
                    session.formation.cars[i].mu_connected,
                    O::MultipleUnit,
                ),
            ] {
                if desired != actual {
                    session.operate_car(i, op)?;
                }
            }
        }
        let switches: BTreeMap<String, openrailsrs_track::SwitchPosition> =
            serde_json::from_value(v["switches"].clone()).map_err(|e| e.to_string())?;
        for (id, pos) in switches {
            if session.graph.switch_position(&id) != Some(pos) {
                traffic.dispatch_switch(session, &id)?;
            }
        }
        let signals =
            serde_json::from_value(v["signal_overrides"].clone()).map_err(|e| e.to_string())?;
        if session.signal_overrides != signals {
            session.signal_overrides = signals;
            session.native_signals.needs_refresh = true;
        }
        let decisions: BTreeMap<String, crate::rail_operations::IncidentChoice> =
            serde_json::from_value(v["incident_decisions"].clone()).map_err(|e| e.to_string())?;
        for (id, choice) in decisions {
            if !session.state.rail_operations.decisions.contains_key(&id) {
                if session.state.rail_operations.pending_incident.as_ref() != Some(&id) {
                    return Err("Decisión grabada fuera de su incidente".into());
                }
                session.decide_incident(choice)?;
            }
        }
        let cab = v["cab"].as_array().ok_or("Cabina grabada inválida")?;
        let diesel = v["diesel"].as_array().ok_or("Diésel grabado inválido")?;
        if cab.len() != session.state.cab_equipment.len()
            || diesel.len() != session.state.diesel.cars.len()
        {
            return Err("Equipo grabado incompatible".into());
        }
        for (c, wanted) in session.state.cab_equipment.iter_mut().zip(cab) {
            c.window_targets = serde_json::from_value(wanted["window_targets"].clone())
                .map_err(|e| e.to_string())?;
            c.door_target = wanted["door_target"]
                .as_bool()
                .ok_or("Puerta grabada inválida")?;
            c.pump_command = wanted["pump_command"]
                .as_bool()
                .ok_or("Bomba grabada inválida")?;
        }
        for (c, wanted) in session.state.diesel.cars.iter_mut().zip(diesel) {
            c.command_running = wanted["command_running"]
                .as_bool()
                .ok_or("Motor grabado inválido")?;
            c.cooling_on = wanted["cooling_on"]
                .as_bool()
                .ok_or("Ventilación grabada inválida")?;
        }
        if let Some(boiler) = session.state.boiler_state.as_mut() {
            let controls: crate::steam::SteamControls =
                serde_json::from_value(v["steam"].clone()).map_err(|e| e.to_string())?;
            if ![controls.cutoff, controls.firing, controls.damper]
                .iter()
                .all(|n| n.is_finite() && (0. ..=1.).contains(n))
            {
                return Err("Control de vapor grabado inválido".into());
            }
            boiler.controls = controls;
        }
        if let Some(active) = v.get("active_cab") {
            let active: Option<usize> =
                serde_json::from_value(active.clone()).map_err(|e| e.to_string())?;
            let selected = active.unwrap_or(session.cab_vehicle_index);
            if selected != session.cab_vehicle() {
                session.select_cab(selected)?;
            }
        }
        if let Some(doors) = v.get("vehicle_doors") {
            let targets: Vec<[bool; 2]> =
                serde_json::from_value(doors.clone()).map_err(|e| e.to_string())?;
            if targets.len() != session.state.vehicle_systems.len() {
                return Err("Puertas grabadas incompatibles".into());
            }
            for (car, targets) in session.state.vehicle_systems.iter_mut().zip(targets) {
                if let Some(p) = &car.definition.doors {
                    if !p.left && targets[0] || !p.right && targets[1] {
                        return Err("Lado de puerta grabado no equipado".into());
                    }
                } else if targets != [false; 2] {
                    return Err("El vehículo grabado no tiene puertas".into());
                }
                car.door_target = targets;
            }
        }
        if let Some(weather) = v.get("surface_weather") {
            let input: Option<crate::surface_conditions::SurfaceWeather> =
                serde_json::from_value(weather.clone()).map_err(|e| e.to_string())?;
            if let Some(input) = input {
                if !input.valid() {
                    return Err("Superficie grabada inválida".into());
                }
                session
                    .state
                    .surface_conditions
                    .get_or_insert_with(|| crate::surface_conditions::SurfaceConditions::new(input))
                    .weather = input;
            }
        }
        let flag = |k: &str| {
            v[k].as_bool()
                .ok_or_else(|| format!("Interruptor grabado inválido: {k}"))
        };
        session.state.electric.pantograph_command_up = flag("pantograph")?;
        session.state.electric.breaker_command_closed = flag("breaker")?;
        if let Some(controls) = v.get("electric_collectors") {
            let controls: Vec<Vec<(Option<bool>, bool)>> =
                serde_json::from_value(controls.clone()).map_err(|e| e.to_string())?;
            if controls.len() != session.state.electric.cars.len()
                || controls
                    .iter()
                    .zip(&session.state.electric.cars)
                    .any(|(p, c)| p.len() != c.pantographs.len())
            {
                return Err("Pantógrafos grabados incompatibles".into());
            }
            for (car, controls) in session.state.electric.cars.iter_mut().zip(controls) {
                for (p, (command, contact)) in car.pantographs.iter_mut().zip(controls) {
                    p.command_up = command;
                    p.contact_enabled = contact;
                }
            }
        }

        if session.exterior.door.anim_open_target() != flag("doors")? {
            session.exterior.toggle_door();
        }
        if let Some(rail) = session.state.rail_adhesion.as_mut() {
            let weather = &v["weather"];
            rail.weather =
                serde_json::from_value(weather["weather"].clone()).map_err(|e| e.to_string())?;
            rail.weather_target_factor =
                serde_json::from_value(weather["target"].clone()).map_err(|e| e.to_string())?;
            if rail
                .weather_target_factor
                .is_some_and(|n| !n.is_finite() || !(0.5..=1.).contains(&n))
            {
                return Err("Adherencia grabada inválida".into());
            }
            rail.sander_command = weather["sander"]
                .as_bool()
                .ok_or("Arena grabada inválida")?;
        }
        session.driver_throttle = number("driver_throttle")?;
        session.driver_brake = number("driver_brake")?;
        session.driver_direction = number("driver_direction")?;
        session.speed_mul = dt_speed;
        session.headlights = v["headlights"]
            .as_u64()
            .filter(|n| *n <= 2)
            .ok_or("Faros grabados inválidos")? as u8;
        session.cab_light = flag("cab_light")?;
        session.wiper_active = flag("wiper_active")?;
        session.wiper_mode =
            serde_json::from_value(v["wiper_mode"].clone()).map_err(|e| e.to_string())?;
        session.horn_pressed_until_s = number("horn_until")?;
        session.state.brake_system.emergency = flag("emergency")?;
        session.state.brake_system.independent_command = number("independent")?;
        session.state.ambient_temperature_c = ambient;
        session.gameplay.quick_station_practice = flag("quick_stations")?;
        session.gameplay.missed_stop_policy =
            serde_json::from_value(v["missed_policy"].clone()).map_err(|e| e.to_string())?;
        session.traffic_brake_assistance = flag("traffic_assistance")?;
        Ok(())
    }
}
impl ReplayOutput {
    pub fn capture(player: &LiveDriveSession, traffic: &LiveTraffic) -> Result<Self, String> {
        let p = player.snapshot();
        let mut sections = BTreeMap::new();
        for (name, value) in [
            ("physics", serde_json::to_value(&p.state)),
            ("service", serde_json::to_value(&p.gameplay)),
            ("formation", serde_json::to_value(&p.formation)),
            ("signals", serde_json::to_value(&p.signal_runtime)),
            ("dispatcher", serde_json::to_value(&p.dispatcher)),
            ("traffic", serde_json::to_value(traffic.snapshot())),
        ] {
            sections.insert(name.into(), digest(&value.map_err(|e| e.to_string())?)?);
        }
        Ok(Self {
            clock_s: player.time_s(),
            speed_mps: player.velocity_mps(),
            odometer_m: player.state.odometer_m,
            sections,
        })
    }
}
impl IncidentReplay {
    pub fn start(
        header: ReplayHeader,
        player: &LiveDriveSession,
        traffic: &LiveTraffic,
    ) -> Result<Self, String> {
        ReplayInput::capture(player)?;
        for s in &traffic.services {
            ReplayInput::capture(&s.session)?;
        }
        if header.schema != 1
            || header.engine_contract != "native-rust-replay-v1"
            || header.resources.len() > 256
        {
            return Err("Contrato de grabación inválido".into());
        }
        Ok(Self {
            header,
            player: player.snapshot(),
            traffic: traffic.snapshot(),
            frames: vec![],
        })
    }
    pub fn frame(
        &mut self,
        player: &mut LiveDriveSession,
        traffic: &mut LiveTraffic,
        dt: f64,
        automatic: Option<f64>,
    ) -> Result<(), String> {
        if !dt.is_finite()
            || !(0. ..=1.).contains(&dt)
            || automatic.is_some_and(|n| !n.is_finite() || !(0. ..=1.).contains(&n))
            || self.frames.len() >= 12000
        {
            return Err("Límite de grabación alcanzado o paso inválido".into());
        }
        let input = ReplayInput::capture(player)?;
        let clock_s = player.time_s();
        traffic.advance(player, dt, automatic, |_| {});
        self.frames.push(ReplayFrame {
            clock_s,
            dt,
            automatic,
            input,
            expected: ReplayOutput::capture(player, traffic)?,
        });
        Ok(())
    }
    pub fn replay(
        &self,
        player: &mut LiveDriveSession,
        traffic: &mut LiveTraffic,
        resources: &BTreeMap<String, String>,
    ) -> Result<ReplayReport, String> {
        if self.header.schema != 1
            || self.header.engine_contract != "native-rust-replay-v1"
            || self.header.application_version != env!("CARGO_PKG_VERSION")
            || self.header.open_rails_reference != "1.6.1"
            || self.header.resources != *resources
            || self.frames.len() > 12000
        {
            return Err("Versión, contenido o tamaño de reproducción incompatible".into());
        }
        player.validate_snapshot(&self.player)?;
        traffic.validate_snapshot(&self.traffic)?;
        SessionSnapshot::validate_shared_dispatcher(
            std::iter::once(&self.player).chain(self.traffic.iter().map(|t| &t.session)),
        )?;
        player.restore_snapshot(self.player.clone())?;
        traffic.restore_snapshot(self.traffic.clone())?;
        let mut report = ReplayReport {
            matched: true,
            frames: 0,
            clock_s: player.time_s(),
            first_difference: None,
            original_binary_sha256: self.header.binary_sha256.clone(),
        };
        for (index, frame) in self.frames.iter().enumerate() {
            if !frame.dt.is_finite()
                || !(0. ..=1.).contains(&frame.dt)
                || !frame.clock_s.is_finite()
                || frame
                    .automatic
                    .is_some_and(|n| !n.is_finite() || !(0. ..=1.).contains(&n))
            {
                return Err("Paso grabado inválido".into());
            }
            if (player.time_s() - frame.clock_s).abs() > 1e-8 {
                return Err(format!("Reloj divergente antes del paso {index}"));
            }
            frame.input.apply(player, traffic)?;
            traffic.advance(player, frame.dt, frame.automatic, |_| {});
            let actual = ReplayOutput::capture(player, traffic)?;
            report.frames = index + 1;
            report.clock_s = actual.clock_s;
            let different = frame
                .expected
                .sections
                .iter()
                .find(|(name, sha)| actual.sections.get(*name) != Some(*sha));
            if let Some((name, _)) = different {
                report.matched = false;
                report.first_difference = Some(format!(
                    "Paso {index}, {:.6} s, sección {name}: velocidad esperada {:.9}, obtenida {:.9} m/s; odómetro esperado {:.6}, obtenido {:.6} m",
                    actual.clock_s,
                    frame.expected.speed_mps,
                    actual.speed_mps,
                    frame.expected.odometer_m,
                    actual.odometer_m
                ));
                break;
            }
        }
        Ok(report)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (LiveDriveSession, LiveTraffic, ReplayHeader) {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/traction_operation/scenario.toml");
        let s = openrailsrs_scenarios::load_scenario(&p).unwrap();
        let session = LiveDriveSession::from_scenario(p.parent().unwrap(), &s).unwrap();
        let traffic = LiveTraffic::from_scenario(p.parent().unwrap(), &s).unwrap();
        let header = ReplayHeader {
            schema: 1,
            engine_contract: "native-rust-replay-v1".into(),
            application_version: env!("CARGO_PKG_VERSION").into(),
            open_rails_reference: "1.6.1".into(),
            binary_sha256: "a".repeat(64),
            resources: BTreeMap::from([("own-fixture".into(), "b".repeat(64))]),
            seed: s.simulation.seed,
            scope: "own diesel acceptance".into(),
        };
        (session, traffic, header)
    }
    #[test]
    fn input_recording_recomputes_physics_and_reports_first_changed_result() {
        let (mut player, mut traffic, header) = setup();
        let mut replay = IncidentReplay::start(header.clone(), &player, &traffic).unwrap();
        for i in 0..160 {
            player.driver_direction = 1.;
            player.driver_throttle = if i < 100 { 0.65 } else { 0. };
            player.driver_brake = if i >= 120 { 0.5 } else { 0. };
            player.headlights = 2;
            replay.frame(&mut player, &mut traffic, 0.05, None).unwrap();
        }
        let replay: IncidentReplay =
            serde_json::from_slice(&serde_json::to_vec(&replay).unwrap()).unwrap();
        let (mut second, mut traffic2, _) = setup();
        let report = replay
            .replay(&mut second, &mut traffic2, &header.resources)
            .unwrap();
        assert!(report.matched, "{:?}", report);
        assert_eq!(report.frames, 160);
        let mut corrupted = replay.clone();
        corrupted.frames[70].input.command["driver_throttle"] = serde_json::json!(0.);
        let report = corrupted
            .replay(&mut second, &mut traffic2, &header.resources)
            .unwrap();
        assert!(!report.matched);
        assert!(report.first_difference.unwrap().contains("Paso 70"));
        let changed = BTreeMap::new();
        assert!(replay.replay(&mut second, &mut traffic2, &changed).is_err());
    }
}

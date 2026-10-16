use super::{
    DriverControls, FiremanControls, MAX_TRAINS, Member, RemoteCommand, Role, TrainInfo, TrainView,
};
use openrailsrs_sim::{LiveDriveSession, LiveTraffic, SessionSnapshot, TrafficSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldCheckpoint {
    pub version: u32,
    pub player_id: String,
    pub player: SessionSnapshot,
    pub traffic: Vec<TrafficSnapshot>,
    pub control_history: Vec<String>,
    pub couplings: Vec<openrailsrs_sim::independent_coupling::CouplingReceipt>,
}
pub fn capture_world(
    player: &mut LiveDriveSession,
    traffic: &mut LiveTraffic,
    scripts: bool,
) -> Result<WorldCheckpoint, String> {
    if traffic.services.len() + 1 > MAX_TRAINS {
        return Err("La sesión de red admite hasta dieciséis servicios".into());
    }
    Ok(WorldCheckpoint {
        version: 1,
        player_id: player.service_id.clone(),
        player: if scripts {
            player.snapshot_with_scripts()?
        } else {
            player.snapshot()
        },
        traffic: if scripts {
            traffic.snapshot_with_scripts()?
        } else {
            traffic.snapshot()
        },
        control_history: traffic.control_history.clone(),
        couplings: traffic.couplings.clone(),
    })
}
pub fn restore_world(
    player: &mut LiveDriveSession,
    traffic: &mut LiveTraffic,
    saved: WorldCheckpoint,
) -> Result<(), String> {
    if saved.version != 1
        || saved.traffic.len() + 1 > MAX_TRAINS
        || saved.control_history.len() > 128
        || saved.couplings.len() > 64
    {
        return Err("Mundo de red inválido o demasiado grande".into());
    }
    // Callers reconstruct on a disposable world for disk loading. In a live
    // mirror the ownership receipt is unchanged; no physical tick is consumed.
    if player.service_id != saved.player_id
        || traffic.control_history != saved.control_history
        || serde_json::to_value(&traffic.couplings).map_err(|e| e.to_string())?
            != serde_json::to_value(&saved.couplings).map_err(|e| e.to_string())?
    {
        traffic.reconstruct_service_history(
            player,
            &saved.control_history,
            &saved.player_id,
            &saved.couplings,
        )?;
    }
    traffic.restore_shared_snapshots(player, saved.player, saved.traffic)
}
pub fn train_catalog(player: &LiveDriveSession, traffic: &LiveTraffic) -> Vec<TrainInfo> {
    let info = |session: &LiveDriveSession, departed| TrainInfo {
        id: session.service_id.clone(),
        steam: session.state.boiler_state.is_some(),
        cars: session.formation.cars.len(),
        departed,
    };
    std::iter::once(info(player, true))
        .chain(
            traffic
                .services
                .iter()
                .map(|s| info(&s.session, s.departed)),
        )
        .collect()
}
pub fn session<'a>(
    player: &'a LiveDriveSession,
    traffic: &'a LiveTraffic,
    id: &str,
) -> Option<&'a LiveDriveSession> {
    if player.service_id == id {
        Some(player)
    } else {
        traffic
            .services
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.session)
    }
}
pub fn session_mut<'a>(
    player: &'a mut LiveDriveSession,
    traffic: &'a mut LiveTraffic,
    id: &str,
) -> Option<&'a mut LiveDriveSession> {
    if player.service_id == id {
        Some(player)
    } else {
        traffic
            .services
            .iter_mut()
            .find(|s| s.id == id)
            .map(|s| &mut s.session)
    }
}
pub fn driver_controls(session: &LiveDriveSession) -> DriverControls {
    DriverControls {
        throttle: session.driver_throttle,
        train_brake: session.driver_brake,
        independent_brake: session
            .state
            .brake_system
            .has_independent_brake()
            .then_some(session.state.brake_system.independent_command),
        direction: session.driver_direction,
        headlights: session.headlights,
        cab_light: session.cab_light,
        wipers: session.wiper_active,
        horn: session.horn_active(),
    }
}
pub fn fireman_controls(session: &LiveDriveSession) -> Option<FiremanControls> {
    session
        .state
        .boiler_state
        .as_ref()
        .map(|b| FiremanControls {
            automatic: b.controls.automatic_fireman,
            firing: b.controls.firing,
            damper: b.controls.damper,
            injector1: b.controls.injector1,
            injector2: b.controls.injector2,
            blower: b.controls.blower,
        })
}
pub fn train_views(
    player: &LiveDriveSession,
    traffic: &LiveTraffic,
    follow: Option<&str>,
    spectator: bool,
) -> Vec<TrainView> {
    let view = |s: &LiveDriveSession| {
        let current = s
            .state
            .edge_index
            .min(s.state.path_edges.len().saturating_sub(1));
        let start = current.saturating_sub(2048);
        let end = current.saturating_add(33).min(s.state.path_edges.len());
        TrainView {
            id: s.service_id.clone(),
            time_s: s.time_s(),
            edge: s.current_edge_id().unwrap_or("").into(),
            position_m: s.pos_on_edge_m(),
            velocity_mps: s.velocity_mps(),
            path_edges: s.state.path_edges[start..end].to_vec(),
            edge_index: current - start,
            path_reversed: s.state.path_reversed,
            odometer_m: s.state.odometer_m,
            headlights: s.headlights,
            wipers: s.wiper_active,
            driver: driver_controls(s),
            fireman: fireman_controls(s),
            boiler_pressure_bar: s.state.boiler_state.as_ref().map(|b| b.pressure_bar),
            boiler_water_kg: s.state.boiler_state.as_ref().map(|b| b.water_kg),
            cutoff: s.state.boiler_state.as_ref().map(|b| b.controls.cutoff),
            cylinder_cocks: s
                .state
                .boiler_state
                .as_ref()
                .map(|b| b.controls.cylinder_cocks),
            has_local_regulators: s
                .state
                .steam_boilers
                .iter()
                .any(|b| b.local_regulator.is_some()),
        }
    };
    let all = std::iter::once(player).chain(
        traffic
            .services
            .iter()
            .filter(|s| s.departed)
            .map(|s| &s.session),
    );
    // Public observer stream is bounded independently of private simulation
    // snapshots. The selected service is always first, followed by at most7.
    let mut services = all.collect::<Vec<_>>();
    services.sort_by_key(|s| s.service_id != follow.unwrap_or(&player.service_id));
    services
        .into_iter()
        .take(if spectator { 8 } else { MAX_TRAINS })
        .map(view)
        .collect()
}
pub fn safe_stop(player: &mut LiveDriveSession, traffic: &mut LiveTraffic, id: &str) {
    if let Some(s) = session_mut(player, traffic, id) {
        safe_stop_session(s);
    }
}
fn safe_stop_session(s: &mut LiveDriveSession) {
    s.driver_throttle = 0.;
    s.driver_brake = 1.;
    s.stop_horn();
    for engine in &mut s.state.steam_boilers {
        if engine.local_regulator.is_some() {
            engine.local_regulator = Some(0.);
        }
    }
}
fn publish_active_boiler(s: &mut LiveDriveSession) {
    let active = s.state.active_cab_vehicle.unwrap_or(0);
    if let Some(boiler) = &s.state.boiler_state
        && let Some(engine) = s
            .state
            .steam_boilers
            .iter_mut()
            .find(|engine| engine.vehicle == active)
    {
        engine.boiler = boiler.clone();
    }
}
pub fn set_automatic_fireman(
    player: &mut LiveDriveSession,
    traffic: &mut LiveTraffic,
    id: &str,
    automatic: bool,
) {
    if let Some(s) = session_mut(player, traffic, id) {
        let native_without_crew = automatic
            && s.state
                .boiler_state
                .as_ref()
                .is_some_and(|b| b.native_or161.is_some());
        if let Some(boiler) = &mut s.state.boiler_state {
            boiler.controls.automatic_fireman = automatic && boiler.native_or161.is_none();
        }
        if native_without_crew {
            // This manual profile has no AI fireman. Close local regulators
            // as well as the driver's command; a local override must not keep
            // producing traction while service-brake pressure builds.
            safe_stop_session(s);
        }
        publish_active_boiler(s);
    }
}
pub fn apply_command(
    player: &mut LiveDriveSession,
    traffic: &mut LiveTraffic,
    member: &Member,
    command: &RemoteCommand,
) -> Result<(), String> {
    if !command.valid() {
        return Err("Mando fuera de rango".into());
    }
    match command {
        RemoteCommand::Switch { node, position } if member.role == Role::Dispatcher => {
            let old = player
                .graph
                .switch_position(node)
                .ok_or("Cambio desconocido")?;
            if old != *position {
                traffic.dispatch_switch(player, node)?;
            }
            Ok(())
        }
        RemoteCommand::Signal { id, aspect } if member.role == Role::Dispatcher => {
            traffic.synchronize_occupancy(player);
            // Validate the same order on every service before publication.
            for s in std::iter::once(&*player).chain(traffic.services.iter().map(|s| &s.session)) {
                let signal = s
                    .graph
                    .signal(id)
                    .ok_or("Señal desconocida en un servicio")?;
                let occupied = s.occupied_edges();
                let base = signal.edge_id.strip_suffix("_r").unwrap_or(&signal.edge_id);
                if matches!(
                    aspect,
                    Some(
                        openrailsrs_track::SignalAspect::Clear
                            | openrailsrs_track::SignalAspect::Caution
                    )
                ) && (occupied.contains_key(base) || occupied.contains_key(&format!("{base}_r")))
                {
                    return Err("El tramo está ocupado".into());
                }
            }
            player.dispatch_signal(id, *aspect)?;
            for service in &mut traffic.services {
                service.session.dispatch_signal(id, *aspect)?;
            }
            Ok(())
        }
        RemoteCommand::Priority { train, priority } if member.role == Role::Dispatcher => {
            session_mut(player, traffic, train)
                .ok_or("Servicio desconocido")?
                .dispatcher
                .priority = i8::try_from(*priority).map_err(|_| "Prioridad fuera de rango")?;
            Ok(())
        }
        RemoteCommand::Drive { controls } if member.role == Role::Driver => {
            let train = member.train.as_deref().ok_or("Puesto sin tren")?;
            let s = session_mut(player, traffic, train).ok_or("Servicio desconocido")?;
            if controls.independent_brake.is_some() && !s.state.brake_system.has_independent_brake()
            {
                return Err("El tren no tiene freno independiente".into());
            }
            s.set_direction(controls.direction)?;
            if let Some(value) = controls.independent_brake {
                s.set_independent_brake(value)?;
            }
            s.driver_throttle = controls.throttle;
            s.driver_brake = controls.train_brake;
            s.headlights = controls.headlights;
            s.cab_light = controls.cab_light;
            s.wiper_active = controls.wipers;
            s.wiper_mode = if controls.wipers {
                openrailsrs_sim::wiper::WiperMode::Slow
            } else {
                openrailsrs_sim::wiper::WiperMode::Off
            };
            if controls.horn {
                s.trigger_horn(0.5);
            } else {
                s.stop_horn();
            }
            Ok(())
        }
        RemoteCommand::SteamDriver {
            cutoff,
            cylinder_cocks,
        } if member.role == Role::Driver => {
            let s = session_mut(
                player,
                traffic,
                member.train.as_deref().ok_or("Puesto sin tren")?,
            )
            .ok_or("Servicio desconocido")?;
            let boiler = s
                .state
                .boiler_state
                .as_mut()
                .ok_or("El tren no tiene caldera")?;
            if boiler
                .native_or161
                .as_ref()
                .is_some_and(|n| *cutoff > n.hardware.source.cutoff_maximum as f64)
            {
                return Err("El corte supera el máximo del equipo de vapor nativo".into());
            }
            boiler.controls.cutoff = *cutoff;
            boiler.controls.cylinder_cocks = *cylinder_cocks;
            publish_active_boiler(s);
            Ok(())
        }
        RemoteCommand::ResumeRegulators if member.role == Role::Driver => {
            let s = session_mut(
                player,
                traffic,
                member.train.as_deref().ok_or("Puesto sin tren")?,
            )
            .ok_or("Servicio desconocido")?;
            for engine in &mut s.state.steam_boilers {
                engine.local_regulator = None;
            }
            Ok(())
        }
        RemoteCommand::Fire { controls } if member.role == Role::Fireman => {
            let s = session_mut(
                player,
                traffic,
                member.train.as_deref().ok_or("Puesto sin tren")?,
            )
            .ok_or("Servicio desconocido")?;
            let boiler = s
                .state
                .boiler_state
                .as_mut()
                .ok_or("El tren no tiene caldera")?;
            if controls.automatic && boiler.native_or161.is_some() {
                return Err("El ciclo de vapor nativo requiere fogonero manual".into());
            }
            boiler.controls.automatic_fireman = controls.automatic;
            boiler.controls.firing = controls.firing;
            boiler.controls.damper = controls.damper;
            boiler.controls.injector1 = controls.injector1;
            boiler.controls.injector2 = controls.injector2;
            boiler.controls.blower = controls.blower;
            publish_active_boiler(s);
            Ok(())
        }
        _ => Err("El rol no autoriza esta operación física".into()),
    }
}

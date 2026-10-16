//! Multiple steam locomotives retain their own boiler and controls.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleBoiler {
    pub vehicle: usize,
    pub params: openrailsrs_train::SteamParams,
    pub boiler: crate::steam::BoilerState,
    /// None follows the existing compatible MU command; Some is a locally
    /// crewed regulator and remains attached to this locomotive's identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_regulator: Option<f64>,
}
impl VehicleBoiler {
    pub fn valid(&self) -> bool {
        self.params.valid()
            && self.boiler.valid_for(&self.params)
            && self
                .local_regulator
                .is_none_or(|v| v.is_finite() && (0. ..=1.).contains(&v))
    }
}
pub fn command_vehicle(
    state: &mut crate::TrainSimState,
    vehicle: usize,
    command: crate::steam::SteamCommand,
) -> Result<(), String> {
    let active = state.active_cab_vehicle.unwrap_or(0);
    if let Some(b) = state
        .steam_boilers
        .iter_mut()
        .find(|b| b.vehicle == vehicle)
    {
        b.boiler.command(command);
        if active == vehicle {
            state.boiler_state = Some(b.boiler.clone());
        }
        return Ok(());
    }
    if vehicle == active
        && state.steam_boilers.is_empty()
        && let Some(b) = &mut state.boiler_state
    {
        b.command(command);
        return Ok(());
    }
    Err("El vehículo seleccionado no tiene una caldera independiente".into())
}
pub fn set_local_regulator(
    state: &mut crate::TrainSimState,
    vehicle: usize,
    value: Option<f64>,
) -> Result<(), String> {
    if value.is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v)) {
        return Err("Regulador local fuera de 0..1".into());
    }
    let b = state
        .steam_boilers
        .iter_mut()
        .find(|b| b.vehicle == vehicle)
        .ok_or("El vehículo no tiene caldera independiente")?;
    b.local_regulator = value;
    Ok(())
}
pub fn configure(
    state: &mut crate::TrainSimState,
    consist: &openrailsrs_train::Consist,
) -> Result<(), String> {
    let cars = consist
        .vehicles
        .iter()
        .enumerate()
        .filter_map(|(i, v)| match v {
            openrailsrs_train::Vehicle::Loco(l) => l.steam.as_ref().map(|p| (i, p)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let native = cars
        .iter()
        .filter(|(_, p)| p.operation.native_or161.is_some())
        .collect::<Vec<_>>();
    if native.iter().any(|(_, p)| {
        p.operation
            .native_or161
            .as_ref()
            .is_some_and(|p| p.tender_required)
    }) {
        return Err("El ciclo de vapor OR 1.6.1 integrado admite locomotoras tanque; el ténder externo requiere vincular su equipo nativo".into());
    }
    if !native.is_empty()
        && state
            .vehicle_systems
            .iter()
            .any(|c| c.definition.auxiliaries.is_some())
    {
        return Err("El ciclo de vapor OR 1.6.1 no admite cargas auxiliares genéricas adicionales; usa sus equipos nativos".into());
    }
    if !native.is_empty() && state.brake_system.cylinders.len() != consist.vehicles.len() {
        return Err(
            "El ciclo de vapor OR 1.6.1 requiere equipo de vacío en cada vehículo acoplado".into(),
        );
    }
    if !native.is_empty() && (cars.len() != 1 || native[0].0 != 0 || !native[0].1.valid()) {
        return Err(
            "El ciclo de vapor OR 1.6.1 requiere una única locomotora de vapor válida al frente"
                .into(),
        );
    }
    if cars.len() > 1
        || cars.iter().any(|(_, p)| {
            p.operation.thermodynamics.is_some() || p.operation.native_or161.is_some()
        })
    {
        state.steam_boilers = cars
            .into_iter()
            .map(|(vehicle, params)| VehicleBoiler {
                vehicle,
                params: params.clone(),
                boiler: crate::steam::BoilerState::from_params(params),
                local_regulator: None,
            })
            .collect();
        if let Some(boiler) = state.steam_boilers.first_mut()
            && let Some(native) = &mut boiler.boiler.native_or161
        {
            state.brake_system.initialize_native_steam_vacuum(native)?;
            state.brake = native.hardware.source.initial_train_brake as f64;
        }
        state.boiler_state = state.steam_boilers.first().map(|b| b.boiler.clone());
    }
    Ok(())
}
/// Propagate the native vacuum pipe before the locomotive's thermal update.
/// No observed reference values enter this boundary: environment and direction
/// are taken from the running simulation, pressure from its brake equipment.
pub(crate) fn prepare_native_brakes(
    state: &mut crate::TrainSimState,
    command: f64,
    dt: f64,
    backwards: bool,
) -> bool {
    let Some(boiler) = state.boiler_state.as_mut() else {
        return false;
    };
    let Some(native) = boiler.native_or161.as_mut() else {
        return false;
    };
    native.feedback.outside_c = state.ambient_temperature_c as f32;
    native.feedback.signed_speed_mps = (if backwards {
        -state.velocity_mps
    } else {
        state.velocity_mps
    }) as f32;
    // Native height is the brake model's sea-level context, independent of a
    // route tile's local Y. The bounded profile starts at sea level.
    if state
        .brake_system
        .step_native_steam_vacuum(native, command, dt, native.feedback.height_m as f64)
        .is_err()
    {
        state.brake_system.emergency = true;
        state.traction_inhibited = true;
    }
    true
}
pub fn advance(
    state: &mut crate::TrainSimState,
    train: &crate::physics::TrainPhysics,
    throttle: f64,
    speed: f64,
    dt: f64,
) -> f64 {
    let active = state.active_cab_vehicle.unwrap_or(0);
    if let Some(b) = state.steam_boilers.iter_mut().find(|b| b.vehicle == active)
        && let Some(snapshot) = &state.boiler_state
    {
        b.boiler = snapshot.clone();
    }
    let commands = state
        .steam_boilers
        .iter()
        .map(|b| {
            (
                b.vehicle,
                effective_regulator(state, train, b.vehicle, throttle, b.local_regulator),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut total = 0.;
    for b in &mut state.steam_boilers {
        if b.vehicle >= train.vehicle_lengths_m.len() {
            continue;
        }
        let aux = state
            .vehicle_systems
            .iter()
            .find(|c| c.definition.vehicle == b.vehicle)
            .and_then(|c| c.definition.auxiliaries.as_ref());
        let force = crate::steam::steam_step_with_equipment(
            &mut b.boiler,
            &b.params,
            commands[&b.vehicle],
            speed,
            dt,
            f64::INFINITY,
            aux,
        );
        if let Some(slot) = state.motor_force_n.get_mut(b.vehicle) {
            *slot = force;
        }
        total += force;
    }
    state.boiler_state = state
        .steam_boilers
        .iter()
        .find(|b| b.vehicle == active)
        .map(|b| b.boiler.clone());
    total
}

/// A local crew chooses its regulator independently, but cannot bypass a
/// disconnected locomotive, train protection, doors or a physical power cut.
/// Steam's battery policy remains the same as the common steam regulator;
/// `enabled`/`master_key_on` are the operational isolation authority.
pub fn effective_regulator(
    state: &crate::TrainSimState,
    train: &crate::physics::TrainPhysics,
    vehicle: usize,
    common: f64,
    local: Option<f64>,
) -> f64 {
    let powered = train
        .power_supply
        .cars
        .iter()
        .find(|c| c.vehicle == vehicle)
        .is_none_or(|c| c.enabled && c.master_key_on);
    if vehicle >= train.vehicle_lengths_m.len()
        || !powered
        || !state.power_supply.power_available(vehicle)
        || state.traction_inhibited
        || state.brake_system.emergency
        || crate::derailment::any_active(state, train)
        || !crate::vehicle_systems::doors_allow_traction(state, train.vehicle_lengths_m.len())
        || (0.25..0.75).contains(&state.driver_reverser)
    {
        return 0.;
    }
    let requested = local.unwrap_or_else(|| crate::multiple_unit::throttle(state, vehicle, common));
    let requested = train.native.as_ref().map_or(requested, |native| {
        native.throttle(
            requested,
            state
                .brake_system
                .cylinders
                .first()
                .map_or(0., |c| c.pressure_bar()),
        )
    });
    crate::brake_equipment::traction_limit(state, vehicle, requested).clamp(0., 1.)
}

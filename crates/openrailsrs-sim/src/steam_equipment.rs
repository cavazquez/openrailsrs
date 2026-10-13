//! Multiple steam locomotives retain their own boiler and controls.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleBoiler {
    pub vehicle: usize,
    pub params: openrailsrs_train::SteamParams,
    pub boiler: crate::steam::BoilerState,
}
pub fn configure(state: &mut crate::TrainSimState, consist: &openrailsrs_train::Consist) {
    let cars = consist
        .vehicles
        .iter()
        .enumerate()
        .filter_map(|(i, v)| match v {
            openrailsrs_train::Vehicle::Loco(l) => l.steam.as_ref().map(|p| (i, p)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if cars.len() > 1 {
        state.steam_boilers = cars
            .into_iter()
            .map(|(vehicle, params)| VehicleBoiler {
                vehicle,
                params: params.clone(),
                boiler: crate::steam::BoilerState::from_params(params),
            })
            .collect();
        state.boiler_state = state.steam_boilers.first().map(|b| b.boiler.clone());
    }
}
pub fn advance(
    state: &mut crate::TrainSimState,
    count: usize,
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
                crate::multiple_unit::throttle(state, b.vehicle, throttle),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut total = 0.;
    for b in &mut state.steam_boilers {
        if b.vehicle >= count {
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

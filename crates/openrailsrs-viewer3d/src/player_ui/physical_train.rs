//! Controls and diagnostics read the same physical state used by the solver.
use super::*;
use openrailsrs_sim::steam::SteamCommand as S;

pub(super) fn build_controls(p: &mut ChildSpawnerCommands<'_>, s: &LiveDriveSession) {
    for boiler in &s.state.steam_boilers {
        let vehicle = boiler.vehicle;
        label(
            p,
            format!("Vapor · locomotora {}", vehicle + 1),
            17.,
            ACCENT,
        );
        row(p, |p| {
            button(
                p,
                "Seguir regulador común",
                UiCommand::SteamRegulator(vehicle, None),
            );
            let current = boiler.local_regulator.unwrap_or(s.driver_throttle);
            button(
                p,
                "Regulador local −",
                UiCommand::SteamRegulator(vehicle, Some((current - 0.1).clamp(0., 1.))),
            );
            button(
                p,
                "Regulador local +",
                UiCommand::SteamRegulator(vehicle, Some((current + 0.1).clamp(0., 1.))),
            );
        });
        row(p, |p| {
            if boiler.boiler.native_or161.is_none() {
                button(
                    p,
                    "Fogonero auto/manual",
                    UiCommand::SteamVehicle(vehicle, S::AutomaticFireman),
                );
            } else {
                label(p, "Fogonero manual · ciclo OR 1.6.1", 14., MUTED);
            }
            button(
                p,
                "Corte −",
                UiCommand::SteamVehicle(vehicle, S::Cutoff(-0.05)),
            );
            button(
                p,
                "Corte +",
                UiCommand::SteamVehicle(vehicle, S::Cutoff(0.05)),
            );
            button(
                p,
                "Purgas",
                UiCommand::SteamVehicle(vehicle, S::CylinderCocks),
            );
        });
        row(p, |p| {
            button(
                p,
                "Fuego −",
                UiCommand::SteamVehicle(vehicle, S::Firing(-0.1)),
            );
            button(
                p,
                "Fuego +",
                UiCommand::SteamVehicle(vehicle, S::Firing(0.1)),
            );
            button(
                p,
                "Tiro −",
                UiCommand::SteamVehicle(vehicle, S::Damper(-0.1)),
            );
            button(
                p,
                "Tiro +",
                UiCommand::SteamVehicle(vehicle, S::Damper(0.1)),
            );
            button(
                p,
                "Inyector 1",
                UiCommand::SteamVehicle(vehicle, S::Injector1),
            );
            button(
                p,
                "Inyector 2",
                UiCommand::SteamVehicle(vehicle, S::Injector2),
            );
            button(p, "Soplador", UiCommand::SteamVehicle(vehicle, S::Blower));
        });
        if boiler.params.operation.compound.is_some() {
            button(
                p,
                "Compound: válvula de arranque",
                UiCommand::SteamVehicle(vehicle, S::CompoundStartingValve),
            );
        }
    }
    for rack in &s.state.rack.cars {
        button(
            p,
            format!(
                "Cremallera {}: {}",
                rack.vehicle + 1,
                if rack.engage_requested {
                    "soltar piñón"
                } else {
                    "solicitar engrane"
                }
            ),
            UiCommand::RackEngage(rack.vehicle, !rack.engage_requested),
        );
    }
    if s.physics
        .bogies
        .iter()
        .any(|p| p.vertical_suspension.is_some())
    {
        let enabled = s
            .state
            .bogies
            .iter()
            .zip(&s.physics.bogies)
            .any(|(state, model)| {
                state
                    .vertical
                    .as_ref()
                    .zip(model.vertical_suspension.as_ref())
                    .is_some_and(|(state, model)| state.enabled(model))
            });
        button(
            p,
            format!("Irregularidades físicas: {}", yes(enabled)),
            UiCommand::Irregularities(!enabled),
        );
    }
    if s.physics.bogies.iter().any(|p| p.derailment.is_some()) {
        let enabled = s
            .state
            .bogies
            .iter()
            .zip(&s.physics.bogies)
            .any(|(state, model)| {
                state
                    .derailment
                    .as_ref()
                    .zip(model.derailment.as_ref())
                    .is_some_and(|(state, model)| state.enabled(model))
            });
        button(
            p,
            format!("Consecuencias de descarrilamiento: {}", yes(enabled)),
            UiCommand::Derailment(!enabled),
        );
        label(
            p,
            "La causa queda registrada al desactivar las consecuencias. Para recuperar una formación descarrilada, cargá un guardado anterior o reiniciá el ejercicio.",
            14.,
            MUTED,
        );
    }
}

pub(super) fn diagnostics(s: &LiveDriveSession) -> String {
    let mut text = String::new();
    for car in &s.state.diesel.cars {
        if let Some(hydraulic) = &car.hydraulic {
            let status = if hydraulic.overtemperature {
                "protección por temperatura"
            } else if hydraulic.shift_remaining_s > 0. {
                "cambiando"
            } else if hydraulic.lock_up {
                "bloqueado"
            } else {
                "convertidor"
            };
            text += &format!(
                "Hidráulica · vehículo {} · motor {} · marcha {} · {}\nMotor {:.0} RPM · bomba {:.0} RPM · turbina {:.0} RPM\nRelación de velocidad {:.3} · relación de par {:.3} · cambio restante {:.2} s\nPar bomba {:.1} Nm · turbina {:.1} Nm · esfuerzo convertidor {:.1} kN\nEntrada {:.1} kW · salida {:.1} kW · pérdidas {:.1} kW\nAceite {:.1} °C · calor {:.3} MJ · refrigerado {:.3} MJ\nCombustible {:.3} L · consumido {:.3} L · caudal {:.2} L/h\n",
                car.vehicle + 1,
                car.phase.label(),
                hydraulic.gear + 1,
                status,
                car.rpm,
                hydraulic.pump_rpm,
                hydraulic.turbine_rpm,
                hydraulic.speed_ratio,
                hydraulic.torque_ratio,
                hydraulic.shift_remaining_s,
                hydraulic.pump_torque_nm,
                hydraulic.turbine_torque_nm,
                hydraulic.force_n / 1000.,
                hydraulic.input_power_w / 1000.,
                hydraulic.output_power_w / 1000.,
                hydraulic.loss_power_w / 1000.,
                hydraulic.temperature_c,
                hydraulic.heat_j / 1e6,
                hydraulic.cooled_j / 1e6,
                car.fuel_l,
                car.consumed_l,
                car.flow_lps * 3600.
            );
        }
    }
    for b in &s.state.steam_boilers {
        text += &format!(
            "Caldera {}: {:.2} bar · agua {:.0} kg · regulador {}\n",
            b.vehicle + 1,
            b.boiler.pressure_bar,
            b.boiler.water_kg,
            b.local_regulator
                .map_or_else(|| "común".into(), |v| format!("local {:.0}%", v * 100.))
        );
        if b.boiler.native_or161.is_some() {
            text += "Ciclo OR 1.6.1 · fogonero manual\n";
        }
        if let Some(t) = &b.boiler.thermodynamics {
            text += &format!(
                "Masa {:.2} kg · energía {:.3} MJ · vapor {:.2} kg · receptor {:.2} bar · HP/LP {:.1}/{:.1} kN\n",
                t.total_mass_kg,
                t.internal_energy_j / 1e6,
                t.steam_mass_kg,
                t.receiver_pressure_bar,
                t.hp_force_n / 1000.,
                t.lp_force_n / 1000.
            );
        }
    }
    for rack in &s.state.rack.cars {
        text += &format!(
            "Cremallera {}: {:?} · fuerza {:.1} kN · freno {:.1} kN · trabajo {:.2} MJ{}{}\n",
            rack.vehicle + 1,
            rack.phase,
            rack.transmitted_force_n / 1000.,
            rack.brake_force_n / 1000.,
            rack.brake_work_j / 1e6,
            if rack.incompatible {
                " · sistema incompatible"
            } else {
                ""
            },
            if rack.overspeed {
                " · exceso de velocidad"
            } else {
                ""
            }
        );
    }
    for (index, b) in s.state.bogies.iter().enumerate() {
        if let Some(v) = &b.vertical {
            let wheels = v.supports.iter().flat_map(|p| &p.wheels);
            let unloaded = wheels.clone().filter(|w| w.contact_lost).count();
            text += &format!(
                "Suspensión {}: carga normal {:.2} · contactos perdidos {}\n",
                index + 1,
                v.normal_factor(),
                unloaded
            );
        }
    }
    for (index, event) in s.derailment_events() {
        text += &format!(
            "DESCARRILAMIENTO {}: {} · t={:.2}s · {:.1}m · {:.1}km/h · fuerza lateral {:.1}kN · carga mínima {:.1}kN\n",
            index + 1,
            event.cause.map_or("Sin causa", |c| c.label()),
            event.event_time_s.unwrap_or(0.),
            event.event_chainage_m.unwrap_or(0.),
            event.event_speed_mps * 3.6,
            event.lateral_force_n / 1000.,
            event.minimum_wheel_load_n / 1000.
        );
    }
    text
}

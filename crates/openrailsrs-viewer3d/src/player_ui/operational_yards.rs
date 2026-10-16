use super::*;
use openrailsrs_sim::operational_yards::YardCommand as Y;

pub(super) fn build(p: &mut ChildSpawnerCommands<'_>, live: &LiveDrive) {
    let world = &live.session.state.yard_operations;
    label(
        p,
        "Las órdenes usan las mismas vías, vehículos, frenos y reservas de la partida. Cerrá esta ventana para que avance el reloj.",
        14.,
        MUTED,
    );
    if world.definition.is_empty() {
        label(
            p,
            "Este escenario no declara mesas, depósitos ni ejercicios de patio. Podés elegir los ejemplos de Patios operativos en Nueva partida.",
            14.,
            CAUTION,
        );
        return;
    }
    if !world.notice.is_empty() {
        label(p, &world.notice, 14., ACCENT);
    }
    for device in &world.definition.moving_devices {
        let Some(state) = world.devices.get(&device.id) else {
            continue;
        };
        label(
            p,
            format!(
                "{} · {:?} · posición {:.3} · {}",
                device.id,
                device.kind,
                state.position,
                if state.target_port.is_some() {
                    "En movimiento, vías bloqueadas"
                } else {
                    "Alineada y enclavada"
                }
            ),
            16.,
            TEXT,
        );
        label(
            p,
            format!(
                "Tablero {:.1} m · capacidad {:.0} t",
                device.deck_length_m,
                device.max_mass_kg / 1000.
            ),
            13.,
            MUTED,
        );
        row(p, |p| {
            for (port, destination) in device.ports.iter().enumerate() {
                button(
                    p,
                    format!("Alinear con {}", destination.id),
                    UiCommand::Yard(Y::MoveDevice {
                        device: device.id.clone(),
                        port,
                    }),
                );
            }
        });
    }
    for pool in &world.definition.pools {
        label(
            p,
            format!(
                "Depósito {} · {} [{:.0}–{:.0} m]",
                pool.id, pool.edge, pool.start_m, pool.end_m
            ),
            16.,
            TEXT,
        );
        for job in &pool.jobs {
            button(
                p,
                format!(
                    "Reservar formación para {} · {:.0}–{:.0} s",
                    job.id, job.not_before_s, job.deadline_s
                ),
                UiCommand::Yard(Y::ReservePool {
                    pool: pool.id.clone(),
                    job: job.id.clone(),
                }),
            );
        }
        for member in &pool.members {
            if let Some(state) = world.pool_members.get(member) {
                label(
                    p,
                    format!(
                        "{} · tarea {} · {} · devoluciones {}",
                        member,
                        state.reserved_job.as_deref().unwrap_or("Disponible"),
                        if state.dispatched {
                            "En servicio"
                        } else {
                            "En depósito"
                        },
                        state.returns
                    ),
                    14.,
                    MUTED,
                );
                row(p, |p| {
                    button(
                        p,
                        "Despachar",
                        UiCommand::Yard(Y::DispatchPool {
                            member: member.clone(),
                        }),
                    );
                    button(
                        p,
                        "Devolver",
                        UiCommand::Yard(Y::ReturnPool {
                            member: member.clone(),
                        }),
                    );
                });
            }
        }
    }
    for meeting in &world.definition.rendezvous {
        let state = world.rendezvous.get(&meeting.id);
        label(
            p,
            format!(
                "Encuentro {} · {} recibe de {} · {}",
                meeting.id,
                meeting.receiver,
                meeting.donor,
                state.map_or("Pendiente", |s| if s.complete {
                    "Completado"
                } else if s.failed {
                    "Error: ver diagnóstico"
                } else {
                    "Esperando contacto físico"
                })
            ),
            14.,
            TEXT,
        );
        if let Some(s) = state
            && !s.notice.is_empty()
        {
            label(p, &s.notice, 13., CAUTION);
        }
    }
    for run in &world.definition.run_arounds {
        button(
            p,
            format!("Iniciar rodeo de {} · {}", run.service, run.id),
            UiCommand::Yard(Y::StartRunAround { id: run.id.clone() }),
        );
        if let Some(s) = world.run_arounds.get(&run.id) {
            label(p, format!("Fase {} · {}", s.phase, s.notice), 13., MUTED);
        }
    }
    for release in &world.definition.gravity_releases {
        if !world.released.contains(&release.id) {
            row(p, |p| {
                for (route, path) in release.routes.iter().enumerate() {
                    button(
                        p,
                        format!("{}: clasificar hacia {}", release.id, path.id),
                        UiCommand::Yard(Y::RouteGravity {
                            id: release.id.clone(),
                            route,
                        }),
                    );
                }
            });
            button(
                p,
                format!("Liberar por gravedad: {}", release.id),
                UiCommand::Yard(Y::ReleaseGravity {
                    id: release.id.clone(),
                }),
            );
        } else {
            label(
                p,
                format!(
                    "{}: vagones liberados; siguen su vía y frenos físicos",
                    release.id
                ),
                14.,
                MUTED,
            );
            button(
                p,
                "Asegurar en vía de clasificación",
                UiCommand::Yard(Y::SecureGravity {
                    id: release.id.clone(),
                }),
            );
        }
    }
    for retarder in &world.definition.retarders {
        let state = world.retarders.get(&retarder.id);
        let force = std::iter::once(&live.session)
            .chain(live.traffic.services.iter().map(|s| &s.session))
            .filter_map(|s| s.state.yard_operations.retarders.get(&retarder.id))
            .flat_map(|r| &r.last_forces)
            .map(|(_, force, _)| force)
            .sum::<f64>();
        label(
            p,
            format!(
                "Retardador {} · {:.0}% · fuerza {:.1} kN · energía absorbida {:.1} kJ",
                retarder.id,
                state.map_or(0., |s| s.fraction) * 100.,
                force / 1000.,
                state.map_or(0., |s| s.absorbed_j.values().sum::<f64>()) / 1000.
            ),
            14.,
            TEXT,
        );
        row(p, |p| {
            for fraction in [0., 0.25, 0.5, 0.75, 1.] {
                button(
                    p,
                    format!("{:.0}%", fraction * 100.),
                    UiCommand::Yard(Y::SetRetarder {
                        id: retarder.id.clone(),
                        fraction,
                    }),
                );
            }
        });
    }
    for exercise in &world.definition.recovery_exercises {
        label(p, format!("Recuperación {}", exercise.id), 16., TEXT);
        row(p, |p| {
            button(
                p,
                "Proteger vía",
                UiCommand::Yard(Y::ProtectRecovery {
                    id: exercise.id.clone(),
                }),
            );
            button(
                p,
                "Asegurar vagones",
                UiCommand::Yard(Y::SecureRecovery {
                    id: exercise.id.clone(),
                }),
            );
        });
        if let Some(s) = world.recovery.get(&exercise.id) {
            label(
                p,
                format!(
                    "Protegida: {} · recuperada: {} {}",
                    s.protected_at_s.is_some(),
                    s.recovered_at_s.is_some(),
                    s.failed.as_deref().unwrap_or("")
                ),
                13.,
                MUTED,
            );
        }
    }
    for contact in world.contacts.iter().rev().take(8) {
        label(
            p,
            format!(
                "Contacto {} / {} · {:.2} m/s · {:.1} kJ{}",
                contact.left,
                contact.right,
                contact.relative_speed_mps,
                contact.absorbed_j / 1000.,
                if contact.unsafe_impact {
                    " · impacto excesivo"
                } else {
                    ""
                }
            ),
            13.,
            if contact.unsafe_impact { BAD } else { MUTED },
        );
    }
}

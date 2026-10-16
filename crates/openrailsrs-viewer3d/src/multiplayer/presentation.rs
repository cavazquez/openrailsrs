use super::*;

pub fn apply_observer(live: &mut LiveDrive, views: &[TrainView]) -> Result<(), String> {
    if views.len() > 8 {
        return Err("Demasiados trenes en la vista de espectador".into());
    }
    // Validate the whole public batch before publishing any pose.
    for view in views {
        let session =
            openrailsrs_game::multiplayer::world::session(&live.session, &live.traffic, &view.id)
                .ok_or("Servicio observado desconocido")?;
        if !view.time_s.is_finite()
            || view.time_s < 0.
            || !view.position_m.is_finite()
            || view.position_m < 0.
            || !view.velocity_mps.is_finite()
            || view.velocity_mps.abs() > 200.
            || !view.odometer_m.is_finite()
            || view.odometer_m < 0.
            || view.path_edges.is_empty()
            || view.path_edges.len() > 2081
            || view.edge_index >= view.path_edges.len()
            || view.path_edges[view.edge_index] != view.edge
            || !view.driver.valid()
            || view.headlights > 2
            || view
                .boiler_pressure_bar
                .is_some_and(|v| !v.is_finite() || v < 0.)
            || view
                .boiler_water_kg
                .is_some_and(|v| !v.is_finite() || v < 0.)
            || view
                .path_edges
                .iter()
                .any(|id| !valid_id(id) || session.graph.edge(id).is_none())
        {
            return Err("Pose pública inválida".into());
        }
        let edge = session
            .graph
            .edge(&view.edge)
            .ok_or("Vía observada no instalada")?;
        if view.position_m > edge.length_m + 0.01 {
            return Err("Posición de observación fuera de la vía".into());
        }
        for pair in view.path_edges.windows(2) {
            if session.graph.edge(&pair[0]).unwrap().to
                != session.graph.edge(&pair[1]).unwrap().from
            {
                return Err("Trayectoria observada discontinua".into());
            }
        }
    }
    for view in views {
        let session = openrailsrs_game::multiplayer::world::session_mut(
            &mut live.session,
            &mut live.traffic,
            &view.id,
        )
        .expect("validated above");
        if session.state.path_edges != view.path_edges {
            session.state.path_edges = view.path_edges.clone();
            session.path_data =
                openrailsrs_sim::path_data::PathData::from_path(&view.path_edges, &session.graph);
        }
        session.state.edge_index = view.edge_index;
        session.state.pos_on_edge_m = view.position_m;
        session.state.path_reversed = view.path_reversed;
        session.state.velocity_mps = view.velocity_mps;
        session.state.time.0 = view.time_s;
        session.state.odometer_m = view.odometer_m;
        session.headlights = view.headlights;
        session.wiper_active = view.wipers;
        session.cab_light = view.driver.cab_light;
        if view.driver.horn {
            session.trigger_horn(0.5);
        } else {
            session.stop_horn();
        }
        session.driver_throttle = view.driver.throttle;
        session.driver_brake = view.driver.train_brake;
        session.driver_direction = view.driver.direction;
        session.reset_remote_presentation();
        if let Some(service) = live.traffic.services.iter_mut().find(|s| s.id == view.id) {
            service.departed = true;
        }
    }
    Ok(())
}
pub fn camera(
    state: Res<MultiplayerState>,
    live: Option<Res<LiveDrive>>,
    mut follow: Option<ResMut<crate::camera::CameraFollowMode>>,
    mut mode: Option<ResMut<crate::camera::CameraMode>>,
    roots: Query<
        (
            &Transform,
            Option<&crate::live::LiveTrainMarker>,
            Option<&crate::traffic::TrafficTrainMarker>,
        ),
        (
            Without<Camera3d>,
            Or<(
                With<crate::live::LiveTrainMarker>,
                With<crate::traffic::TrafficTrainMarker>,
            )>,
        ),
    >,
    mut cameras: Query<
        (&mut crate::camera::OrbitState, &mut Transform),
        (With<Camera3d>, Without<crate::mirrors::MirrorCamera>),
    >,
) {
    if !state.active() {
        return;
    }
    let Some(live) = live else {
        return;
    };
    let id = state
        .followed
        .as_ref()
        .or(state.train.as_ref())
        .unwrap_or(&live.session.service_id);
    // The ordinary cab remains usable for the primary service; an observer of
    // another service follows its actual rendered model, never a copied train.
    if id == &live.session.service_id && state.role != Role::Spectator {
        return;
    }
    if let Some(follow) = &mut follow {
        **follow = crate::camera::CameraFollowMode::Off;
    }
    if state.camera == ObserverCamera::Free {
        if let Some(mode) = &mut mode {
            **mode = crate::camera::CameraMode::Fly;
        }
        return;
    }
    if let Some(mode) = &mut mode {
        **mode = crate::camera::CameraMode::Orbit;
    }
    let index = if id == &live.session.service_id {
        0
    } else {
        let Some(index) = live.traffic.services.iter().position(|s| &s.id == id) else {
            return;
        };
        index + 1
    };
    let Some((target, _, _)) = roots.iter().find(|(_, player, traffic)| {
        if index == 0 {
            player.is_some()
        } else {
            traffic.is_some_and(|m| m.0 == index)
        }
    }) else {
        return;
    };
    for (mut orbit, mut transform) in &mut cameras {
        orbit.focus = target.translation + Vec3::Y * 2.;
        if state.camera == ObserverCamera::Exterior {
            orbit.yaw =
                crate::camera::chase_yaw_from_train(crate::camera::yaw_from_transform(target));
            orbit.pitch = 0.3;
            orbit.distance = orbit.distance.clamp(15., 150.);
        }
        *transform = crate::camera::camera_transform_from_orbit_state(
            orbit.focus,
            orbit.yaw,
            orbit.pitch,
            orbit.distance,
        );
    }
}

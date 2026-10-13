pub fn apply(
    consist: &mut openrailsrs_train::Consist,
    profile: Option<&openrailsrs_scenarios::freight::FreightService>,
) -> Result<(), crate::SimError> {
    let Some(p) = profile else {
        return Ok(());
    };
    if !p.valid() {
        return Err(crate::SimError::Msg(
            "Servicio de mercancías inválido".into(),
        ));
    }
    // Validate all targets before changing any authored mass.
    if p.loads.iter().any(|l| {
        !matches!(
            consist.vehicles.get(l.vehicle),
            Some(openrailsrs_train::Vehicle::Wagon(_))
        )
    }) {
        return Err(crate::SimError::Msg(
            "La carga requiere un vagón existente".into(),
        ));
    }
    for l in &p.loads {
        if let openrailsrs_train::Vehicle::Wagon(w) = &mut consist.vehicles[l.vehicle] {
            w.mass_kg += l.cargo_kg;
        }
    }
    Ok(())
}
pub fn cap(
    path: &mut crate::path_data::PathData,
    profile: Option<&openrailsrs_scenarios::freight::FreightService>,
) {
    if let Some(speed) = profile.and_then(|p| p.maximum_speed_mps) {
        for edge in &mut path.edges {
            edge.speed_limit_mps = edge.speed_limit_mps.min(speed);
            for p in &mut edge.profile.speed_posts {
                p.speed_limit_kmh = p.speed_limit_kmh.min(speed * 3.6);
            }
        }
    }
}

/// Store the service limit on every edge so rerouting and restoring cannot drop it.
pub fn cap_graph(
    graph: &mut openrailsrs_track::TrackGraph,
    profile: Option<&openrailsrs_scenarios::freight::FreightService>,
) {
    let Some(speed) = profile.and_then(|p| p.maximum_speed_mps) else {
        return;
    };
    let ids = graph
        .edges_iter()
        .map(|(id, _)| id.to_owned())
        .collect::<Vec<_>>();
    for id in ids {
        graph.cap_edge_speed_limit_kmh(&id, speed * 3.6);
        if let Some(mut physics) = graph.physics_profile(&id).cloned() {
            for post in &mut physics.speed_posts {
                post.speed_limit_kmh = post.speed_limit_kmh.min(speed * 3.6);
            }
            graph.set_physics_profile(&id, physics);
        }
    }
}

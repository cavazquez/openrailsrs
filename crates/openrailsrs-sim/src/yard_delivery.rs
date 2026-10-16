//! Yard acceptance derives from saved car positions and physical handbrakes.
//! Taking the wagons away immediately removes the delivery acceptance.
#[derive(Clone, Debug, serde::Serialize)]
pub struct YardDeliveryStatus {
    pub id: String,
    pub destination: String,
    pub accepted: bool,
    pub actual_vehicles: Vec<usize>,
    pub reason: String,
}
impl crate::LiveDriveSession {
    fn car_on_siding(
        &self,
        index: usize,
        def: &openrailsrs_scenarios::rail_operations::YardDeliveryDef,
    ) -> bool {
        let Some(car) = self.formation.cars.get(index) else {
            return false;
        };
        let center = self.formation.center_offset_m(index);
        [-car.length_m * 0.5, car.length_m * 0.5]
            .into_iter()
            .all(|delta| {
                self.presentation_position_for_car(center + delta, center, 0.)
                    .is_some_and(|(edge, pos)| {
                        let base = edge.strip_suffix("_r").unwrap_or(&edge);
                        let coordinate = if edge.ends_with("_r") {
                            self.graph
                                .edge(&edge)
                                .map_or(f64::NAN, |e| e.length_m - pos)
                        } else {
                            pos
                        };
                        base == def.edge.strip_suffix("_r").unwrap_or(&def.edge)
                            && (def.start_m..=def.end_m).contains(&coordinate)
                    })
            })
    }
    pub fn yard_deliveries(&self) -> Vec<YardDeliveryStatus> {
        self.state
            .rail_operations
            .definition
            .yard_deliveries
            .iter()
            .filter(|def| def.service.as_deref().unwrap_or("Jugador") == self.service_id)
            .map(|def| {
                let actual = (0..self.formation.cars.len())
                    .filter(|i| self.car_on_siding(*i, def))
                    .collect::<Vec<_>>();
                let identities = self.vehicle_ids();
                let mut actual_ids = actual
                    .iter()
                    .map(|i| identities[*i].clone())
                    .collect::<Vec<_>>();
                actual_ids.sort();
                let mut expected = def
                    .vehicles
                    .iter()
                    .map(|i| format!("{}/car/{i}", self.service_id))
                    .collect::<Vec<_>>();
                expected.sort();
                let composition = actual_ids == expected;
                let secured = actual.iter().all(|i| {
                    (*i >= self.formation.coupled_count
                        || self.formation.cars[..self.formation.coupled_count]
                            .iter()
                            .all(|c| !c.powered))
                        && self.formation.cars[*i].handbrake
                });
                let accepted = composition && secured && self.velocity_mps() < 0.1;
                YardDeliveryStatus {
                    id: def.id.clone(),
                    destination: def.destination.clone(),
                    accepted,
                    actual_vehicles: actual,
                    reason: if !composition {
                        "La composición o su posición no coincide"
                    } else if !secured {
                        "Separá y asegurá los vagones con freno de mano"
                    } else if self.velocity_mps() >= 0.1 {
                        "Detené la locomotora para verificar la entrega"
                    } else {
                        "Composición entregada y asegurada"
                    }
                    .into(),
                }
            })
            .collect()
    }
}

impl crate::LiveTraffic {
    pub fn yard_inventory(&self, player: &crate::LiveDriveSession) -> Vec<YardDeliveryStatus> {
        player
            .state
            .rail_operations
            .definition
            .yard_deliveries
            .iter()
            .map(|def| {
                let desired_service = def.service.as_deref().unwrap_or("Jugador");
                let actual = std::iter::once(player)
                    .chain(
                        self.services
                            .iter()
                            .filter(|s| s.departed)
                            .map(|s| &s.session),
                    )
                    .flat_map(|session| {
                        (0..session.formation.cars.len())
                            .filter(|i| session.car_on_siding(*i, def))
                            .map(move |i| (session, i))
                    })
                    .collect::<Vec<_>>();
                let mut ids = actual
                    .iter()
                    .map(|(s, i)| s.vehicle_ids()[*i].clone())
                    .collect::<Vec<_>>();
                ids.sort();
                let mut expected = def
                    .vehicles
                    .iter()
                    .map(|i| format!("{desired_service}/car/{i}"))
                    .collect::<Vec<_>>();
                expected.sort();
                let composition = ids == expected;
                let secured = actual.iter().all(|(s, i)| {
                    (*i >= s.formation.coupled_count
                        || s.formation.cars[..s.formation.coupled_count]
                            .iter()
                            .all(|c| !c.powered))
                        && s.formation.cars[*i].handbrake
                        && s.velocity_mps().abs() < 0.1
                });
                YardDeliveryStatus {
                    id: def.id.clone(),
                    destination: def.destination.clone(),
                    accepted: composition && secured,
                    actual_vehicles: actual.iter().map(|(_, i)| *i).collect(),
                    reason: if !composition {
                        "Faltan vagones de este destino o hay vehículos ajenos en la vía"
                    } else if !secured {
                        "Separá y asegurá los vagones con freno de mano"
                    } else {
                        "Composición entregada y asegurada"
                    }
                    .into(),
                }
            })
            .collect()
    }
}

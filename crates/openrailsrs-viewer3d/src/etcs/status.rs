//! Synthetic `ETCSStatus` for the DMI (no TCS scripting yet).

use openrailsrs_sim::LiveDriveSession;

#[derive(Clone, Debug, PartialEq)]
pub struct EtcsStatus {
    pub active: bool,
    pub speed_kmh: f64,
    pub allowed_kmh: f64,
    pub target_kmh: Option<f64>,
    pub target_distance_m: Option<f64>,
    pub intervention_kmh: f64,
    pub overspeed: bool,
    /// Dial full-scale (standard OR scale ≥ allowed).
    pub dial_max_kmh: u32,
}

impl Default for EtcsStatus {
    fn default() -> Self {
        Self {
            active: true,
            speed_kmh: 0.0,
            allowed_kmh: 80.0,
            target_kmh: None,
            target_distance_m: None,
            intervention_kmh: 85.0,
            overspeed: false,
            dial_max_kmh: 140,
        }
    }
}

impl EtcsStatus {
    pub fn from_telemetry(
        speed_kmh: f64,
        allowed_kmh: f64,
        overspeed: bool,
        target_distance_m: Option<f64>,
    ) -> Self {
        let allowed = allowed_kmh.max(0.0);
        // Soft target: next reduction ≈ 80% of allowed when approaching a stop/limit.
        let target_kmh = target_distance_m.and_then(|d| {
            if d < 3000.0 && allowed > 10.0 {
                Some((allowed * 0.8).max(0.0))
            } else {
                None
            }
        });
        let intervention = if overspeed {
            speed_kmh.max(allowed) + 5.0
        } else {
            allowed + (allowed * 0.05).max(5.0)
        };
        Self {
            active: true,
            speed_kmh: speed_kmh.max(0.0),
            allowed_kmh: allowed,
            target_kmh,
            target_distance_m,
            intervention_kmh: intervention,
            overspeed,
            dial_max_kmh: pick_dial_scale(allowed.max(speed_kmh)),
        }
    }
}

pub fn etcs_status_from_live(session: &LiveDriveSession) -> EtcsStatus {
    let tel = session.cab_telemetry();
    EtcsStatus::from_telemetry(
        tel.speed_kmh,
        tel.limit_kmh,
        tel.overspeed,
        session.distance_to_next_stop_m(),
    )
}

fn pick_dial_scale(need: f64) -> u32 {
    const SCALES: [u32; 8] = [140, 150, 180, 240, 250, 260, 280, 400];
    for s in SCALES {
        if need <= f64::from(s) {
            return s;
        }
    }
    400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dial_scale_picks_standard() {
        assert_eq!(pick_dial_scale(90.0), 140);
        assert_eq!(pick_dial_scale(200.0), 240);
    }

    #[test]
    fn overspeed_raises_intervention() {
        let s = EtcsStatus::from_telemetry(100.0, 80.0, true, Some(500.0));
        assert!(s.intervention_kmh >= 100.0);
        assert!(s.target_kmh.is_some());
    }
}

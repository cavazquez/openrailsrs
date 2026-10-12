//! Reproducible Rust extension: diagnostic state observes real brake/power APIs.
//! Run with `cargo run -p openrailsrs-sim --example rust_vehicle_extension`.
use openrailsrs_sim::{
    CarOperation, LiveDriveSession,
    etcs::{BasicEtcsTcs, EtcsTcs},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DiagnosticState {
    schema: u32,
    last_clock_s: f64,
    low_voltage_s: f64,
    open_hose_s: f64,
}
impl Default for DiagnosticState {
    fn default() -> Self {
        Self {
            schema: 1,
            last_clock_s: 0.,
            low_voltage_s: 0.,
            open_hose_s: 0.,
        }
    }
}
impl DiagnosticState {
    fn valid(&self) -> bool {
        self.schema == 1
            && [self.last_clock_s, self.low_voltage_s, self.open_hose_s]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.)
    }
    fn update(&mut self, session: &LiveDriveSession) -> Result<(), String> {
        if !self.valid() {
            return Err("Invalid extension state/version".into());
        }
        let now = session.time_s();
        if now < self.last_clock_s {
            return Err(
                "Restore extension state together with the session before rewinding".into(),
            );
        }
        let elapsed = now - self.last_clock_s;
        if !session.cab_telemetry().low_voltage_power {
            self.low_voltage_s += elapsed;
        }
        if (0..session.formation.cars.len()).any(|i| session.leaking_hose(i).is_some()) {
            self.open_hose_s += elapsed;
        }
        self.last_clock_s = now;
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/traction_operation/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path)?;
    let mut session = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)?;
    let mut diagnostic = DiagnosticState::default();
    session.operate_car(0, CarOperation::Battery)?;
    for _ in 0..20 {
        session.step_realtime(0.05, |_| {});
        diagnostic.update(&session)?;
    }
    assert!(diagnostic.low_voltage_s > 0.9);
    assert!(!session.cab_telemetry().low_voltage_power);
    // Persist both states on the railway clock, with exact float round trips.
    let bytes = serde_json::to_vec(&(session.snapshot(), &diagnostic))?;
    let (snapshot, mut restored_state): (openrailsrs_sim::SessionSnapshot, DiagnosticState) =
        serde_json::from_slice(&bytes)?;
    let mut restored = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)?;
    restored.restore_snapshot(snapshot)?;
    assert!(restored_state.valid());
    for _ in 0..20 {
        restored.step_realtime(0.05, |_| {});
        restored_state.update(&restored)?;
    }
    assert!((restored_state.low_voltage_s - 2.).abs() < 1e-9);
    restored.operate_car(0, CarOperation::Battery)?;
    for _ in 0..20 {
        restored.step_realtime(0.05, |_| {});
        restored_state.update(&restored)?;
    }
    assert!(restored.cab_telemetry().low_voltage_power);
    let status = BasicEtcsTcs::default().compute(&restored);
    let _ = status;
    restored_state.schema = 99;
    assert!(restored_state.update(&restored).is_err());
    restored_state.schema = 1;
    println!("{}", serde_json::to_string_pretty(&restored_state)?);
    Ok(())
}

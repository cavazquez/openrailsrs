//! Actual C# -> JSONL -> Rust session -> braking / DMI acceptance, without a renderer.
use openrailsrs_sim::{
    LiveDriveSession,
    etcs::{EtcsSupervision, ScriptHostConfig, TcsInput},
};
use std::{path::PathBuf, time::Duration};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("Usage: tcs_host_smoke DOTNET HOST_DLL SCRIPT.cs".into());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples/smoke/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&path).map_err(|e| e.to_string())?;
    let mut session = LiveDriveSession::from_scenario(path.parent().unwrap(), &scenario)
        .map_err(|e| e.to_string())?;
    session.attach_script_tcs(&ScriptHostConfig {
        executable: args[1].clone().into(),
        arguments: vec![args[2].clone()],
        script: args[3].clone().into(),
        type_name: "MinimalTcs".into(),
        timeout: Duration::from_millis(250),
    })?;
    let initialized = session.etcs_status();
    assert!(initialized.needs_ack);
    assert_eq!(initialized.supervision, EtcsSupervision::Intervention);
    assert!(session.validate_snapshot(&session.snapshot()).is_err());
    session.driver_direction = 1.0;
    session.driver_throttle = 1.0;
    session.driver_brake = 0.0;
    session.step_realtime(0.1, |_| {});
    assert_eq!(session.state.brake, 1.0);
    session.send_tcs_input(TcsInput::Acknowledge {
        message: "C# TCS listo: confirmar".into(),
    });
    session.step_realtime(0.1, |_| {});
    assert!(!session.etcs_status().needs_ack);
    session.send_tcs_input(TcsInput::Menu {
        action: "restrict".into(),
    });
    session.step_realtime(0.1, |_| {});
    assert!((session.etcs_status().allowed_kmh - 18.0).abs() < 1e-6);
    session.state.velocity_mps = 7.0;
    session.step_realtime(0.1, |_| {});
    assert_eq!(
        session.etcs_status().supervision,
        EtcsSupervision::Intervention
    );
    assert_eq!(session.state.brake, 1.0);
    println!(
        "PASS C# TCS: initialization, 20 Hz ticks, ACK, menu, speed restriction, physical braking, restore guard"
    );
    let extended = root.join("examples/chiltern_extended/scenario.toml");
    let scenario = openrailsrs_scenarios::load_scenario(&extended).map_err(|e| e.to_string())?;
    let mut native = LiveDriveSession::from_scenario(extended.parent().unwrap(), &scenario)
        .map_err(|e| e.to_string())?;
    let signal = native
        .state
        .path_edges
        .iter()
        .enumerate()
        .skip(native.state.edge_index)
        .find_map(|(index, edge)| {
            native
                .graph
                .signals_on_edge(edge)
                .filter(|s| {
                    (index > native.state.edge_index || s.position_m >= native.pos_on_edge_m())
                        && s.script
                            .as_ref()
                            .and_then(|s| s.native.as_ref())
                            .is_some_and(|n| n.function.eq_ignore_ascii_case("NORMAL"))
                })
                .min_by(|a, b| a.position_m.total_cmp(&b.position_m))
                .map(|s| s.id.clone())
        })
        .ok_or("No native normal signal ahead")?;
    native
        .signal_overrides
        .insert(signal, openrailsrs_track::SignalAspect::Stop);
    native.attach_script_tcs(&ScriptHostConfig {
        executable: args[1].clone().into(),
        arguments: vec![args[2].clone()],
        script: root.join("docs/fixtures/tcs/NativeLookaheadTcs.cs"),
        type_name: "NativeLookaheadTcs".into(),
        timeout: Duration::from_millis(250),
    })?;
    assert!(
        native
            .etcs_status()
            .messages
            .iter()
            .any(|m| m.text.contains("Stop"))
    );
    native.state.velocity_mps = 2.;
    native.driver_throttle = 1.;
    native.driver_direction = 1.;
    native.step_realtime(0.1, |_| {});
    assert_eq!(native.state.throttle, 0.);
    assert_eq!(native.state.brake, 1.);
    println!("PASS native SIGSCR -> indexed C# aspect -> physical train brake on Chiltern");
    Ok(())
}

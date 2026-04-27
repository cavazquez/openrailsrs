use openrailsrs_audio::{AudioCmd, AudioEngine};

#[test]
fn try_start_is_ci_safe() {
    // The CI runner usually has no audio device; `try_start` must not panic
    // and may legally return either `Some` or `None`. We just assert the call
    // succeeds without aborting the process.
    let _ = AudioEngine::try_start();
}

#[test]
fn engine_accepts_all_commands_when_available() {
    let Some(engine) = AudioEngine::try_start() else {
        return;
    };

    engine.send(AudioCmd::SetVelocity(20.0));
    engine.send(AudioCmd::SetBraking(0.5));
    engine.send(AudioCmd::EnterRegion {
        id: "depot1".into(),
        kind: "depot".into(),
        base_volume: 0.4,
    });
    engine.send(AudioCmd::EnterRegion {
        id: "tunnel1".into(),
        kind: "tunnel".into(),
        base_volume: 0.6,
    });
    engine.send(AudioCmd::LeaveRegion {
        id: "depot1".into(),
    });
    engine.send(AudioCmd::Horn);
    engine.send(AudioCmd::LeaveRegion {
        id: "tunnel1".into(),
    });
    // `Drop` will send `Stop`.
}

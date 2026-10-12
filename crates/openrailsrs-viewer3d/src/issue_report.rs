//! Explicit, reviewable bug reports. No environment, machine username, source
//! directory, log dump, credential store or script payload enters the report.
use bevy::prelude::*;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
};
#[derive(Serialize)]
pub struct Diagnostic {
    schema: u32,
    application: &'static str,
    version: &'static str,
    binary_sha256: Option<String>,
    bevy: &'static str,
    openrails_reference: &'static str,
    renderer: String,
    view_distance_m: f32,
    weather: String,
    weather_budget: usize,
    physics: Option<Physics>,
    missing_files: Vec<String>,
}
#[derive(Serialize)]
struct Physics {
    seconds: f64,
    distance_m: f64,
    speed_mps: f64,
    throttle: f64,
    brake: f64,
    reverser: f64,
    mass_kg: f64,
    brake_pipe_bar: f64,
    brake_cylinder_bar: f64,
    main_power: bool,
    doors: String,
    stops_served: usize,
    stops_missed: usize,
}
impl Diagnostic {
    pub fn capture(
        settings: &crate::player_settings::PlayerSettings,
        live: Option<&crate::live::LiveDrive>,
        audit: Option<&openrailsrs_train::ConsistAudit>,
    ) -> Self {
        Self {
            schema: 1,
            application: "openrailsrs",
            version: env!("CARGO_PKG_VERSION"),
            binary_sha256: None,
            bevy: "0.19.1",
            openrails_reference: "1.6.1",
            renderer: settings.renderer.label().into(),
            view_distance_m: settings.view_distance_m,
            weather: settings.weather_profile.label().into(),
            weather_budget: settings.weather_particle_budget,
            physics: live.map(|l| {
                let s = &l.session;
                let t = s.cab_telemetry();
                Physics {
                    seconds: s.time_s(),
                    distance_m: s.state.odometer_m,
                    speed_mps: s.velocity_mps(),
                    throttle: s.driver_throttle,
                    brake: s.driver_brake,
                    reverser: s.driver_direction,
                    mass_kg: s.effective_mass_kg(),
                    brake_pipe_bar: t.brake_pipe_bar,
                    brake_cylinder_bar: t.brake_cyl_bar,
                    main_power: t.main_power,
                    doors: format!("{:?}", s.exterior.door),
                    stops_served: s.gameplay.stop_results.len(),
                    stops_missed: s.gameplay.missed_stops.len(),
                }
            }),
            missing_files: audit
                .into_iter()
                .flat_map(|a| &a.missing_resources)
                .map(|r| {
                    std::path::Path::new(&r.name.replace('\\', "/"))
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .chars()
                        .take(256)
                        .collect()
                })
                .take(128)
                .collect(),
        }
    }
}
#[derive(Resource, Default)]
pub struct Reports {
    pub directory: Option<PathBuf>,
    pub preview: String,
    pub status: String,
    pending: Option<Mutex<mpsc::Receiver<Result<(PathBuf, String), String>>>>,
}
impl Reports {
    pub fn prepare(&mut self, diagnostic: Diagnostic) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("El informe se está preparando".into());
        }
        let root = crate::player_settings::player_data_dir().join("reports");
        let (tx, rx) = mpsc::channel();
        self.pending = Some(Mutex::new(rx));
        self.status = "Preparando datos para que los revises…".into();
        std::thread::spawn(move || {
            let mut diagnostic = diagnostic;
            diagnostic.binary_sha256 = std::env::current_exe()
                .ok()
                .and_then(|p| openrailsrs_train::consist_audit::content_file_sha256(&p).ok());
            let result = (|| {
                let directory = root.join(format!(
                    "report-{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
                let json = serde_json::to_string_pretty(&diagnostic).map_err(|e| e.to_string())?;
                let preview = format!(
                    "Descripción del problema:\n\nPasos para reproducirlo:\n\nResultado esperado:\n\nDiagnóstico revisable:\n{json}\n"
                );
                crate::player_settings::atomic_write(
                    &directory.join("diagnostic.json"),
                    json.as_bytes(),
                )?;
                crate::player_settings::atomic_write(
                    &directory.join("report.txt"),
                    preview.as_bytes(),
                )?;
                Ok((directory, preview))
            })();
            let _ = tx.send(result);
        });
        Ok(())
    }
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.pending else {
            return false;
        };
        let Ok(receiver) = receiver.try_lock() else {
            return false;
        };
        let result = match receiver.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("No se pudo preparar el informe".into()),
        };
        drop(receiver);
        self.pending = None;
        match result {
            Ok((path, preview)) => {
                self.directory = Some(path);
                self.preview = preview;
                self.status =
                    "Informe listo. Revisá los datos y la captura antes de compartirlos.".into();
            }
            Err(error) => self.status = error,
        }
        true
    }
    pub fn open_issue(&self) -> Result<(), String> {
        if self.directory.is_none() {
            return Err("Prepará y revisá primero el informe".into());
        }
        let encode = |s: &str| {
            s.bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        };
        crate::official_content::open_source_url(&format!(
            "https://github.com/cavazquez/openrailsrs/issues/new?title={}&body={}",
            encode("Problema durante una partida"),
            encode(&self.preview)
        ))
    }
}
#[derive(Resource)]
pub struct PendingCapture {
    pub path: PathBuf,
    frames: u8,
    hidden: Vec<(Entity, Visibility)>,
    paused_before: Option<bool>,
    #[cfg(feature = "dev-inspector")]
    inspector: Option<bool>,
}
impl PendingCapture {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            frames: 0,
            hidden: vec![],
            paused_before: None,
            #[cfg(feature = "dev-inspector")]
            inspector: None,
        }
    }
}
pub fn capture(
    mut commands: Commands,
    mut pending: Option<ResMut<PendingCapture>>,
    mut nodes: Query<(Entity, &mut Visibility), With<Node>>,
    mut live: Option<ResMut<crate::live::LiveDrive>>,
    #[cfg(feature = "dev-inspector")] mut inspector: Option<
        ResMut<crate::dev_inspector::InspectorState>,
    >,
) {
    let Some(p) = pending.as_mut() else {
        return;
    };
    if p.frames == 0 {
        if let Some(l) = live.as_mut() {
            p.paused_before = Some(l.paused);
            l.paused = true;
        }
        for (entity, mut visibility) in &mut nodes {
            p.hidden.push((entity, *visibility));
            *visibility = Visibility::Hidden;
        }
        #[cfg(feature = "dev-inspector")]
        if let Some(inspector) = inspector.as_mut() {
            p.inspector = Some(inspector.enabled);
            inspector.enabled = false;
        }
    }
    p.frames += 1;
    if p.frames == 3 {
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(p.path.clone()));
    }
    if p.frames >= 8 {
        if let Some(l) = live.as_mut() {
            l.paused = p.paused_before.unwrap_or(l.paused);
        }
        for (entity, old) in &p.hidden {
            if let Ok((_, mut visibility)) = nodes.get_mut(*entity) {
                *visibility = *old;
            }
        }
        #[cfg(feature = "dev-inspector")]
        if let Some(inspector) = inspector.as_mut()
            && let Some(enabled) = p.inspector
        {
            inspector.enabled = enabled;
        }
        commands.remove_resource::<PendingCapture>();
    }
}
/// The graphical regression opts into this with both screenshot capture and a
/// dev-tools flag. It exercises the same preparation/capture as the UI buttons.
#[cfg(feature = "dev-tools")]
pub fn regression_capture(
    mut commands: Commands,
    mut live: Option<ResMut<crate::live::LiveDrive>>,
    settings: Res<crate::player_settings::PlayerSettings>,
    mut reports: ResMut<Reports>,
    mut started: Local<bool>,
    mut requested: Local<bool>,
    mut equipment: Local<bool>,
    mut ready_frames: Local<u32>,
    progress: Option<Res<crate::world::WorldSpawnProgress>>,
    scene: crate::capture::CaptureScene,
) {
    if !crate::capture::capture_enabled() {
        return;
    }
    let Some(live) = live.as_mut() else {
        return;
    };
    if !*equipment && std::env::var("OPENRAILSRS_CAPTURE_EQUIPMENT").as_deref() == Ok("1") {
        *equipment = true;
        for operation in [
            openrailsrs_sim::cab_equipment::CabOperation::LeftWindow,
            openrailsrs_sim::cab_equipment::CabOperation::RightWindow,
            openrailsrs_sim::cab_equipment::CabOperation::Door,
            openrailsrs_sim::cab_equipment::CabOperation::Washer,
        ] {
            if let Err(error) = live.session.operate_cab(operation) {
                crate::viewer_log!("equipment regression: {error}");
            }
        }
    }
    if *equipment && live.session.time_s() >= 6. {
        live.paused = true;
    }
    if std::env::var("OPENRAILSRS_CAPTURE_BUG_REPORT").as_deref() != Ok("1") {
        return;
    }
    if progress.is_none() && scene.ready() {
        *ready_frames = ready_frames.saturating_add(1);
    } else {
        *ready_frames = 0;
    }
    if !*started && live.session.time_s() >= 5. && *ready_frames >= 60 {
        *started = true;
        if let Err(error) = reports.prepare(Diagnostic::capture(&settings, Some(live), None)) {
            crate::viewer_log!("report regression: {error}");
        }
    }
    if !*requested && let Some(directory) = &reports.directory {
        commands.insert_resource(PendingCapture::new(directory.join("screenshot.png")));
        *requested = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_restores_ui_visibility_and_previous_pause_state() {
        let mut app = App::new();
        let mut live =
            crate::live::LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path())
                .unwrap();
        live.paused = false;
        app.insert_resource(live)
            .insert_resource(PendingCapture::new("unused.png".into()))
            .add_systems(Update, capture);
        let node = app
            .world_mut()
            .spawn((Node::default(), Visibility::Visible))
            .id();
        app.update();
        assert!(app.world().resource::<crate::live::LiveDrive>().paused);
        assert_eq!(
            app.world().get::<Visibility>(node),
            Some(&Visibility::Hidden)
        );
        for _ in 0..7 {
            app.update();
        }
        assert!(!app.world().resource::<crate::live::LiveDrive>().paused);
        assert_eq!(
            app.world().get::<Visibility>(node),
            Some(&Visibility::Visible)
        );
        assert!(!app.world().contains_resource::<PendingCapture>());
    }
    #[test]
    fn report_does_not_include_paths_or_environment_secrets() {
        let settings = crate::player_settings::PlayerSettings::default();
        let audit = openrailsrs_train::ConsistAudit {
            path: "/home/private/secret.con".into(),
            missing_resources: vec![openrailsrs_train::MissingResource {
                name: "/home/private/Stock/wagon.eng".into(),
                referenced_by: "/home/private/secret.con".into(),
                destinations: vec!["/home/private/wagon.eng".into()],
                required: true,
            }],
            ..default()
        };
        let report =
            serde_json::to_string(&Diagnostic::capture(&settings, None, Some(&audit))).unwrap();
        assert!(!report.contains("/home"));
        assert!(!report.contains("secret"));
        assert!(report.contains("wagon.eng"));
    }
}

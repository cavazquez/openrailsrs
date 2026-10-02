//! Open Rails-style driving information and track monitor, using native Bevy UI.

use bevy::prelude::*;
use openrailsrs_sim::ServicePhase;

use crate::live::LiveDrive;

pub struct DrivingHudPlugin;

impl Plugin for DrivingHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrivingHudVisibility>()
            .add_systems(
                OnEnter(crate::route_bootstrap::ViewerAppState::Playing),
                spawn_driving_hud.run_if(resource_exists::<LiveDrive>),
            )
            .add_systems(Update, (toggle_driving_hud, update_driving_hud).chain());
    }
}

#[derive(Resource)]
struct DrivingHudVisibility {
    driving: bool,
    monitor: bool,
    debug: bool,
}

impl Default for DrivingHudVisibility {
    fn default() -> Self {
        Self {
            driving: true,
            monitor: true,
            debug: false,
        }
    }
}

#[derive(Component)]
struct DrivingInfo;
#[derive(Component)]
struct TrackMonitor;
#[derive(Component)]
struct DrivingHudPanel;

fn spawn_driving_hud(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    // Bevy's subset default font lacks Spanish glyphs. Bundle a licensed font
    // so HUD text renders identically without relying on system installations.
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../assets/fonts/DejaVuSansMono.ttf").to_vec(),
    ));
    for (left, right, component) in [
        (Val::Px(12.0), Val::Auto, true),
        (Val::Auto, Val::Px(12.0), false),
    ] {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left,
                    right,
                    top: Val::Px(12.0),
                    width: Val::Px(if component { 280.0 } else { 320.0 }),
                    padding: UiRect::all(Val::Px(10.0)),
                    max_width: Val::Percent(45.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                Visibility::Hidden,
                ZIndex(110),
                DrivingHudPanel,
            ))
            .with_children(|panel| {
                let mut text = panel.spawn((
                    Text::new(""),
                    TextFont {
                        font: bevy::text::FontSource::Handle(font.clone()),
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(Color::srgb(0.96, 0.96, 0.96)),
                ));
                if component {
                    text.insert(DrivingInfo);
                } else {
                    text.insert(TrackMonitor);
                }
            });
    }
}

fn toggle_driving_hud(
    keys: Res<ButtonInput<KeyCode>>,
    mut visibility: ResMut<DrivingHudVisibility>,
) {
    if keys.just_pressed(KeyCode::F5) {
        visibility.driving = !visibility.driving;
    }
    if keys.just_pressed(KeyCode::F4) {
        visibility.monitor = !visibility.monitor;
    }
    if keys.just_pressed(KeyCode::F3) {
        visibility.debug = !visibility.debug;
    }
}

pub fn service_instruction(session: &openrailsrs_sim::LiveDriveSession) -> String {
    let gp = &session.gameplay;
    match gp.phase {
        ServicePhase::Boarding => format!(
            "Abrir puertas Q · parada {:.0} s",
            gp.remaining_dwell_s(session.time_s())
        ),
        ServicePhase::ReadyToDepart => "Cerrar puertas Q · salida autorizada".into(),
        ServicePhase::Completed => "Servicio completado · R reinicia".into(),
        ServicePhase::Failed => gp
            .failure
            .clone()
            .unwrap_or_else(|| "Servicio fallido · R reinicia".into()),
        ServicePhase::Approaching => "Detenerse en el punto de parada".into(),
    }
}

fn update_driving_hud(
    live: Option<Res<LiveDrive>>,
    visibility: Res<DrivingHudVisibility>,
    mut info: Query<(&mut Text, &ChildOf), (With<DrivingInfo>, Without<TrackMonitor>)>,
    mut monitor: Query<(&mut Text, &ChildOf), (With<TrackMonitor>, Without<DrivingInfo>)>,
    mut panels: Query<&mut Visibility, (With<DrivingHudPanel>, Without<crate::hud::HudRoot>)>,
    mut debug: Query<&mut Visibility, (With<crate::hud::HudRoot>, Without<DrivingHudPanel>)>,
    time: Res<Time>,
    mut elapsed: Local<f32>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.05 && !visibility.is_changed() {
        return;
    }
    *elapsed = 0.0;
    let Some(live) = live else {
        return;
    };
    let session = &live.session;
    let cab = session.cab_telemetry();
    let direction = if cab.direction >= 0.75 {
        "Adelante"
    } else if cab.direction <= 0.25 {
        "Atrás"
    } else {
        "Neutro"
    };
    let status = if live.paused { "PAUSA" } else { "CONDUCCIÓN" };
    let door = match session.exterior.door {
        openrailsrs_sim::exterior::DoorState::Closed => "Cerradas",
        openrailsrs_sim::exterior::DoorState::Opening => "Abriendo",
        openrailsrs_sim::exterior::DoorState::Open => "Abiertas",
        openrailsrs_sim::exterior::DoorState::Closing => "Cerrando",
    };
    let content = format!(
        "{status}\nVelocidad  {:.1} km/h\nLímite  {:.0} km/h\nRegulador  {:.0}%\nInversor  {direction}\nFreno de tren  {:.0}%\nTubería  {:.2} bar\nCilindro  {:.2} bar\nPuertas  {door}\nF5 HUD · F4 monitor\nPause pausa",
        cab.speed_kmh,
        cab.limit_kmh,
        cab.throttle_pct,
        cab.brake_pct,
        cab.brake_pipe_bar,
        cab.brake_cyl_bar,
    );
    for (mut text, parent) in &mut info {
        if let Ok(mut shown) = panels.get_mut(parent.parent()) {
            shown.set_if_neq(if visibility.driving {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        if text.0 != content {
            text.0 = content.clone();
        }
    }
    let stop = session
        .next_stop_label()
        .unwrap_or(&session.gameplay.destination);
    let distance = session.distance_to_next_stop_m().unwrap_or(0.0);
    let next_signal = session.next_signal_ahead();
    let signal = next_signal
        .map(|(distance, aspect)| format!("Señal  {aspect:?} · {distance:.0} m"))
        .unwrap_or_else(|| "Señal  —".into());
    let content = format!(
        "MONITOR DE VÍA\nPróxima estación  {stop}\nDistancia  {distance:.0} m\n{signal}\nParadas  {} / {}\n{}\n1 cabina · Alt+1 2D/3D · 2 exterior\nA/D regulador · ;/' freno · W/S inversor\nSpace bocina · V limpiaparabrisas",
        session.gameplay.passed_stops.len(),
        session.gameplay.stop_targets.len(),
        service_instruction(session)
    );
    for (mut text, parent) in &mut monitor {
        if let Ok(mut shown) = panels.get_mut(parent.parent()) {
            shown.set_if_neq(if visibility.monitor {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        if text.0 != content {
            text.0 = content.clone();
        }
    }
    for mut shown in &mut debug {
        shown.set_if_neq(if visibility.debug {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

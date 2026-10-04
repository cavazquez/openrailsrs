//! Compact Open Rails-style information panels, using retained Bevy UI.

use bevy::prelude::*;
use openrailsrs_sim::ServicePhase;
use openrailsrs_track::SignalAspect;

use crate::live::LiveDrive;

const TEXT: Color = Color::srgb(0.94, 0.96, 0.98);
const MUTED: Color = Color::srgb(0.68, 0.73, 0.78);
const GOOD: Color = Color::srgb(0.48, 0.91, 0.64);
const CAUTION: Color = Color::srgb(1.0, 0.79, 0.30);
const ALERT: Color = Color::srgb(1.0, 0.40, 0.36);

pub struct DrivingHudPlugin;

impl Plugin for DrivingHudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DrivingHudVisibility>()
            .add_systems(
                OnEnter(crate::route_bootstrap::ViewerAppState::Playing),
                spawn_driving_hud.run_if(resource_exists::<LiveDrive>),
            )
            .add_systems(
                Update,
                (
                    toggle_driving_hud.run_if(crate::player_ui::world_input_available),
                    update_driving_hud,
                )
                    .chain()
                    .run_if(crate::teleport::teleport_closed),
            );
    }
}

#[derive(Resource)]
struct DrivingHudVisibility {
    driving: bool,
    monitor: bool,
    debug: bool,
    help: bool,
}

impl Default for DrivingHudVisibility {
    fn default() -> Self {
        Self {
            driving: true,
            monitor: true,
            debug: false,
            help: false,
        }
    }
}

#[derive(Component, Clone, Copy)]
enum HudPanel {
    Driving,
    Monitor,
    Help,
}

#[derive(Component, Clone, Copy)]
enum HudField {
    Status,
    Speed,
    Limit,
    Controls,
    Brakes,
    Doors,
    Lights,
    Clock,
    Station,
    Distance,
    Signal,
    Progress,
    Schedule,
    Instruction,
}

fn spawn_driving_hud(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    // Bundle Spanish glyphs instead of depending on the host's installed fonts.
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../assets/fonts/DejaVuSansMono.ttf").to_vec(),
    ));
    for (kind, right, width, fields) in [
        (
            HudPanel::Driving,
            false,
            248.0,
            vec![
                (HudField::Status, 12.0),
                (HudField::Speed, 24.0),
                (HudField::Limit, 13.0),
                (HudField::Controls, 13.0),
                (HudField::Brakes, 13.0),
                (HudField::Doors, 13.0),
                (HudField::Lights, 12.0),
            ],
        ),
        (
            HudPanel::Monitor,
            true,
            284.0,
            vec![
                (HudField::Clock, 12.0),
                (HudField::Station, 16.0),
                (HudField::Distance, 22.0),
                (HudField::Signal, 14.0),
                (HudField::Schedule, 12.0),
                (HudField::Progress, 12.0),
                (HudField::Instruction, 13.0),
            ],
        ),
    ] {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: if right { Val::Auto } else { Val::Px(12.0) },
                    right: if right { Val::Px(12.0) } else { Val::Auto },
                    top: Val::Px(12.0),
                    width: Val::Px(width),
                    max_width: Val::Percent(42.0),
                    padding: UiRect::all(Val::Px(10.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.72)),
                Visibility::Hidden,
                ZIndex(110),
                kind,
            ))
            .with_children(|panel| {
                for (field, size) in fields {
                    panel.spawn((
                        Text::new(""),
                        TextFont {
                            font: bevy::text::FontSource::Handle(font.clone()),
                            font_size: FontSize::Px(size),
                            ..default()
                        },
                        TextColor(TEXT),
                        field,
                    ));
                }
                panel.spawn((
                    Text::new(if right {
                        "F4 vía · F7 servicio · M mapa"
                    } else {
                        "F5 conducción · F6 ayuda"
                    }),
                    TextFont {
                        font: bevy::text::FontSource::Handle(font.clone()),
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(MUTED),
                ));
            });
    }
    commands.spawn((
        Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(12.0),
            max_width: Val::Percent(90.0), padding: UiRect::all(Val::Px(12.0)), ..default() },
        BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.90)),
        Visibility::Hidden, ZIndex(120), HudPanel::Help,
    )).with_children(|panel| {
        panel.spawn((Text::new(
            "CONTROLES · F6 cierra esta ayuda\n\nD / A  subir / bajar regulador    ' / ;  aplicar / soltar freno\nW / S  adelante / atrás          \\  inversor neutro\nQ  puertas · Space  bocina · V  limpiaparabrisas\nBackspace  emergencia · Pause o P  pausa · R  reiniciar\n+ / -  acelerar / reducir el tiempo\n\n1  cabina · Alt+1  cambiar 2D/3D · 2  exterior · 3  órbita\nF1  cámara orbital · F2  cámara libre · flechas  mover cámara\nRePág / AvPág  subir / bajar cámara · rueda  acercar / alejar\nArrastrar con botón izquierdo  girar · botón central  desplazar\nF4  monitor · F5  conducción · F3  diagnóstico"
        ), TextFont { font: bevy::text::FontSource::Handle(font), font_size: FontSize::Px(13.0), ..default() }, TextColor(TEXT)));
    });
}

fn toggle_driving_hud(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<crate::player_settings::PlayerSettings>,
    mut visibility: ResMut<DrivingHudVisibility>,
) {
    if settings.just_pressed(&keys, crate::player_settings::PlayerAction::DrivingHud) {
        visibility.driving = !visibility.driving;
    }

    if settings.just_pressed(&keys, crate::player_settings::PlayerAction::DebugHud) {
        visibility.debug = !visibility.debug;
    }
}

pub fn service_instruction(session: &openrailsrs_sim::LiveDriveSession) -> String {
    use openrailsrs_sim::exterior::DoorState;
    let gp = &session.gameplay;
    match gp.phase {
        ServicePhase::Boarding if session.exterior.door == DoorState::Closed => {
            "Q · abrir puertas para embarcar".into()
        }
        ServicePhase::Boarding => format!(
            "Embarque · faltan {:.0} s",
            gp.remaining_dwell_s(session.time_s())
        ),
        ServicePhase::ReadyToDepart if session.exterior.door == DoorState::Closed => {
            "Salida autorizada".into()
        }
        ServicePhase::ReadyToDepart => "Q · cerrar puertas para salir".into(),
        ServicePhase::Completed => "Servicio completado · R reinicia".into(),
        ServicePhase::Failed => gp
            .failure
            .clone()
            .unwrap_or_else(|| "Servicio fallido · R reinicia".into()),
        ServicePhase::Approaching => "Detenerse en el punto de parada".into(),
    }
}

fn clock_label(seconds: f64) -> String {
    let seconds = seconds.floor().rem_euclid(86400.0) as u32;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn distance_label(metres: f64) -> String {
    if metres >= 1000.0 {
        format!("{:.2} km", metres / 1000.0)
    } else {
        format!("{metres:.0} m")
    }
}

fn signal_label(aspect: SignalAspect) -> (&'static str, Color) {
    match aspect {
        SignalAspect::Clear => ("Vía libre", GOOD),
        SignalAspect::Caution => ("Precaución", CAUTION),
        SignalAspect::Stop => ("Alto", ALERT),
    }
}

fn update_driving_hud(
    live: Option<Res<LiveDrive>>,
    settings: Option<Res<crate::player_settings::PlayerSettings>>,
    visibility: Res<DrivingHudVisibility>,
    mut fields: Query<(&HudField, &mut Text, &mut TextColor)>,
    mut panels: Query<(&HudPanel, &mut Visibility), Without<crate::hud::HudRoot>>,
    mut debug: Query<&mut Visibility, (With<crate::hud::HudRoot>, Without<HudPanel>)>,
    time: Res<Time>,
    mut elapsed: Local<f32>,
) {
    *elapsed += time.delta_secs();
    if *elapsed < 0.05 && !visibility.is_changed() {
        return;
    }
    *elapsed = 0.0;
    let Some(live) = live else { return };
    for (kind, mut shown) in &mut panels {
        let enabled = match kind {
            HudPanel::Driving => visibility.driving,
            HudPanel::Monitor => visibility.monitor,
            HudPanel::Help => visibility.help,
        };
        shown.set_if_neq(if enabled {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for mut shown in &mut debug {
        shown.set_if_neq(if visibility.debug {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let session = &live.session;
    let cab = session.cab_telemetry();
    let door_key = settings
        .as_ref()
        .map(|s| s.key_label(crate::player_settings::PlayerAction::Doors))
        .unwrap_or_else(|| "Q".into());
    let mph = settings.is_some_and(|s| s.mph);
    let unit = if mph { "mph" } else { "km/h" };
    let factor = if mph { 1.0 / 1.609344 } else { 1.0 };
    for (field, mut text, mut color) in &mut fields {
        let (content, tint) = match field {
            HudField::Status => (
                if live.paused {
                    "PAUSA".into()
                } else if session.speed_mul != 1.0 {
                    format!("CONDUCCIÓN · tiempo ×{:.2}", session.speed_mul)
                } else {
                    "CONDUCCIÓN".into()
                },
                if live.paused { CAUTION } else { MUTED },
            ),
            HudField::Speed => (
                format!("{:.1} {unit}", cab.speed_kmh * factor),
                if cab.speed_kmh > cab.limit_kmh + 5.0 {
                    ALERT
                } else if cab.speed_kmh > cab.limit_kmh + 0.5 {
                    CAUTION
                } else {
                    TEXT
                },
            ),
            HudField::Limit => (
                format!("Límite  {:.0} {unit}", cab.limit_kmh * factor),
                MUTED,
            ),
            HudField::Controls => {
                let direction = if cab.direction >= 0.75 {
                    "Adelante"
                } else if cab.direction <= 0.25 {
                    "Atrás"
                } else {
                    "Neutro"
                };
                (
                    format!(
                        "Regulador {:>3.0}% · {direction}\nFreno de tren {:>3.0}%",
                        cab.throttle_pct, cab.brake_pct
                    ),
                    TEXT,
                )
            }
            HudField::Brakes => (
                format!(
                    "Tubería   {:>4.2} bar\nCilindro  {:>4.2} bar",
                    cab.brake_pipe_bar, cab.brake_cyl_bar
                ),
                MUTED,
            ),
            HudField::Doors => {
                use openrailsrs_sim::exterior::DoorState;
                let (label, tint) = match session.exterior.door {
                    DoorState::Closed => ("Cerradas", TEXT),
                    DoorState::Opening => ("Abriendo", CAUTION),
                    DoorState::Open => ("Abiertas", CAUTION),
                    DoorState::Closing => ("Cerrando", CAUTION),
                };
                (format!("Puertas  {label}"), tint)
            }
            HudField::Lights => (
                format!(
                    "Faros {} · cabina {}\nLimpiaparabrisas {}",
                    match session.headlights {
                        0 => "apagados",
                        1 => "bajos",
                        _ => "altos",
                    },
                    if session.cab_light { "Sí" } else { "No" },
                    if session.wiper_active { "Sí" } else { "No" },
                ),
                MUTED,
            ),
            HudField::Clock => (
                format!("MONITOR DE VÍA   {}", clock_label(live.clock_time_s())),
                MUTED,
            ),
            HudField::Station => (
                session
                    .next_stop_label()
                    .unwrap_or(&session.gameplay.destination)
                    .to_string(),
                TEXT,
            ),
            HudField::Distance => (
                session
                    .distance_to_next_stop_m()
                    .map(distance_label)
                    .unwrap_or_else(|| "Destino alcanzado".into()),
                TEXT,
            ),
            HudField::Signal => session
                .next_signal_ahead()
                .map(|(distance, aspect)| {
                    let (label, color) = signal_label(aspect);
                    (format!("● {label} · {}", distance_label(distance)), color)
                })
                .unwrap_or_else(|| ("Señal  —".into(), MUTED)),
            HudField::Schedule => (
                session
                    .gameplay
                    .stop_targets
                    .get(session.gameplay.next_stop_idx)
                    .map(|stop| {
                        format!(
                            "Llegada prevista {}",
                            clock_label(live.start_clock_s + stop.arrive_s)
                        )
                    })
                    .unwrap_or_else(|| "Fin del recorrido".into()),
                MUTED,
            ),
            HudField::Progress => (
                format!(
                    "Paradas {}/{} · recorrido {:.0}%",
                    session.gameplay.passed_stops.len(),
                    session.gameplay.stop_targets.len(),
                    session.route_progress() * 100.0
                ),
                MUTED,
            ),
            HudField::Instruction => (
                service_instruction(session).replace("Q ·", &format!("{door_key} ·")),
                match session.gameplay.phase {
                    ServicePhase::Completed | ServicePhase::ReadyToDepart => GOOD,
                    ServicePhase::Failed => ALERT,
                    _ => TEXT,
                },
            ),
        };
        if text.0 != content {
            text.0 = content;
        }
        color.set_if_neq(TextColor(tint));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_wraps_at_midnight_without_resetting_elapsed_service_time() {
        assert_eq!(clock_label(35700.0 + 390.0), "10:01:30");
        assert_eq!(clock_label(86401.0), "00:00:01");
    }

    #[test]
    fn signal_status_is_translated_and_keeps_the_safety_color() {
        assert_eq!(signal_label(SignalAspect::Stop), ("Alto", ALERT));
        assert_eq!(signal_label(SignalAspect::Caution), ("Precaución", CAUTION));
        assert_eq!(signal_label(SignalAspect::Clear), ("Vía libre", GOOD));
    }
}

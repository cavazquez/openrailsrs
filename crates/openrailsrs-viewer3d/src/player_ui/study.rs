use super::*;
use crate::journey_setup::{JourneySetup, PlayMode};
use openrailsrs_sim::learning::{Lesson, braking_advice};

pub(super) fn challenge_weather(
    setup: &JourneySetup,
    profile: &mut crate::weather_state::WeatherProfile,
    weather: &mut PlayerWeather,
) {
    if setup.mode != PlayMode::Challenge {
        return;
    }
    use crate::weather_state::WeatherProfile;
    (*profile, *weather) = match setup.lesson {
        Lesson::Snow => (WeatherProfile::HeavySnow, PlayerWeather::Snow),
        Lesson::Fog => (WeatherProfile::Automatic, PlayerWeather::Fog),
        _ => (WeatherProfile::Downpour, PlayerWeather::Rain),
    };
}
pub(super) fn build_practice(
    p: &mut ChildSpawnerCommands<'_>,
    settings: &PlayerSettings,
    menu: &PlayerLaunchMenu,
    live: Option<&LiveDrive>,
) {
    if let Some(l) = live {
        if let Some(lesson) = &l.session.state.learning {
            label(
                p,
                format!(
                    "{} · paso {}/{} · intento {}",
                    lesson.lesson.label(),
                    (lesson.stage + 1).min(lesson.lesson.steps().len()),
                    lesson.lesson.steps().len(),
                    lesson.attempts
                ),
                18.,
                ACCENT,
            );
            label(p, lesson.instruction(), 16., TEXT);
            row(p, |p| {
                button(p, "Repetir este paso", UiCommand::RepeatLesson(false));
                button(p, "Repetir desde el inicio", UiCommand::RepeatLesson(true));
            });
        } else {
            label(
                p,
                "Esta partida no contiene un ejercicio. Elegí un tutorial en Nueva partida para practicar con puntos de repetición.",
                14.,
                TEXT,
            );
        }
        button(
            p,
            "Informe y gráficas",
            UiCommand::Open(PlayerPanel::Report),
        );
    } else {
        label(p, "Modo de partida", 18., TEXT);
        row(p, |p| {
            for mode in PlayMode::ALL {
                button(
                    p,
                    format!(
                        "{}{}",
                        if mode == settings.journey_setup.mode {
                            "● "
                        } else {
                            ""
                        },
                        mode.label()
                    ),
                    UiCommand::PlayMode(mode),
                );
            }
        });
        label(
            p,
            "Exploración mantiene física, señales y límites, sin objetivos de actividad. La vía inicial se elige con el recorrido PAT en Ruta → Opciones.",
            13.,
            MUTED,
        );
        if matches!(
            settings.journey_setup.mode,
            PlayMode::Tutorial | PlayMode::Challenge
        ) {
            for lesson in Lesson::ALL {
                if settings.journey_setup.mode == PlayMode::Challenge
                    && !matches!(lesson, Lesson::Rain | Lesson::Snow | Lesson::Fog)
                {
                    continue;
                }
                button(
                    p,
                    format!(
                        "{}{}",
                        if lesson == settings.journey_setup.lesson {
                            "● "
                        } else {
                            ""
                        },
                        lesson.label()
                    ),
                    UiCommand::Lesson(lesson),
                );
            }
            label(
                p,
                "El tutorial eléctrico requiere electric_supply y un tren eléctrico. El de motor requiere traction_operation. Los fallos preparados usan los frenos y la alimentación reales.",
                13.,
                MUTED,
            );
        }
        if matches!(
            settings.journey_setup.mode,
            PlayMode::Practice | PlayMode::Explore
        ) {
            row(p, |p| {
                button(p, "Estación anterior", UiCommand::PracticeStop(-1));
                label(
                    p,
                    settings
                        .journey_setup
                        .start_stop
                        .checked_sub(1)
                        .and_then(|i| menu.preview.stops.get(i))
                        .map_or("Origen de la ruta", String::as_str),
                    14.,
                    TEXT,
                );
                button(p, "Estación siguiente", UiCommand::PracticeStop(1));
            });
            button(
                p,
                format!(
                    "Inicio: {}",
                    if settings.journey_setup.approach {
                        "500 m antes de la estación"
                    } else {
                        "punto de parada"
                    }
                ),
                UiCommand::PracticeApproach,
            );
        }
        button(
            p,
            format!(
                "Tráfico del escenario: {}",
                yes(settings.journey_setup.traffic)
            ),
            UiCommand::PracticeTraffic,
        );
        label(
            p,
            "El modo elegido se aplica al pulsar Jugar. No modifica el escenario del autor.",
            13.,
            MUTED,
        );
    }
    button(
        p,
        format!("Asesor de frenado: {}", yes(settings.braking_coach)),
        UiCommand::Coach,
    );
    label(
        p,
        "El asesor estima con masa, pendiente y agarre actuales. Sólo aconseja; regulador y freno siguen siendo tuyos.",
        13.,
        MUTED,
    );
}
pub(super) fn build_report(
    p: &mut ChildSpawnerCommands<'_>,
    live: Option<&LiveDrive>,
    settings: &PlayerSettings,
) {
    let Some(l) = live else {
        label(
            p,
            "Iniciá o cargá una partida para ver sus datos.",
            14.,
            TEXT,
        );
        return;
    };
    let r = &l.session.state.driving_report;
    label(
        p,
        format!(
            "Frenadas {} · emergencias {} · intervenciones {}\nFuera del límite {:.1} s · freno asistido {:.1} s · freno manual {:.1} s\nConfort: aceleración RMS {:.3} m/s² · tirón máximo {:.3} m/s³\nEnergía neta {:.2} kWh · combustible consumido {:.2} L · masa actual {:.1} t",
            r.braking_events,
            r.emergency_events,
            r.protection_events,
            r.overspeed_s,
            r.assisted_s,
            r.braking_s,
            r.comfort_rms_mps2(),
            r.maximum_jerk_mps3,
            l.session.state.cumulative_energy_j / 3_600_000.,
            l.session.state.fuel_consumption_g / (1000. * 0.8508),
            l.session.effective_mass_kg() / 1000.
        ),
        14.,
        TEXT,
    );
    let speed: Vec<_> = r
        .samples
        .iter()
        .map(|s| (s.time_s, settings.display_speed_mps(s.speed_mps)))
        .collect();
    chart(
        p,
        &format!("Velocidad ({}) / tiempo", settings.speed_unit_label()),
        &speed,
        ACCENT,
    );
    chart(
        p,
        "Freno manual (%) / tiempo",
        &r.samples
            .iter()
            .map(|s| (s.time_s, s.requested_brake * 100.))
            .collect::<Vec<_>>(),
        CAUTION,
    );
    chart(
        p,
        "Energía neta (kWh) / tiempo",
        &r.samples
            .iter()
            .map(|s| (s.time_s, s.energy_j / 3_600_000.))
            .collect::<Vec<_>>(),
        Color::srgb(0.3, 0.8, 0.4),
    );
    label(p, "Paradas realizadas", 17., TEXT);
    for stop in &l.session.gameplay.stop_results {
        label(
            p,
            format!(
                "{} · demora {:+.0} s · error {:.1} m · embarque {:.0} s · salida anticipada {:.0} s",
                stop.name,
                stop.delay_s,
                stop.position_error_m,
                stop.dwell_s,
                stop.early_departure_s
            ),
            13.,
            TEXT,
        );
    }
    for stop in &l.session.gameplay.missed_stops {
        label(p, format!("Parada omitida: {}", stop.name), 13., CAUTION);
    }
    label(
        p,
        "Las gráficas conservan hasta 3600 muestras de un segundo; los totales abarcan la partida completa y se guardan al continuar.",
        12.,
        MUTED,
    );
}
pub(super) fn chart(
    p: &mut ChildSpawnerCommands<'_>,
    title: &str,
    values: &[(f64, f64)],
    color: Color,
) {
    label(p, title, 14., TEXT);
    let max = values.iter().map(|(_, v)| *v).fold(1., f64::max);
    let min = values.iter().map(|(_, v)| *v).fold(0., f64::min);
    let first = values.first().map_or(0., |v| v.0);
    let duration = values.last().map_or(1., |v| (v.0 - first).max(1.));
    label(
        p,
        format!(
            "{min:.1} a {max:.1} · {first:.0} a {:.0} s",
            first + duration
        ),
        12.,
        MUTED,
    );
    p.spawn((
        Node {
            height: Val::Px(120.),
            width: Val::Px(300.),
            max_width: Val::Percent(100.),
            position_type: PositionType::Relative,
            ..default()
        },
        BackgroundColor(Color::srgb(0.04, 0.07, 0.1)),
    ))
    .with_children(|p| {
        let point = |(t, v): (f64, f64)| {
            Vec2::new(
                ((t - first) / duration * 290.) as f32 + 5.,
                115. - ((v - min) / (max - min).max(0.001) * 110.) as f32,
            )
        };
        // Downsample only chart geometry; the stored samples and totals stay intact.
        let stride = values.len().div_ceil(300).max(1);
        let points: Vec<_> = values.iter().step_by(stride).copied().collect();
        for pair in points.windows(2) {
            line(p, point(pair[0]), point(pair[1]), 1.5, color);
        }
    });
}
#[derive(Component)]
pub(super) struct LessonHud;
#[derive(Component)]
pub(super) struct LessonHudRoot;
pub(super) fn spawn_hud(mut commands: Commands, cameras: Query<Entity, With<PlayerUiCamera>>) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(25.),
                right: Val::Percent(25.),
                bottom: Val::Px(45.),
                padding: UiRect::all(Val::Px(8.)),
                ..default()
            },
            BackgroundColor(BG),
            LessonHudRoot,
            Visibility::Hidden,
            UiTargetCamera(camera),
            ScreenHud,
            ZIndex(5),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont {
                    font_size: 14.0.into(),
                    ..default()
                },
                TextColor(TEXT),
                LessonHud,
            ));
        });
}
pub(super) fn update_hud(
    live: Option<Res<LiveDrive>>,
    ui: Res<PlayerUiState>,
    settings: Res<PlayerSettings>,
    mut text: Query<&mut Text, With<LessonHud>>,
    mut root: Query<&mut Visibility, With<LessonHudRoot>>,
    capture: Option<Res<crate::issue_report::PendingCapture>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let mut value = String::new();
    if ui.panel == PlayerPanel::None
        && let Some(l) = live
    {
        if let Some(lesson) = &l.session.state.learning {
            value = format!(
                "{} · {}/{}\n{}",
                lesson.lesson.label(),
                lesson.stage.min(lesson.lesson.steps().len()),
                lesson.lesson.steps().len(),
                lesson.instruction()
            );
        }
        if settings.braking_coach
            && let Some(a) = braking_advice(&l.session)
        {
            value.push_str(&format!("\nAsesor: {} · {:.0} m para frenar · objetivo {:.0} m · pendiente {:+.1}% · agarre {:.0}%",a.message,a.distance_m,a.target_distance_m,a.grade_percent,a.grip*100.));
        }
    }
    if capture.is_none() {
        for mut visibility in &mut root {
            let next = if value.is_empty() {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
            if *visibility != next {
                *visibility = next;
            }
        }
    }
    if text.0 != value {
        text.0 = value;
    }
}

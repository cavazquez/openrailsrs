use super::*;
use openrailsrs_scenarios::weather_track::{WeatherKind, WeatherPhase, WeatherTrack};
#[derive(Clone, Copy, Debug)]
pub(super) enum Field {
    Time,
    Transition,
    Kind,
    Intensity,
    WindX,
    WindZ,
    Seed,
    Temperature,
}
pub(super) fn edit(
    track: &mut WeatherTrack,
    index: usize,
    field: Field,
    delta: f64,
) -> Result<(), String> {
    let mut candidate = track.clone();
    if matches!(field, Field::Seed) {
        candidate.seed = candidate.seed.wrapping_add_signed(delta as i32);
    } else {
        let phase = candidate
            .phases
            .get_mut(index)
            .ok_or("La fase ya no existe")?;
        match field {
            Field::Time => phase.start_s += delta,
            Field::Transition => phase.transition_s += delta,
            Field::Kind => {
                phase.kind = WeatherKind::ALL[cycle(
                    WeatherKind::ALL
                        .iter()
                        .position(|k| *k == phase.kind)
                        .unwrap_or(0),
                    WeatherKind::ALL.len(),
                    delta as i32,
                )];
            }
            Field::Intensity => phase.intensity = (phase.intensity + delta as f32).clamp(0., 1.),
            Field::WindX => phase.wind_mps[0] += delta as f32,
            Field::WindZ => phase.wind_mps[1] += delta as f32,
            Field::Temperature => {
                phase.temperature_c = Some(
                    (phase
                        .temperature_c
                        .unwrap_or(if phase.kind == WeatherKind::Snow {
                            -5.
                        } else {
                            15.
                        })
                        + delta as f32)
                        .clamp(-60., 60.),
                )
            }
            Field::Seed => {}
        }
    }
    if !candidate.valid() {
        return Err("La primera fase debe comenzar en 0 s; las siguientes deben quedar ordenadas y su transición no puede superar el intervalo anterior".into());
    }
    *track = candidate;
    Ok(())
}
pub(super) fn add(track: &mut WeatherTrack) -> Result<(), String> {
    let last = track.phases.last().ok_or("Perfil vacío")?.start_s;
    let mut candidate = track.clone();
    candidate.phases.push(WeatherPhase {
        start_s: last + 120.,
        transition_s: 60.,
        kind: WeatherKind::Clear,
        intensity: 0.5,
        temperature_c: None,
        wind_mps: [1., 0.],
    });
    if !candidate.valid() {
        return Err("Se alcanzó el límite de fases o duración".into());
    }
    *track = candidate;
    Ok(())
}
pub(super) fn remove(track: &mut WeatherTrack, index: usize) -> Result<(), String> {
    if index == 0 || track.phases.len() == 1 {
        return Err("La fase inicial es obligatoria".into());
    }
    if index >= track.phases.len() {
        return Err("Fase inexistente".into());
    }
    let mut candidate = track.clone();
    candidate.phases.remove(index);
    if !candidate.valid() {
        return Err("Revisá la transición de la fase siguiente antes de eliminar ésta".into());
    }
    *track = candidate;
    Ok(())
}
pub(super) fn profile_file(
    track: &mut WeatherTrack,
    import: bool,
) -> Result<std::path::PathBuf, String> {
    let path = player_data_dir().join("weather-profiles/custom.json");
    if import {
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
            return Err("Perfil demasiado grande".into());
        }
        let candidate: WeatherTrack =
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if !candidate.valid() {
            return Err("Perfil incompatible o inválido".into());
        }
        *track = candidate;
    } else {
        crate::player_settings::atomic_write(
            &path,
            &serde_json::to_vec_pretty(track).map_err(|e| e.to_string())?,
        )?;
    }
    Ok(path)
}
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    ui: &PlayerUiState,
    settings: &PlayerSettings,
) {
    let track = &settings.weather_track;
    if !track.valid() {
        label(
            p,
            "Perfil inválido. Cargá un perfil válido o restaurá los ajustes.",
            14.,
            CAUTION,
        );
        return;
    }
    let index = ui.weather_phase.min(track.phases.len() - 1);
    let phase = &track.phases[index];
    label(
        p,
        "Las fases usan segundos desde el inicio de la partida. El reloj se detiene al pausar. La transición comienza antes de la hora indicada y llega a ella sin saltos.",
        13.,
        MUTED,
    );
    row(p, |p| {
        button(p, "Fase anterior", UiCommand::WeatherPhase(-1));
        label(
            p,
            format!("Fase {}/{}", index + 1, track.phases.len()),
            17.,
            TEXT,
        );
        button(p, "Fase siguiente", UiCommand::WeatherPhase(1));
        button(p, "Agregar fase", UiCommand::WeatherAdd);
        button(p, "Eliminar fase", UiCommand::WeatherRemove);
    });
    for (name, value, field, step) in [
        (
            "Inicio",
            format!("{:.0} s", phase.start_s),
            Field::Time,
            30.,
        ),
        (
            "Transición",
            format!("{:.0} s", phase.transition_s),
            Field::Transition,
            10.,
        ),
        ("Clima", phase.kind.label().into(), Field::Kind, 1.),
        (
            "Intensidad",
            format!("{:.0}%", phase.intensity * 100.),
            Field::Intensity,
            0.1,
        ),
        (
            "Viento este",
            format!("{:.1} m/s", phase.wind_mps[0]),
            Field::WindX,
            1.,
        ),
        (
            "Viento norte",
            format!("{:.1} m/s", phase.wind_mps[1]),
            Field::WindZ,
            1.,
        ),
        (
            "Temperatura",
            phase
                .temperature_c
                .map_or("Tipo original".into(), |t| format!("{t:.0} °C")),
            Field::Temperature,
            1.,
        ),
        ("Semilla", track.seed.to_string(), Field::Seed, 1.),
    ] {
        row(p, |p| {
            label(p, format!("{name}: {value}"), 14., TEXT);
            button(p, "−", UiCommand::WeatherEdit(field, -step));
            button(p, "+", UiCommand::WeatherEdit(field, step));
        });
    }
    row(p, |p| {
        button(p, "Vista −60 s", UiCommand::WeatherPreview(-60.));
        button(p, "Vista +60 s", UiCommand::WeatherPreview(60.));
    });
    let frame = track.at(ui.weather_preview_s);
    label(
        p,
        format!(
            "Vista {:.0} s · lluvia {:.0}% · nieve {:.0}% · visibilidad {:.0} m · agarre {:.0}%",
            ui.weather_preview_s,
            frame.rain * 100.,
            frame.snow * 100.,
            frame.visibility_m,
            frame.rail_factor() * 100.
        ),
        14.,
        ACCENT,
    );
    let end = track.phases.last().unwrap().start_s.max(600.);
    let points: Vec<_> = (0..=120)
        .map(|i| {
            let t = end * i as f64 / 120.;
            (t, track.at(t).rain as f64 * 100.)
        })
        .collect();
    super::study::chart(p, "Previsión de lluvia (%) / segundos", &points, ACCENT);
    row(p, |p| {
        button(p, "Usar este perfil", UiCommand::WeatherActivate(true));
        button(
            p,
            "Volver al clima elegido",
            UiCommand::WeatherActivate(false),
        );
        button(p, "Guardar perfil", UiCommand::WeatherProfileFile(false));
        button(
            p,
            "Importar custom.json",
            UiCommand::WeatherProfileFile(true),
        );
    });
    label(
        p,
        format!(
            "Perfil activo: {}\nArchivo editable: {}",
            yes(settings.weather_profile == WeatherProfile::Edited),
            player_data_dir()
                .join("weather-profiles/custom.json")
                .display()
        ),
        12.,
        MUTED,
    );
}

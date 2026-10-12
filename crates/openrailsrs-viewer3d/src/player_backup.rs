//! Versioned, bounded player backups. Only known preferences and player saves
//! are exported; importing creates new history entries and never replaces slots.
use crate::{player_settings::PlayerSettings, saved_game::SavedGame};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentReference {
    pub name: String,
    pub download_sha256: String,
    pub old_root: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Backup {
    schema: u32,
    created_utc: i64,
    settings: Option<PlayerSettings>,
    saves: Vec<SavedGame>,
    content: Vec<ContentReference>,
    skipped: Vec<String>,
}
pub struct Imported {
    pub settings: Option<PlayerSettings>,
    pub files: Vec<PathBuf>,
    pub missing: Vec<String>,
}
fn bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > max {
        return Err("Archivo de respaldo demasiado grande".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}
fn inside(path: &Path, root: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    path.canonicalize().ok().filter(|p| p.starts_with(root))
}
fn references(data: &Path) -> Vec<ContentReference> {
    let root = data.join("official-content");
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten().take(512) {
            let Some(path) = inside(&entry.path(), &root) else {
                continue;
            };
            let Some(manifest) = inside(&path.join("openrailsrs-content.json"), &path) else {
                continue;
            };
            let Ok(bytes) = bounded(&manifest, 8 * 1024 * 1024) else {
                continue;
            };
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let Some(sha) = value["download_sha256"]
                .as_str()
                .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            else {
                continue;
            };
            out.push(ContentReference {
                name: value["package"]
                    .as_str()
                    .unwrap_or("Contenido")
                    .chars()
                    .take(256)
                    .collect(),
                download_sha256: sha.into(),
                old_root: path,
            });
        }
    }
    out
}
pub fn export(data: &Path) -> Result<PathBuf, String> {
    let settings = inside(&data.join("settings.json"), data)
        .map(|p| {
            bounded(&p, 4 * 1024 * 1024).and_then(|b| {
                serde_json::from_slice::<PlayerSettings>(&b).map_err(|e| e.to_string())
            })
        })
        .transpose()?;
    if let Some(settings) = &settings {
        settings.validate()?;
    }
    let mut paths = (1..=3)
        .map(|i| data.join(format!("save-{i}.json")))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    if let Ok(history) = std::fs::read_dir(data.join("history")) {
        let mut entries = history
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect::<Vec<_>>();
        entries.sort();
        entries.reverse();
        paths.extend(entries.into_iter().take(61));
    }
    let mut saves = vec![];
    let mut skipped = vec![];
    for path in paths {
        let Some(path) = inside(&path, data) else {
            continue;
        };
        match SavedGame::read(&path) {
            Ok(mut game) => {
                if game.session.script_state.is_some()
                    || game
                        .traffic
                        .iter()
                        .any(|s| s.session.script_state.is_some())
                {
                    skipped.push(format!(
                        "{}: estado de script externo; no se exporta automáticamente",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    continue;
                }
                let original: toml::Value = toml::from_str(&game.scenario_toml)
                    .map_err(|e| format!("Escenario inválido: {e}"))?;
                let scenario: openrailsrs_scenarios::ScenarioFile = original
                    .clone()
                    .try_into()
                    .map_err(|e| format!("Escenario inválido: {e}"))?;
                let mut clean = toml::Value::try_from(&scenario).map_err(|e| e.to_string())?;
                if let Some(setup) = original.get("player_setup") {
                    let setup: crate::journey_setup::JourneySetup =
                        setup.clone().try_into().map_err(|e| e.to_string())?;
                    if !setup.valid() {
                        return Err("Modo de partida inválido".into());
                    }
                    clean.as_table_mut().ok_or("Escenario inválido")?.insert(
                        "player_setup".into(),
                        toml::Value::try_from(setup).map_err(|e| e.to_string())?,
                    );
                }
                game.scenario_toml = toml::to_string(&clean).map_err(|e| e.to_string())?;
                saves.push(game);
            }
            Err(error) => skipped.push(format!(
                "{}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    if settings.is_none() && saves.is_empty() {
        return Err("No hay ajustes ni partidas válidas para respaldar".into());
    }
    let backup = Backup {
        schema: 1,
        created_utc: chrono::Utc::now().timestamp(),
        settings,
        saves,
        content: references(data),
        skipped,
    };
    let bytes = serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("El respaldo supera 64 MiB; archivá algunas partidas por separado".into());
    }
    let dir = data.join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("respaldo-{}.json", nonce()));
    write_new(&path, &bytes)?;
    Ok(path)
}
fn nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = std::fs::remove_file(path);
        return Err(e.to_string());
    }
    Ok(())
}
pub fn import(data: &Path, path: &Path) -> Result<Imported, String> {
    let backup: Backup = serde_json::from_slice(&bounded(path, MAX_BYTES)?)
        .map_err(|e| format!("Respaldo inválido: {e}"))?;
    if backup.schema != 1
        || backup.saves.len() > 64
        || backup.content.len() > 512
        || backup.skipped.len() > 256
    {
        return Err("Versión o tamaño de respaldo incompatible".into());
    }
    if let Some(settings) = &backup.settings {
        settings.validate()?;
    }
    // Validate all game structures before writing the first restored file.
    for game in &backup.saves {
        if game.session.script_state.is_some()
            || game
                .traffic
                .iter()
                .any(|s| s.session.script_state.is_some())
        {
            return Err("El respaldo contiene estado de código externo".into());
        }
        SavedGame::from_bytes(&serde_json::to_vec(game).map_err(|e| e.to_string())?)?;
    }
    let installed = references(data);
    let mut mapping = vec![];
    let mut missing = vec![];
    for reference in &backup.content {
        if !reference.old_root.is_absolute()
            || reference.old_root.components().count() < 3
            || reference
                .old_root
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            || reference.name.len() > 256
            || reference.download_sha256.len() != 64
            || !reference
                .download_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err("Referencia de contenido inválida".into());
        }
        if let Some(current) = installed
            .iter()
            .find(|p| p.download_sha256 == reference.download_sha256 && p.name == reference.name)
        {
            mapping.push((reference.old_root.clone(), current.old_root.clone()));
        } else {
            missing.push(reference.name.clone());
        }
    }
    let history = data.join("history");
    std::fs::create_dir_all(&history).map_err(|e| e.to_string())?;
    let id = nonce();
    let mut files = vec![];
    let result = (|| {
        for (i, mut game) in backup.saves.into_iter().enumerate() {
            rebase(&mut game, &mapping)?;
            let path = history.join(format!("import-{id}-{i}.json"));
            write_new(
                &path,
                &serde_json::to_vec_pretty(&game).map_err(|e| e.to_string())?,
            )?;
            files.push(path);
        }
        if let Some(settings) = &backup.settings {
            let path = data.join(format!("settings-import-{id}.json"));
            write_new(
                &path,
                &serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
            )?;
            files.push(path);
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        for path in &files {
            let _ = std::fs::remove_file(path);
        }
        return Err(error);
    }
    Ok(Imported {
        settings: backup.settings,
        files: files
            .into_iter()
            .filter(|p| p.parent() == Some(history.as_path()))
            .collect(),
        missing,
    })
}
fn rebase(game: &mut SavedGame, mapping: &[(PathBuf, PathBuf)]) -> Result<(), String> {
    let replace = |path: &Path| {
        mapping
            .iter()
            .find_map(|(old, new)| path.strip_prefix(old).ok().map(|suffix| new.join(suffix)))
            .unwrap_or_else(|| path.into())
    };
    if let Some(root) = &mut game.route_root {
        *root = replace(root);
    }
    let mut value: toml::Value =
        toml::from_str(&game.scenario_toml).map_err(|e| format!("Escenario inválido: {e}"))?;
    for (section, field) in [("route", "path"), ("train", "consist")] {
        if let Some(path) = value.get_mut(section).and_then(|s| s.get_mut(field))
            && let Some(source) = path.as_str()
        {
            *path = toml::Value::String(replace(Path::new(source)).to_string_lossy().into_owned());
        }
    }
    if let Some(trains) = value
        .get_mut("extra_trains")
        .and_then(toml::Value::as_array_mut)
    {
        for train in trains {
            if let Some(path) = train.get_mut("consist")
                && let Some(source) = path.as_str()
            {
                *path =
                    toml::Value::String(replace(Path::new(source)).to_string_lossy().into_owned());
            }
        }
    }
    game.scenario_toml = toml::to_string(&value).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn list(data: &Path) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(data.join("backups"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .take(128)
        .collect::<Vec<_>>();
    paths.sort();
    paths.reverse();
    paths
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backup_whitelists_settings_and_import_preserves_original_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let settings = PlayerSettings {
            braking_coach: true,
            ..Default::default()
        };
        let mut value = serde_json::to_value(&settings).unwrap();
        value["secret_token"] = serde_json::json!("do-not-export");
        std::fs::write(
            root.join("settings.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        std::fs::write(root.join("credentials.json"), b"do-not-export").unwrap();
        let before = std::fs::read(root.join("settings.json")).unwrap();
        let backup = export(root).unwrap();
        assert!(
            !String::from_utf8(std::fs::read(&backup).unwrap())
                .unwrap()
                .contains("do-not-export")
        );
        let restored = import(root, &backup).unwrap();
        assert!(restored.settings.unwrap().braking_coach);
        assert_eq!(std::fs::read(root.join("settings.json")).unwrap(), before);
    }
    #[test]
    fn shared_backup_rejects_malformed_weather_before_writing_any_save() {
        use crate::{
            camera::{CameraFollowMode, DriverLookOffset, OrbitState},
            floating_origin::FloatingOrigin,
            live::LiveDrive,
            player_launch::ActivePlayerContent,
            saved_game::SavedCamera,
        };
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let mut live = LiveDrive::from_scenario_path(&path).unwrap();
        let camera = SavedCamera::capture(
            CameraFollowMode::ChaseCam,
            &OrbitState::default(),
            &DriverLookOffset::default(),
            &FloatingOrigin::default(),
        );
        SavedGame::save(
            &mut live,
            &ActivePlayerContent::default(),
            camera,
            &root.join("save-1.json"),
            None,
            Some(&PlayerSettings::default()),
        )
        .unwrap();
        let file = export(root).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        value["saves"][0]["player_options"]["weather_track"]["phases"] = serde_json::json!([]);
        std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(import(root, &file).is_err());
        assert!(!root.join("history").exists());
        value["saves"][0]["player_options"]["weather_track"] =
            serde_json::to_value(openrailsrs_scenarios::weather_track::WeatherTrack::default())
                .unwrap();
        value["saves"][0]["player_options"]["journey_setup"]["start_stop"] =
            serde_json::json!(5000);
        std::fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(import(root, &file).is_err());
        assert!(!root.join("history").exists());
    }
    #[test]
    fn valid_save_backup_creates_a_new_resumable_history_copy_and_strips_unknown_fields() {
        use crate::{
            camera::{CameraFollowMode, DriverLookOffset, OrbitState},
            floating_origin::FloatingOrigin,
            live::LiveDrive,
            player_launch::ActivePlayerContent,
            saved_game::SavedCamera,
        };
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/scenario.toml");
        let mut live = LiveDrive::from_scenario_path(&path).unwrap();
        live.session.step_realtime(1., |_| {});
        let camera = SavedCamera::capture(
            CameraFollowMode::ChaseCam,
            &OrbitState::default(),
            &DriverLookOffset::default(),
            &FloatingOrigin::default(),
        );
        let slot = root.join("save-1.json");
        SavedGame::save(
            &mut live,
            &ActivePlayerContent::default(),
            camera,
            &slot,
            None,
            Some(&PlayerSettings::default()),
        )
        .unwrap();
        let mut game = SavedGame::read(&slot).unwrap();
        game.scenario_toml
            .push_str("\n[private]\nsecret_token='not-a-game-field'\n");
        std::fs::write(&slot, serde_json::to_vec(&game).unwrap()).unwrap();
        let original = std::fs::read(&slot).unwrap();
        let file = export(root).unwrap();
        assert!(
            !String::from_utf8(std::fs::read(&file).unwrap())
                .unwrap()
                .contains("not-a-game-field")
        );
        let imported = import(root, &file).unwrap();
        assert_eq!(imported.files.len(), 1);
        assert_ne!(imported.files[0], slot);
        assert_eq!(std::fs::read(&slot).unwrap(), original);
        let restored = SavedGame::read(&imported.files[0]).unwrap();
        let scenario: openrailsrs_scenarios::ScenarioFile =
            toml::from_str(&restored.scenario_toml).unwrap();
        let mut resumed = LiveDrive::from_scenario(root, &scenario).unwrap();
        restored.clone().restore(&mut resumed).unwrap();
        assert_eq!(resumed.session.state.time, game.session.state.time);
        assert_eq!(restored.session.state.time, game.session.state.time);
    }
}

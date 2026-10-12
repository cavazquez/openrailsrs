//! Portable saves contain logical references and verified author-file identities,
//! never a fallback search through the recipient's filesystem.
use crate::{
    player_backup::{Backup, Imported},
    saved_game::SavedGame,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContentIdentity {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub token: String,
    pub files: u32,
}
#[derive(Serialize, Deserialize)]
struct Portable {
    schema: u32,
    application_version: String,
    open_rails_reference: String,
    created_utc: i64,
    content: Vec<ContentIdentity>,
    saves: Vec<SavedGame>,
}
fn bounded(path: &Path) -> Result<Vec<u8>, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > MAX_BYTES {
        return Err("Partida portátil supera 64 MiB".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}
fn content_extension(p: &Path) -> bool {
    p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "toml"
                | "trk"
                | "tdb"
                | "rdb"
                | "tsection"
                | "dat"
                | "eng"
                | "wag"
                | "con"
                | "s"
                | "sd"
                | "ace"
                | "dds"
                | "png"
                | "bmp"
                | "t"
                | "raw"
                | "w"
                | "ws"
                | "pat"
                | "act"
                | "srv"
                | "trf"
                | "sms"
                | "wav"
                | "cvf"
                | "sigcfg"
                | "sigscr"
                | "sig"
                | "cs"
                | "txt"
        )
    })
}
/// Hash names and bytes of actual railway resources, including generated track
/// geometry, while excluding caches, logs and application manifests.
pub fn identify(root: &Path) -> Result<ContentIdentity, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    let mut todo = vec![root.clone()];
    let mut visited = 0;
    let mut total = 0u64;
    while let Some(dir) = todo.pop() {
        visited += 1;
        if visited > 100_000 {
            return Err("Instalación excede el límite de directorios".into());
        }
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            openrailsrs_formats::cancellation::check().map_err(|e| e.to_string())?;
            let e = entry.map_err(|e| e.to_string())?;
            let p = e.path();
            let kind = e.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                return Err(format!(
                    "Recurso enlazado no transportable: {}",
                    p.strip_prefix(&root).unwrap_or(&p).display()
                ));
            }
            if kind.is_dir() {
                if !matches!(
                    e.file_name().to_str(),
                    Some(".git" | "target" | "cache" | "texture-cache" | "logs")
                ) {
                    todo.push(p);
                }
                continue;
            }
            if !kind.is_file() || !content_extension(&p) {
                continue;
            }
            total = total.saturating_add(e.metadata().map_err(|e| e.to_string())?.len());
            if files.len() >= 200_000 || total > 32 * 1024 * 1024 * 1024 {
                return Err(
                    "Instalación excede el presupuesto de identidad (200000 archivos, 32 GiB)"
                        .into(),
                );
            }
            let name = p
                .strip_prefix(&root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if name.len() > 4096 {
                return Err("Nombre de recurso demasiado largo".into());
            }
            files.insert(
                name,
                openrailsrs_train::consist_audit::content_file_sha256(&p)
                    .map_err(|e| e.to_string())?,
            );
        }
    }
    if files.is_empty() {
        return Err("No se encontraron recursos ferroviarios en la instalación".into());
    }
    let mut h = Sha256::new();
    for (name, sha) in &files {
        h.update((name.len() as u64).to_le_bytes());
        h.update(name.as_bytes());
        h.update(sha.as_bytes());
    }
    let manifest = root.join("openrailsrs-content.json");
    let meta = if manifest.is_file()
        && std::fs::metadata(&manifest).is_ok_and(|m| m.len() <= 1024 * 1024)
    {
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(&manifest).map_err(|e| e.to_string())?,
        )
        .ok()
    } else {
        None
    };
    let id = meta
        .as_ref()
        .and_then(|m| m["catalog_source"].as_str())
        .unwrap_or("local-content")
        .to_string();
    let version = meta
        .as_ref()
        .and_then(|m| {
            m["revision"]["commit"]
                .as_str()
                .or_else(|| m["download_sha256"].as_str())
        })
        .unwrap_or("files-v1")
        .to_string();
    if id.len() > 512
        || version.len() > 256
        || id.chars().chain(version.chars()).any(char::is_control)
    {
        return Err("Identidad del contenido inválida".into());
    }
    Ok(ContentIdentity {
        id,
        version,
        sha256: format!("{:x}", h.finalize()),
        token: String::new(),
        files: files.len() as u32,
    })
}
fn game_paths(game: &SavedGame) -> Result<Vec<PathBuf>, String> {
    let scenario: openrailsrs_scenarios::ScenarioFile =
        toml::from_str(&game.scenario_toml).map_err(|e| e.to_string())?;
    let mut paths = vec![
        PathBuf::from(scenario.route.path),
        PathBuf::from(scenario.train.consist),
    ];
    paths.extend(
        scenario
            .extra_trains
            .into_iter()
            .map(|s| PathBuf::from(s.consist)),
    );
    if !scenario.train.announcements.events.is_empty() {
        paths.push(PathBuf::from(scenario.train.announcements.root));
    }
    paths.extend(game.route_root.clone());
    if let Some(options) = &game.player_options {
        paths.extend(options.shared_installations.clone());
    }
    Ok(paths)
}
fn managed_roots(data: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(data.join("official-content"))
        .into_iter()
        .flatten()
        .flatten()
        .take(512)
        .filter_map(|e| e.path().canonicalize().ok())
        .filter(|p| p.join("openrailsrs-content.json").is_file())
        .collect()
}
pub fn available_roots(data: &Path, explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut result = managed_roots(data);
    for p in explicit {
        if let Ok(p) = p.canonicalize()
            && p.is_dir()
            && !result.contains(&p)
        {
            result.push(p);
        }
    }
    result
}
pub fn export(data: &Path, explicit: &[PathBuf]) -> Result<PathBuf, String> {
    let temporary = crate::player_backup::export(data)?;
    let result = (|| {
        let backup: Backup =
            serde_json::from_slice(&bounded(&temporary)?).map_err(|e| e.to_string())?;
        if backup.saves.is_empty() {
            return Err("Guardá primero una partida para transportarla".into());
        }
        let roots = available_roots(data, explicit);
        let mut used = vec![];
        for game in &backup.saves {
            for p in game_paths(game)? {
                let p = p
                    .canonicalize()
                    .map_err(|_| "Falta un recurso de la partida; completalo antes de exportar")?;
                let root=roots.iter().filter(|r|p.starts_with(r)).max_by_key(|r|r.components().count()).ok_or("El recurso no está en una instalación registrada. Agregá su instalación original o elegí un ejemplo incluido.")?;
                if !used.contains(root) {
                    used.push(root.clone());
                }
            }
        }
        if used.len() > 32 {
            return Err("La partida depende de más de 32 instalaciones".into());
        }
        let mut identities = vec![];
        let mut mapping = vec![];
        for (i, root) in used.iter().enumerate() {
            let mut id = identify(root)?;
            id.token = format!("/__openrailsrs_content__/package-{i}");
            mapping.push((root.clone(), PathBuf::from(&id.token)));
            identities.push(id);
        }
        let mut saves = backup.saves;
        for game in &mut saves {
            crate::player_backup::rebase(game, &mapping)?;
            let mut scenario: toml::Value =
                toml::from_str(&game.scenario_toml).map_err(|e| e.to_string())?;
            // Output paths belong to the recipient, never to the author package.
            if let Some(output) = scenario.get_mut("output") {
                output["csv"] = toml::Value::String("portable-run.csv".into());
                output["metadata"] = toml::Value::String("portable-run.json".into());
            }
            scenario
                .as_table_mut()
                .ok_or("Escenario inválido")?
                .remove("validate");
            if let Some(services) = scenario
                .get_mut("extra_trains")
                .and_then(toml::Value::as_array_mut)
            {
                for (i, service) in services.iter_mut().enumerate() {
                    service["output_csv"] =
                        toml::Value::String(format!("portable-traffic-{i}.csv"));
                }
            }
            game.scenario_toml = toml::to_string_pretty(&scenario).map_err(|e| e.to_string())?;
            for p in game_paths(game)? {
                if !identities.iter().any(|id| p.starts_with(&id.token)) {
                    return Err("Referencia de contenido no transportable".into());
                }
            }
        }
        let portable = Portable {
            schema: 2,
            application_version: env!("CARGO_PKG_VERSION").into(),
            open_rails_reference: "1.6.1".into(),
            created_utc: chrono::Utc::now().timestamp(),
            content: identities,
            saves,
        };
        let bytes = serde_json::to_vec_pretty(&portable).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("Partida portátil supera 64 MiB".into());
        }
        let file = data.join("backups").join(format!(
            "portable-{}.json",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        crate::player_settings::atomic_write(&file, &bytes)?;
        Ok(file)
    })();
    let _ = std::fs::remove_file(temporary);
    result
}
pub fn import(data: &Path, path: &Path, explicit: &[PathBuf]) -> Result<Imported, String> {
    let portable: Portable = serde_json::from_slice(&bounded(path)?)
        .map_err(|e| format!("Partida portátil inválida: {e}"))?;
    if portable.schema != 2
        || portable.application_version != env!("CARGO_PKG_VERSION")
        || portable.open_rails_reference != "1.6.1"
        || portable.saves.is_empty()
        || portable.saves.len() > 64
        || portable.content.is_empty()
        || portable.content.len() > 32
    {
        return Err("Versión o tamaño de partida portátil incompatible".into());
    }
    let roots = available_roots(data, explicit);
    let mut identified = vec![];
    let mut mapping = vec![];
    for (index, id) in portable.content.iter().enumerate() {
        if id.token != format!("/__openrailsrs_content__/package-{index}")
            || id.sha256.len() != 64
            || !id.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || id.id.len() > 512
            || id.version.len() > 256
            || id.files > 200_000
        {
            return Err("Identidad de recurso portátil inválida".into());
        }
        let mut found = None;
        for root in &roots {
            let fingerprint = if let Some((_, f)) = identified.iter().find(|(p, _)| p == root) {
                f
            } else {
                let f = identify(root)?;
                identified.push((root.clone(), f));
                &identified.last().unwrap().1
            };
            if fingerprint.id == id.id
                && fingerprint.version == id.version
                && fingerprint.sha256 == id.sha256
                && fingerprint.files == id.files
            {
                found = Some(root.clone());
                break;
            }
        }
        let root=found.ok_or_else(||format!("Falta contenido idéntico o sus archivos cambiaron: {} · {} · SHA256 {}. Instalá la misma versión original; no se reemplaza por otra copia.",id.id,id.version,id.sha256))?;
        mapping.push((PathBuf::from(&id.token), root));
    }
    let mut saves = portable.saves;
    for game in &mut saves {
        for p in game_paths(game)? {
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
                || !mapping.iter().any(|(old, _)| p.starts_with(old))
            {
                return Err("Referencia fuera del contenido portátil".into());
            }
        }
        crate::player_backup::rebase(game, &mapping)?;
        // Check every canonical path, including symlink containment, before writing.
        for p in game_paths(game)? {
            let canonical = p
                .canonicalize()
                .map_err(|_| "Recurso portátil ausente en la instalación")?;
            if !mapping.iter().any(|(_, root)| canonical.starts_with(root)) {
                return Err("Recurso portátil sale de la instalación".into());
            }
        }
        let scenario: openrailsrs_scenarios::ScenarioFile =
            toml::from_str(&game.scenario_toml).map_err(|e| e.to_string())?;
        if let Some(options) = &game.player_options {
            openrailsrs_formats::resource_links::configure(&options.shared_installations)?;
        }
        let live = crate::live::LiveDrive::from_scenario(Path::new("."), &scenario)?;
        live.session.validate_snapshot(&game.session)?;
        live.traffic.validate_snapshot(&game.traffic)?;
    }
    crate::player_backup::import_backup(
        data,
        Backup {
            schema: 1,
            created_utc: portable.created_utc,
            settings: None,
            saves,
            content: vec![],
            skipped: vec![],
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
    #[test]
    fn portable_save_has_no_personal_paths_and_restores_identical_content_in_another_folder() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("alice-install");
        let b = temp.path().join("bob-different-name");
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        copy(&fixture, &a);
        copy(&fixture, &b);
        let data1 = temp.path().join("data-a");
        let data2 = temp.path().join("data-b");
        std::fs::create_dir_all(&data1).unwrap();
        std::fs::create_dir_all(&data2).unwrap();
        let mut live =
            crate::live::LiveDrive::from_scenario_path(&a.join("scenario.toml")).unwrap();
        live.session.driver_throttle = 0.5;
        live.session.driver_direction = 1.;
        live.session.step_realtime(2., |_| {});
        let camera = crate::saved_game::SavedCamera::capture(
            crate::camera::CameraFollowMode::ChaseCam,
            &crate::camera::OrbitState::default(),
            &crate::camera::DriverLookOffset::default(),
            &crate::floating_origin::FloatingOrigin::default(),
        );
        SavedGame::save(
            &mut live,
            &crate::player_launch::ActivePlayerContent::default(),
            camera,
            &data1.join("save-1.json"),
            None,
            None,
        )
        .unwrap();
        let slot = data1.join("save-1.json");
        let mut source_save = SavedGame::read(&slot).unwrap();
        let mut scenario_value: toml::Value = toml::from_str(&source_save.scenario_toml).unwrap();
        scenario_value.as_table_mut().unwrap().insert(
            "validate".into(),
            toml::Value::Table(toml::map::Map::from_iter([(
                "baseline_or".into(),
                toml::Value::String("/home/alice/private/oracle/dump.csv".into()),
            )])),
        );
        source_save.scenario_toml = toml::to_string(&scenario_value).unwrap();
        std::fs::write(&slot, serde_json::to_vec(&source_save).unwrap()).unwrap();
        let path = export(&data1, std::slice::from_ref(&a)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&a.to_string_lossy().to_string()));
        assert!(!text.contains("alice-install"));
        assert!(!text.contains("/home/alice"));
        assert!(text.contains("/__openrailsrs_content__/package-0"));
        let result = import(&data2, &path, std::slice::from_ref(&b)).unwrap();
        assert_eq!(result.files.len(), 1);
        let game = SavedGame::read(&result.files[0]).unwrap();
        let scenario: openrailsrs_scenarios::ScenarioFile =
            toml::from_str(&game.scenario_toml).unwrap();
        let mut resumed = crate::live::LiveDrive::from_scenario(Path::new("."), &scenario).unwrap();
        game.clone().restore(&mut resumed).unwrap();
        assert_eq!(resumed.session.state.time.0, live.session.state.time.0);
        assert_eq!(
            resumed.session.state.odometer_m,
            live.session.state.odometer_m
        );
        std::fs::write(b.join("vehicles/minimal.eng"), "changed original").unwrap();
        assert!(import(&data2, &path, std::slice::from_ref(&b)).is_err());
        assert_eq!(std::fs::read_dir(data2.join("history")).unwrap().count(), 1);
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["saves"][0]["scenario_toml"] = serde_json::json!(game.scenario_toml);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(import(&data2, &path, std::slice::from_ref(&b)).is_err());
    }
}

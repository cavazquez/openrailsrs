//! Campaign progress and material continuity live in the player's data directory.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone)]
pub struct CampaignEntry {
    pub path: PathBuf,
    pub definition: openrailsrs_campaign::CampaignFile,
    pub id: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MaterialLedger {
    pub sources: Vec<String>,
    pub state: openrailsrs_sim::TrainSimState,
    pub cars: Vec<openrailsrs_sim::operations::CarOperationState>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct CampaignProgress {
    pub schema: u32,
    pub results: openrailsrs_campaign::Progress,
    pub material: Option<MaterialLedger>,
    pub last_mission: Option<String>,
}
#[derive(Resource, Default)]
pub struct CampaignPlay {
    pub entries: Vec<CampaignEntry>,
    pub selected: usize,
    pub progress: CampaignProgress,
    pub status: String,
    pending: Option<(usize, usize)>,
    active: Option<(usize, usize)>,
    completed: bool,
    seen_path: Option<PathBuf>,
    data: Option<PathBuf>,
}
fn identity(path: &Path) -> Result<String, String> {
    openrailsrs_train::consist_audit::content_file_sha256(path).map_err(|e| e.to_string())
}
fn material_sources(live: &crate::live::LiveDrive) -> Result<Vec<String>, String> {
    let scenario =
        openrailsrs_scenarios::load_scenario(live.scenario_path()).map_err(|e| e.to_string())?;
    let con = live
        .scenario_path()
        .parent()
        .unwrap_or(Path::new("."))
        .join(scenario.train.consist);
    let ast = openrailsrs_formats::parse_vehicle_text(
        &openrailsrs_formats::read_msts_file_to_string(&con).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let entries = openrailsrs_formats::ConsistFile::from_ast(&ast).map_err(|e| e.to_string())?;
    entries
        .entries
        .iter()
        .map(|e| {
            openrailsrs_train::consist_audit::source_fingerprint(
                &openrailsrs_train::resolve_consist_entry_path(
                    openrailsrs_train::consist_asset_root(&con),
                    e.path(),
                ),
            )
            .map_err(|e| e.to_string())
        })
        .collect()
}
impl CampaignPlay {
    fn data_dir(&self) -> PathBuf {
        self.data
            .clone()
            .unwrap_or_else(crate::player_settings::player_data_dir)
    }
    pub fn discover(&mut self, project: &Path) {
        self.entries.clear();
        let mut todo = vec![
            project.join("examples"),
            self.data_dir().join("campaign-definitions"),
        ];
        let mut visited = 0;
        while let Some(dir) = todo.pop() {
            visited += 1;
            if visited > 4096 {
                break;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                let p = entry.path();
                if kind.is_dir() {
                    todo.push(p);
                } else if p.file_name().is_some_and(|n| n == "campaign.toml")
                    && std::fs::metadata(&p).is_ok_and(|m| m.len() <= 1024 * 1024)
                    && let Ok(definition) = openrailsrs_campaign::load_campaign(&p)
                    && definition.missions.len() <= 256
                    && let Ok(id) = identity(&p)
                {
                    self.entries.push(CampaignEntry {
                        path: p,
                        definition,
                        id,
                    });
                }
            }
        }
        self.entries
            .sort_by(|a, b| a.definition.campaign.name.cmp(&b.definition.campaign.name));
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        let _ = self.select(self.selected);
    }
    pub fn select(&mut self, index: usize) -> Result<(), String> {
        let entry = self.entries.get(index).ok_or("Campaña no disponible")?;
        self.selected = index;
        let file = self
            .data_dir()
            .join("campaigns")
            .join(&entry.id)
            .join("progress.json");
        self.progress = if file.is_file() {
            if std::fs::metadata(&file).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
                return Err("Progreso demasiado grande".into());
            }
            serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        } else {
            // Read earlier CLI progress without writing into the campaign source.
            CampaignProgress {
                schema: 1,
                results: {
                    let p = entry.path.parent().unwrap().join("progress.json");
                    if p.is_file()
                        && std::fs::metadata(&p).is_ok_and(|m| m.len() <= 4 * 1024 * 1024)
                    {
                        openrailsrs_campaign::load_progress(&p).map_err(|e| e.to_string())?
                    } else {
                        default()
                    }
                },
                ..default()
            }
        };
        if self.progress.schema != 1 || self.progress.results.completed.len() > 256 {
            return Err("Progreso de campaña incompatible".into());
        }
        self.status = "Resultados y reservas se conservan entre jornadas.".into();
        Ok(())
    }
    pub fn start(
        &mut self,
        mission: usize,
        menu: &crate::player_launch::PlayerLaunchMenu,
    ) -> Result<crate::player_launch::QueuedPlayerLaunch, String> {
        let entry = self.entries.get(self.selected).ok_or("Elegí una campaña")?;
        let statuses =
            openrailsrs_campaign::mission_statuses(&entry.definition, &self.progress.results);
        let status = statuses.get(mission).ok_or("Jornada inexistente")?;
        if status.state == openrailsrs_campaign::MissionState::Locked {
            return Err("Completá primero las jornadas requeridas".into());
        }
        let mut allowed = vec![
            menu.project_path().join("examples"),
            entry.path.parent().unwrap().to_path_buf(),
        ];
        allowed.extend(menu.original_installations().into_iter().map(|(p, _)| p));
        let source = resolve_scenario(&entry.path, &status.def.scenario, &allowed)?;
        let mut scenario = read_scenario(&source)?;
        let base = source.parent().unwrap();
        for p in [&mut scenario.route.path, &mut scenario.train.consist] {
            *p = resolve_author_resource(base, p, &allowed)?
                .to_string_lossy()
                .into_owned();
        }
        for t in &mut scenario.extra_trains {
            t.consist = resolve_author_resource(base, &t.consist, &allowed)?
                .to_string_lossy()
                .into_owned();
        }
        let mut auditor = openrailsrs_train::ConsistAuditor::new(
            openrailsrs_formats::resource_links::configured()
                .into_iter()
                .map(|r| r.join("TRAINS/TRAINSET"))
                .collect(),
        );
        let audit = auditor.inspect(Path::new(&scenario.train.consist));
        if !audit.player_ready() {
            return Err(audit.label());
        }
        openrailsrs_sim::LiveDriveSession::from_scenario(Path::new("."), &scenario)
            .map_err(|e| e.to_string())?;
        scenario.scenario.description = format!(
            "Jornada {}: {}. {}",
            mission + 1,
            status.def.name,
            status.def.description
        );
        let path = self
            .data_dir()
            .join("campaigns")
            .join(&entry.id)
            .join("launch.toml");
        if !scenario.train.announcements.events.is_empty() {
            let package = &mut scenario.train.announcements;
            package.root = base
                .join(&package.root)
                .canonicalize()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
        }
        let mut value = toml::Value::try_from(&scenario).map_err(|e| e.to_string())?;
        value.as_table_mut().unwrap().insert(
            "player_campaign".into(),
            toml::Value::try_from(CampaignContext {
                campaign_id: entry.id.clone(),
                mission_id: status.def.id.clone(),
            })
            .map_err(|e| e.to_string())?,
        );
        crate::player_settings::atomic_write(
            &path,
            toml::to_string_pretty(&value)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        )?;
        self.seen_path = None;
        self.pending = Some((self.selected, mission));
        self.completed = false;
        Ok(crate::player_launch::QueuedPlayerLaunch {
            path,
            route_root: menu
                .services
                .iter()
                .find(|s| s.source == source)
                .and_then(|s| s.scenery_root.clone()),
            weather: menu.weather,
            environment: menu.environment,
            resume: None,
            weather_checkpoint: None,
            player_options: None,
        })
    }
    pub fn activate(&mut self, live: &mut crate::live::LiveDrive) -> Result<(), String> {
        if self.seen_path.as_deref() == Some(live.scenario_path()) {
            return Ok(());
        }
        self.seen_path = Some(live.scenario_path().to_owned());
        self.completed = false;
        let pending = self.pending.take();
        let Some(active) = pending.or_else(|| {
            read_context(live.scenario_path()).and_then(|c| {
                let i = self.entries.iter().position(|e| e.id == c.campaign_id)?;
                let m = self.entries[i]
                    .definition
                    .missions
                    .iter()
                    .position(|m| m.id == c.mission_id)?;
                Some((i, m))
            })
        }) else {
            self.active = None;
            return Ok(());
        };
        if self.selected != active.0 {
            self.select(active.0)?;
        }
        self.active = Some(active);
        // A restored save already contains this jornada's complete physical state.
        if pending.is_none() {
            return Ok(());
        }
        let sources = material_sources(live)?;
        let Some(ledger) = self.progress.material.as_ref() else {
            return Ok(());
        };
        if ledger.sources != sources {
            self.status="La jornada usa otro material: se conserva el registro anterior y se inicia el equipo del autor.".into();
            return Ok(());
        }
        let mut snapshot = live.session.snapshot();
        snapshot.state.diesel = ledger.state.diesel.clone();
        snapshot.state.diesel_rpm = ledger.state.diesel_rpm.clone();
        snapshot.state.boiler_state = ledger.state.boiler_state.clone();
        snapshot.state.cab_equipment = ledger.state.cab_equipment.clone();
        snapshot.state.brake_system = ledger.state.brake_system.clone();
        snapshot.state.rail_adhesion = ledger.state.rail_adhesion.clone();
        for (next, old) in snapshot.formation.cars.iter_mut().zip(&ledger.cars) {
            next.battery_on = old.battery_on;
            next.power_on = old.power_on;
            next.train_supply_switch_on = old.train_supply_switch_on;
            next.handbrake = old.handbrake;
            next.front_cock_open = old.front_cock_open;
            next.rear_cock_open = old.rear_cock_open;
            next.hose_connected = old.hose_connected;
        }
        for (next, old) in snapshot
            .state
            .couplers
            .iter_mut()
            .zip(&ledger.state.couplers)
        {
            next.broken = old.broken;
        }
        if let (Some(next), Some(old)) = (
            &mut snapshot.state.native_dynamics,
            &ledger.state.native_dynamics,
        ) {
            next.bearing_c = old.bearing_c.clone();
        }
        live.session.validate_snapshot(&snapshot)?;
        live.session.restore_snapshot(snapshot)?;
        self.status="Material de la jornada anterior: reservas y estado conservados; nueva posición y horario.".into();
        Ok(())
    }
    pub fn finish(&mut self, live: &crate::live::LiveDrive) -> Result<(), String> {
        let Some((campaign, mission)) = self.active else {
            return Ok(());
        };
        if self.completed || !live.session.gameplay.is_finished() {
            return Ok(());
        }
        let entry = &self.entries[campaign];
        let def = &entry.definition.missions[mission];
        let score = if live.session.gameplay.phase == openrailsrs_sim::ServicePhase::Completed {
            (100. - live.session.gameplay.accrued_penalty.min(100.)).round() as u32
        } else {
            0
        };
        openrailsrs_campaign::record_result(
            &mut self.progress.results,
            &def.id,
            score,
            def.bonus_threshold,
        );
        if let Some(result) = self.progress.results.completed.get_mut(&def.id) {
            result.timestamp = chrono::Utc::now().to_rfc3339();
        }
        self.progress.schema = 1;
        self.progress.last_mission = Some(def.id.clone());
        self.progress.material = Some(MaterialLedger {
            sources: material_sources(live)?,
            state: live.session.state.clone(),
            cars: live.session.formation.cars.clone(),
        });
        let file = self
            .data_dir()
            .join("campaigns")
            .join(&entry.id)
            .join("progress.json");
        crate::player_settings::atomic_write(
            &file,
            &serde_json::to_vec_pretty(&self.progress).map_err(|e| e.to_string())?,
        )?;
        self.completed = true;
        self.status = format!(
            "Jornada {} registrada: {score}/100. Material conservado para continuar.",
            def.name
        );
        Ok(())
    }
}
pub fn enter(
    mut campaigns: ResMut<CampaignPlay>,
    menu: Res<crate::player_launch::PlayerLaunchMenu>,
) {
    campaigns.discover(menu.project_path());
}
pub fn update(
    mut campaigns: ResMut<CampaignPlay>,
    mut live: Option<ResMut<crate::live::LiveDrive>>,
) {
    if let Some(l) = live.as_mut() {
        let result = campaigns.activate(l).and_then(|()| campaigns.finish(l));
        if let Err(e) = result {
            campaigns.status = e;
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CampaignContext {
    pub campaign_id: String,
    pub mission_id: String,
}
pub fn read_context(path: &Path) -> Option<CampaignContext> {
    if std::fs::metadata(path).ok()?.len() > 4 * 1024 * 1024 {
        return None;
    }
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let context: CampaignContext = value.get("player_campaign")?.clone().try_into().ok()?;
    valid_context(&context).then_some(context)
}
pub fn valid_context(c: &CampaignContext) -> bool {
    c.campaign_id.len() == 64
        && c.campaign_id.bytes().all(|b| b.is_ascii_hexdigit())
        && !c.mission_id.is_empty()
        && c.mission_id.len() <= 128
        && !c.mission_id.chars().any(char::is_control)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn campaign_progress_and_real_material_continue_without_cli_and_resume_keeps_context() {
        let temp = tempfile::tempdir().unwrap();
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let menu = crate::player_launch::PlayerLaunchMenu::discover(&project, None);
        let mut campaigns = CampaignPlay {
            data: Some(temp.path().to_path_buf()),
            ..default()
        };
        campaigns.discover(&project);
        let index = campaigns
            .entries
            .iter()
            .position(|e| e.definition.campaign.name == "Dos jornadas · material propio")
            .unwrap();
        campaigns.select(index).unwrap();
        assert!(campaigns.start(1, &menu).is_err());
        let request = campaigns.start(0, &menu).unwrap();
        let mut first = crate::live::LiveDrive::from_scenario_path(&request.path).unwrap();
        campaigns.activate(&mut first).unwrap();
        first.session.state.diesel.cars[0].fuel_l -= 7.;
        first.session.state.diesel.cars[0].consumed_l += 7.;
        first.session.formation.cars[0].handbrake = true;
        first.session.gameplay.phase = openrailsrs_sim::ServicePhase::Completed;
        first.session.arrived = true;
        let fuel = first.session.state.diesel.cars[0].fuel_l;
        campaigns.finish(&first).unwrap();
        assert!(campaigns.progress.results.completed.contains_key("day1"));
        let mut reload = CampaignPlay {
            data: Some(temp.path().to_path_buf()),
            ..default()
        };
        reload.discover(&project);
        reload.select(index).unwrap();
        let request = reload.start(1, &menu).unwrap();
        let mut second = crate::live::LiveDrive::from_scenario_path(&request.path).unwrap();
        reload.activate(&mut second).unwrap();
        assert_eq!(second.session.state.diesel.cars[0].fuel_l, fuel);
        assert!(second.session.formation.cars[0].handbrake);
        assert_eq!(second.session.time_s(), 0.);
        assert_eq!(second.start_clock_s, 32400.);
        let context = read_context(&request.path).unwrap();
        assert_eq!(context.mission_id, "day2");
        assert_eq!(context.campaign_id, reload.entries[index].id);
        assert!(
            std::fs::read(
                temp.path()
                    .join("campaigns")
                    .join(&context.campaign_id)
                    .join("progress.json")
            )
            .is_ok()
        );
    }
}

fn resolve_author_resource(
    base: &Path,
    reference: &str,
    allowed: &[PathBuf],
) -> Result<PathBuf, String> {
    let path = base
        .join(reference)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !allowed
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| path.starts_with(r))
    {
        return Err(
            "La referencia sale del paquete y de las instalaciones originales seleccionadas".into(),
        );
    }
    Ok(path)
}
fn resolve_scenario(
    campaign: &Path,
    reference: &str,
    allowed: &[PathBuf],
) -> Result<PathBuf, String> {
    let path = resolve_author_resource(
        campaign.parent().ok_or("Campaña sin carpeta")?,
        reference,
        allowed,
    )?;
    if path.extension().is_none_or(|e| e != "toml") {
        return Err("La jornada debe ser un escenario TOML regular".into());
    }
    Ok(path)
}
fn read_scenario(path: &Path) -> Result<openrailsrs_scenarios::ScenarioFile, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err("La jornada debe ser un archivo regular de hasta 4 MiB".into());
    }
    let mut bytes = vec![];
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("El escenario excede el límite de lectura".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let scenario: openrailsrs_scenarios::ScenarioFile =
        toml::from_str(text).map_err(|e| e.to_string())?;
    scenario.validate().map_err(|e| e.to_string())?;
    Ok(scenario)
}
#[cfg(test)]
mod confinement_tests {
    use super::*;
    #[test]
    fn campaign_rejects_external_devices_and_unbounded_sources() {
        let temp = tempfile::tempdir().unwrap();
        let campaign = temp.path().join("campaign.toml");
        std::fs::write(&campaign, "source").unwrap();
        assert!(resolve_scenario(&campaign, "/dev/zero", &[temp.path().to_path_buf()]).is_err());
        #[cfg(unix)]
        {
            assert!(read_scenario(Path::new("/dev/zero")).is_err());
            let link = temp.path().join("escape.toml");
            std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
            assert!(
                resolve_scenario(&campaign, "escape.toml", &[temp.path().to_path_buf()]).is_err()
            );
        }
        let large = temp.path().join("large.toml");
        std::fs::File::create(&large)
            .unwrap()
            .set_len(4 * 1024 * 1024 + 1)
            .unwrap();
        assert!(read_scenario(&large).is_err());
    }
}

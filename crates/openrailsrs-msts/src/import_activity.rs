//! Convert an MSTS Activity (`.act`) + Path (`.pat`) into an `openrailsrs` `scenario.toml`.

use std::path::Path;

use openrailsrs_formats::{ActivityFile, PathFile};
use openrailsrs_scenarios::model::{
    GameplaySection, ObjectiveKind, OutputSection, RouteSection, ScenarioFile, ScenarioMeta,
    SimulationSection, TrainSection,
};

use crate::error::MstsError;

/// Parse an MSTS `.act` file (and the `.pat` it references) and produce a
/// `scenario.toml` TOML string compatible with `openrailsrs-scenarios`.
///
/// `route_dir` is used to resolve the `.pat` path found inside the `.act`.
pub fn import_activity(route_dir: &Path, act_path: &Path) -> Result<String, MstsError> {
    let (toml, _) = import_activity_with_summary(route_dir, act_path)?;
    Ok(toml)
}

/// Same as `import_activity` but also returns the activity name.
pub fn import_activity_with_summary(
    route_dir: &Path,
    act_path: &Path,
) -> Result<(String, String), MstsError> {
    let activity = ActivityFile::from_path(act_path)?;
    let pat_path = resolve_asset_path(route_dir, &activity.player_path);
    let path_file = PathFile::from_path(&pat_path)?;

    let start_node = path_file
        .start_node()
        .map(|n| format!("n{n}"))
        .unwrap_or_else(|| "start".to_string());

    let destination_node = path_file
        .end_node()
        .map(|n| format!("n{n}"))
        .unwrap_or_else(|| "end".to_string());

    // Use the consist path as-is; the user can adjust it after import.
    let consist = sanitize_path(&activity.player_consist);

    // Duration: use the activity's duration, fallback to 2 hours.
    let duration_s = if activity.duration_s > 0.0 {
        activity.duration_s
    } else {
        7200.0
    };

    let scenario = ScenarioFile {
        scenario: ScenarioMeta {
            name: activity.name.clone(),
            description: format!("Imported from MSTS activity: {}", act_path.display()),
        },
        route: RouteSection {
            path: "track.toml".to_string(),
            start: start_node,
            destination: destination_node,
            stops: Vec::new(),
            switches: Vec::new(),
        },
        train: TrainSection {
            consist,
            davis: None,
            max_capacity: None,
        },
        gameplay: GameplaySection {
            objective: ObjectiveKind::Arrive,
            time_limit_seconds: None,
            difficulty: openrailsrs_scenarios::model::Difficulty::Normal,
            penalty_per_second_late: 0.0,
        },
        simulation: SimulationSection {
            duration: duration_s,
            time_step: 1.0,
            seed: 42,
        },
        output: OutputSection {
            csv: "run.csv".to_string(),
            metadata: "run.json".to_string(),
        },
        extra_trains: Vec::new(),
    };

    let toml = toml::to_string_pretty(&scenario)?;
    Ok((toml, activity.name))
}

/// Resolve an asset path that may use Windows backslashes and may be relative
/// to `route_dir`.
fn resolve_asset_path(base: &Path, asset: &str) -> std::path::PathBuf {
    let normalized = asset.trim().replace('\\', "/");
    base.join(&normalized)
}

/// Strip leading path separators and replace backslashes to make the consist
/// path suitable as a TOML string.
fn sanitize_path(s: &str) -> String {
    s.trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

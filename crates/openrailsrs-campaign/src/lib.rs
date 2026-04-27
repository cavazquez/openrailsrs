pub mod error;
pub mod model;

pub use error::CampaignError;
pub use model::{
    CampaignFile, CampaignMeta, Difficulty, MissionDef, MissionResult, MissionState, MissionStatus,
    Progress,
};

use std::path::Path;

// ── Load / save ───────────────────────────────────────────────────────────────

/// Load a `campaign.toml` from disk.
pub fn load_campaign(path: &Path) -> Result<CampaignFile, CampaignError> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

/// Load `progress.json`; returns an empty `Progress` if the file does not exist.
pub fn load_progress(path: &Path) -> Result<Progress, CampaignError> {
    if !path.exists() {
        return Ok(Progress::default());
    }
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

/// Persist `Progress` to `progress.json` (pretty-printed).
pub fn save_progress(path: &Path, progress: &Progress) -> Result<(), CampaignError> {
    let json = serde_json::to_string_pretty(progress)?;
    std::fs::write(path, json)?;
    Ok(())
}

// ── Unlock / status logic ─────────────────────────────────────────────────────

/// Compute the display status of every mission given the current progress.
pub fn mission_statuses<'c>(
    campaign: &'c CampaignFile,
    progress: &Progress,
) -> Vec<MissionStatus<'c>> {
    campaign
        .missions
        .iter()
        .map(|def| {
            let result = progress.completed.get(&def.id);
            let best_score = result.map(|r| r.score);
            let bonus = result.is_some_and(|r| r.bonus);

            let state = if prerequisites_met(def, progress) {
                if best_score.is_some_and(|s| s >= def.min_pass_score) {
                    MissionState::Completed
                } else {
                    MissionState::Available
                }
            } else {
                MissionState::Locked
            };

            MissionStatus {
                def,
                state,
                best_score,
                bonus,
            }
        })
        .collect()
}

fn prerequisites_met(def: &MissionDef, progress: &Progress) -> bool {
    def.requires.iter().all(|req_id| {
        progress
            .completed
            .get(req_id)
            .is_some_and(|r| r.score >= def.min_pass_score)
    })
}

/// Record a mission result. Updates only if the new score is better.
pub fn record_result(progress: &mut Progress, mission_id: &str, score: u32, bonus_threshold: u32) {
    let timestamp = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format_timestamp(secs)
    };

    let bonus = score >= bonus_threshold;

    let entry = progress
        .completed
        .entry(mission_id.to_string())
        .or_insert(MissionResult {
            score: 0,
            timestamp: timestamp.clone(),
            bonus: false,
        });

    if score > entry.score {
        entry.score = score;
        entry.timestamp = timestamp;
        entry.bonus = bonus;
    }
}

fn format_timestamp(unix_secs: u64) -> String {
    // Minimal RFC-3339 formatter without external crates.
    let s = unix_secs;
    let secs = s % 60;
    let mins = (s / 60) % 60;
    let hours = (s / 3600) % 24;
    let days_since_epoch = s / 86400;

    // Very simple Gregorian approximation — good enough for display.
    let year = 1970 + days_since_epoch / 365;
    let day_of_year = days_since_epoch % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, mins, secs
    )
}

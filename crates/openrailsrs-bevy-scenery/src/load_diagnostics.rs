//! Structured MSTS/OR load diagnostics (#54).
//!
//! Single source for CLI, HUD and audits: every request ends as
//! `loaded`, `failed` or `fallback` (`requested = loaded + failed + fallback`).

use std::path::Path;

use bevy::prelude::Resource;

/// Asset class involved in a load attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MstsAssetKind {
    World,
    Shape,
    Ace,
    Terrain,
    /// TrackObj mesh / procedural / failed accounting (#35).
    TrackObj,
}

impl MstsAssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Shape => "shape",
            Self::Ace => "ace",
            Self::Terrain => "terrain",
            Self::TrackObj => "trackobj",
        }
    }
}

/// Why a load did not produce a normal success.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MstsLoadCause {
    Missing,
    Parse,
    Unsupported,
    /// Substitute used (procedural TrackObj, magenta/fallback material, …).
    Fallback,
}

impl MstsLoadCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Parse => "parse",
            Self::Unsupported => "unsupported",
            Self::Fallback => "fallback",
        }
    }
}

/// One failed or fallback sample (path + cause preserved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    pub path: String,
    pub kind: MstsAssetKind,
    pub cause: MstsLoadCause,
    pub detail: String,
    pub tile_x: Option<i32>,
    pub tile_z: Option<i32>,
}

/// Aggregated load pipeline diagnostics (Bevy [`Resource`] + plain struct).
#[derive(Clone, Debug, Resource)]
pub struct MstsLoadDiagnostics {
    pub requested: u64,
    pub loaded: u64,
    pub failed: u64,
    pub fallback: u64,
    pub failures: Vec<LoadFailure>,
    max_samples: usize,
}

impl Default for MstsLoadDiagnostics {
    fn default() -> Self {
        Self {
            requested: 0,
            loaded: 0,
            failed: 0,
            fallback: 0,
            failures: Vec::new(),
            max_samples: 64,
        }
    }
}

impl MstsLoadDiagnostics {
    pub fn with_max_samples(max_samples: usize) -> Self {
        Self {
            max_samples,
            ..Default::default()
        }
    }

    /// `requested == loaded + failed + fallback`.
    pub fn totals_ok(&self) -> bool {
        self.requested
            == self
                .loaded
                .saturating_add(self.failed)
                .saturating_add(self.fallback)
    }

    pub fn record_loaded(&mut self, _kind: MstsAssetKind) {
        self.requested += 1;
        self.loaded += 1;
    }

    pub fn record_failed(
        &mut self,
        path: impl Into<String>,
        kind: MstsAssetKind,
        cause: MstsLoadCause,
        detail: impl Into<String>,
    ) {
        self.record_failed_at(path, kind, cause, detail, None, None);
    }

    pub fn record_failed_at(
        &mut self,
        path: impl Into<String>,
        kind: MstsAssetKind,
        cause: MstsLoadCause,
        detail: impl Into<String>,
        tile_x: Option<i32>,
        tile_z: Option<i32>,
    ) {
        self.requested += 1;
        self.failed += 1;
        self.push_sample(LoadFailure {
            path: path.into(),
            kind,
            cause,
            detail: detail.into(),
            tile_x,
            tile_z,
        });
    }

    pub fn record_fallback(
        &mut self,
        path: impl Into<String>,
        kind: MstsAssetKind,
        detail: impl Into<String>,
    ) {
        self.record_fallback_at(path, kind, detail, None, None);
    }

    pub fn record_fallback_at(
        &mut self,
        path: impl Into<String>,
        kind: MstsAssetKind,
        detail: impl Into<String>,
        tile_x: Option<i32>,
        tile_z: Option<i32>,
    ) {
        self.requested += 1;
        self.fallback += 1;
        self.push_sample(LoadFailure {
            path: path.into(),
            kind,
            cause: MstsLoadCause::Fallback,
            detail: detail.into(),
            tile_x,
            tile_z,
        });
    }

    pub fn record_path_loaded(&mut self, path: &Path, kind: MstsAssetKind) {
        let _ = path;
        self.record_loaded(kind);
    }

    pub fn record_path_failed(
        &mut self,
        path: &Path,
        kind: MstsAssetKind,
        cause: MstsLoadCause,
        detail: impl Into<String>,
    ) {
        self.record_failed(path.display().to_string(), kind, cause, detail);
    }

    /// Merge another bag (e.g. world preload + spawn). Samples capped.
    pub fn merge_from(&mut self, other: &MstsLoadDiagnostics) {
        self.requested += other.requested;
        self.loaded += other.loaded;
        self.failed += other.failed;
        self.fallback += other.fallback;
        for f in &other.failures {
            self.push_sample(f.clone());
        }
    }

    /// Ingest #35 TrackObj exclusive outcomes (`seen = mesh + procedural + failed`).
    pub fn ingest_trackobj_outcomes(
        &mut self,
        mesh: u64,
        procedural: u64,
        failed: u64,
        failure_samples: impl IntoIterator<Item = LoadFailure>,
    ) {
        self.requested += mesh + procedural + failed;
        self.loaded += mesh;
        self.fallback += procedural;
        self.failed += failed;
        for f in failure_samples {
            self.push_sample(f);
        }
    }

    /// Feed texture resolve counters (render3d [`TextureLoadStats`]-compatible).
    pub fn ingest_texture_stats(
        &mut self,
        resolved: u32,
        unresolved: u32,
        decode_failed: u32,
        unresolved_samples: &[(String, String)],
        decode_failed_samples: &[(String, String)],
    ) {
        for _ in 0..resolved {
            self.record_loaded(MstsAssetKind::Ace);
        }
        let mut unresolved_left = unresolved as usize;
        for (shape, detail) in unresolved_samples {
            if unresolved_left == 0 {
                break;
            }
            unresolved_left -= 1;
            self.record_failed(
                format!("{shape} → {detail}"),
                MstsAssetKind::Ace,
                MstsLoadCause::Missing,
                "texture file not found",
            );
        }
        for i in 0..unresolved_left {
            self.record_failed(
                format!("ace:unresolved#{i}"),
                MstsAssetKind::Ace,
                MstsLoadCause::Missing,
                "texture file not found (sample omitted)",
            );
        }
        let mut decode_left = decode_failed as usize;
        for (shape, detail) in decode_failed_samples {
            if decode_left == 0 {
                break;
            }
            decode_left -= 1;
            // Decode failure uses scenery fallback material in render3d.
            self.record_fallback(
                format!("{shape} → {detail}"),
                MstsAssetKind::Ace,
                "ace decode failed; fallback material",
            );
        }
        for i in 0..decode_left {
            self.record_fallback(
                format!("ace:decode#{i}"),
                MstsAssetKind::Ace,
                "ace decode failed; fallback material (sample omitted)",
            );
        }
    }

    pub fn summary_line(&self) -> String {
        format!(
            "load: {} req = {} ok + {} fail + {} fallback{}",
            self.requested,
            self.loaded,
            self.failed,
            self.fallback,
            if self.totals_ok() {
                String::new()
            } else {
                " (WARNING totals mismatch)".to_string()
            }
        )
    }

    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = vec![self.summary_line()];
        for f in self.failures.iter().take(12) {
            let tile = match (f.tile_x, f.tile_z) {
                (Some(x), Some(z)) => format!(" tile={x},{z}"),
                _ => String::new(),
            };
            lines.push(format!(
                "  · {} {} [{}]{} — {}",
                f.kind.as_str(),
                f.cause.as_str(),
                f.path,
                tile,
                f.detail
            ));
        }
        if self.failures.len() > 12 {
            lines.push(format!(
                "  … {} more sample(s) omitted",
                self.failures.len() - 12
            ));
        }
        lines
    }

    pub fn report(&self) {
        if self.requested == 0 {
            return;
        }
        for line in self.summary_lines() {
            println!("{line}");
        }
    }

    /// Compact JSON for `OPENRAILSRS_LOAD_AUDIT` / audits (no serde dep).
    pub fn to_json(&self) -> String {
        let mut failures = String::new();
        for (i, f) in self.failures.iter().enumerate() {
            if i > 0 {
                failures.push(',');
            }
            let tile_x = f
                .tile_x
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());
            let tile_z = f
                .tile_z
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());
            failures.push_str(&format!(
                "{{\"path\":{},\"kind\":\"{}\",\"cause\":\"{}\",\"detail\":{},\"tile_x\":{},\"tile_z\":{}}}",
                json_string(&f.path),
                f.kind.as_str(),
                f.cause.as_str(),
                json_string(&f.detail),
                tile_x,
                tile_z,
            ));
        }
        format!(
            "{{\"requested\":{},\"loaded\":{},\"failed\":{},\"fallback\":{},\"totals_ok\":{},\"failures\":[{failures}]}}",
            self.requested,
            self.loaded,
            self.failed,
            self.fallback,
            self.totals_ok(),
        )
    }

    pub fn maybe_write_audit_env(&self) {
        let Some(val) = std::env::var_os("OPENRAILSRS_LOAD_AUDIT") else {
            return;
        };
        let val = val.to_string_lossy();
        if val == "0" || val.eq_ignore_ascii_case("false") || val.eq_ignore_ascii_case("no") {
            return;
        }
        let json = self.to_json();
        if val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("yes") {
            println!("OPENRAILSRS_LOAD_AUDIT={json}");
            return;
        }
        let path = std::path::PathBuf::from(val.as_ref());
        if let Err(e) = std::fs::write(&path, json.as_bytes()) {
            eprintln!(
                "openrailsrs: failed to write LOAD_AUDIT to {}: {e}",
                path.display()
            );
        } else {
            println!("openrailsrs: wrote load audit → {}", path.display());
        }
    }

    fn push_sample(&mut self, failure: LoadFailure) {
        if self.failures.len() < self.max_samples {
            self.failures.push(failure);
        }
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_close_loaded_failed_fallback() {
        let mut d = MstsLoadDiagnostics::default();
        d.record_loaded(MstsAssetKind::Shape);
        d.record_failed(
            "missing.s",
            MstsAssetKind::Shape,
            MstsLoadCause::Missing,
            "gone",
        );
        d.record_fallback("bad.ace", MstsAssetKind::Ace, "decode → fallback");
        assert_eq!(d.requested, 3);
        assert_eq!(d.loaded, 1);
        assert_eq!(d.failed, 1);
        assert_eq!(d.fallback, 1);
        assert!(d.totals_ok());
        assert_eq!(d.failures.len(), 2);
        assert_eq!(d.failures[0].cause, MstsLoadCause::Missing);
        assert_eq!(d.failures[1].cause, MstsLoadCause::Fallback);
    }

    #[test]
    fn trackobj_ingest_matches_seen() {
        let mut d = MstsLoadDiagnostics::default();
        d.ingest_trackobj_outcomes(
            10,
            3,
            2,
            [LoadFailure {
                path: "foo.s".into(),
                kind: MstsAssetKind::TrackObj,
                cause: MstsLoadCause::Missing,
                detail: "shape_unresolved_and_no_procedural".into(),
                tile_x: Some(-6082),
                tile_z: Some(14925),
            }],
        );
        assert_eq!(d.requested, 15);
        assert_eq!(d.loaded, 10);
        assert_eq!(d.fallback, 3);
        assert_eq!(d.failed, 2);
        assert!(d.totals_ok());
        assert!(d.to_json().contains("\"totals_ok\":true"));
    }

    #[test]
    fn texture_stats_ingest_closes() {
        let mut d = MstsLoadDiagnostics::default();
        d.ingest_texture_stats(
            5,
            2,
            1,
            &[("a.s".into(), "missing.ace".into())],
            &[("b.s".into(), "bad.ace".into())],
        );
        assert_eq!(d.requested, 8);
        assert_eq!(d.loaded, 5);
        assert_eq!(d.failed, 2);
        assert_eq!(d.fallback, 1);
        assert!(d.totals_ok());
    }

    #[test]
    fn corrupt_fixture_records_parse_cause() {
        let mut d = MstsLoadDiagnostics::default();
        d.record_path_failed(
            Path::new("/tmp/broken.w"),
            MstsAssetKind::World,
            MstsLoadCause::Parse,
            "unexpected token",
        );
        assert!(!d.failures.is_empty());
        assert_eq!(d.failures[0].cause, MstsLoadCause::Parse);
        assert!(d.summary_line().contains("1 fail"));
    }
}

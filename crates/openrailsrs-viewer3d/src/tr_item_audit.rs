//! TDB `TrItem` audit (TSRE `checkDatabase` subset).

use std::path::Path;

use bevy::prelude::*;
use openrailsrs_formats::{TSectionCatalog, TrItemKind, TrackDbFile};
use serde::Serialize;

use crate::shapes::RouteAssets;
use crate::tr_item_index::TrItemWorldIndex;
use crate::track_position::tr_item_msts_world;

pub const TR_ITEM_WORLD_MATCH_RADIUS_M: f32 = 25.0;

#[derive(Clone, Debug, Serialize)]
pub struct TrItemAuditReport {
    pub route_dir: String,
    pub items: Vec<TrItemAuditSample>,
    pub summary: TrItemAuditSummary,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct TrItemAuditSummary {
    pub total_items: usize,
    pub signal_items: usize,
    pub single_host_ok: usize,
    pub pose_ok: usize,
    pub world_linked: usize,
    pub world_delta_ok: usize,
    pub errors: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrItemAuditSample {
    pub tr_item_id: u32,
    pub kind: String,
    pub host_count: usize,
    pub host_vector_id: Option<u32>,
    pub distance_m: f64,
    pub pose_msts: Option<[f32; 3]>,
    pub world_refs: usize,
    pub delta_world_xz_m: Option<f32>,
    pub status: String,
}

pub fn run_tr_item_audit(
    route_dir: &Path,
    tdb: &TrackDbFile,
    tsection: Option<&TSectionCatalog>,
    world_index: Option<&TrItemWorldIndex>,
    tile_filter: Option<(i32, i32)>,
) -> TrItemAuditReport {
    let hosts = tdb.index_item_hosts();
    let mut items = Vec::new();
    let mut summary = TrItemAuditSummary {
        total_items: tdb.items.len(),
        ..Default::default()
    };

    for item in &tdb.items {
        let host_list = hosts.get(&item.id).cloned().unwrap_or_default();
        let host_count = host_list.len();
        let host_vector_id = if host_count == 1 {
            summary.single_host_ok += 1;
            Some(host_list[0])
        } else {
            None
        };

        let kind = match &item.kind {
            TrItemKind::Signal { .. } => {
                summary.signal_items += 1;
                "signal"
            }
            TrItemKind::SpeedPost { .. } => "speedpost",
            TrItemKind::SoundSource { .. } => "sound_source",
            TrItemKind::Other => "other",
        }
        .to_string();

        let pose_msts = tr_item_msts_world(tdb, item.id, tsection).map(|p| [p.x, p.y, p.z]);
        if pose_msts.is_some() {
            summary.pose_ok += 1;
        }

        let world_refs = world_index
            .map(|idx| idx.objects_for_item(item.id).len())
            .unwrap_or(0);
        if world_refs > 0 {
            summary.world_linked += 1;
        }

        let delta_world_xz_m = match (pose_msts, world_index) {
            (Some([x, _, z]), Some(idx)) => {
                idx.objects_for_item(item.id).iter().fold(None, |best, r| {
                    let d = Vec2::new(r.position_msts.x - x, r.position_msts.z - z).length();
                    Some(best.map(|b: f32| b.min(d)).unwrap_or(d))
                })
            }
            _ => None,
        };
        if delta_world_xz_m.is_some_and(|d| d <= TR_ITEM_WORLD_MATCH_RADIUS_M) {
            summary.world_delta_ok += 1;
        }

        let status = if host_count != 1 {
            summary.errors += 1;
            format!("host_count={host_count}")
        } else if pose_msts.is_none() {
            summary.errors += 1;
            "no_tdb_pose".into()
        } else if matches!(item.kind, TrItemKind::Signal { .. })
            && world_index.is_some()
            && world_refs == 0
        {
            summary.errors += 1;
            "signal_no_world_ref".into()
        } else if delta_world_xz_m.is_some_and(|d| d > TR_ITEM_WORLD_MATCH_RADIUS_M) {
            summary.errors += 1;
            format!("world_delta_{:.1}m", delta_world_xz_m.unwrap_or(0.0))
        } else {
            "ok".into()
        };

        if tile_filter.is_none()
            || pose_msts.is_some_and(|p| tile_matches(p, tile_filter.unwrap()))
            || matches!(item.kind, TrItemKind::Signal { .. })
        {
            items.push(TrItemAuditSample {
                tr_item_id: item.id,
                kind,
                host_count,
                host_vector_id,
                distance_m: item.distance_m,
                pose_msts,
                world_refs,
                delta_world_xz_m,
                status,
            });
        }
    }

    TrItemAuditReport {
        route_dir: route_dir.display().to_string(),
        items,
        summary,
    }
}

fn tile_matches(pos: [f32; 3], tile: (i32, i32)) -> bool {
    use openrailsrs_formats::{msts_tile_x_index_for_coord, msts_tile_z_index_for_coord};
    msts_tile_x_index_for_coord(pos[0]) == tile.0 && msts_tile_z_index_for_coord(pos[2]) == tile.1
}

pub fn log_tr_item_audit(report: &TrItemAuditReport) {
    crate::viewer_log!(
        "openrailsrs-viewer3d: tr_item audit — {} item(s), {} signal(s), {} pose ok, {} world linked, {} errors",
        report.summary.total_items,
        report.summary.signal_items,
        report.summary.pose_ok,
        report.summary.world_linked,
        report.summary.errors
    );
    for sample in report.items.iter().filter(|s| s.status != "ok").take(8) {
        crate::viewer_log!(
            "  tr_item {} ({}) — {}",
            sample.tr_item_id,
            sample.kind,
            sample.status
        );
    }
}

pub fn run_tr_item_audit_for_route(
    route_dir: &Path,
    world_index: Option<&TrItemWorldIndex>,
    tile_filter: Option<(i32, i32)>,
) -> TrItemAuditReport {
    let assets = RouteAssets::new(route_dir);
    let tdb = assets.track_db().cloned().unwrap_or_default();
    run_tr_item_audit(
        route_dir,
        &tdb,
        Some(assets.tsection()),
        world_index,
        tile_filter,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TrackDbFile;

    #[test]
    fn tr_item_audit_on_with_signals_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/with_signals/route.tdb");
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let report = run_tr_item_audit(path.parent().unwrap(), &tdb, None, None, None);
        assert_eq!(report.summary.total_items, 1);
        assert_eq!(report.summary.single_host_ok, 1);
        assert_eq!(report.summary.signal_items, 1);
        assert_eq!(report.items[0].host_vector_id, Some(2));
        // Minimal zero-tile fixture may not resolve centreline pose; host mapping is still validated.
        if report.summary.pose_ok == 0 {
            assert_eq!(report.items[0].status, "no_tdb_pose");
        }
    }

    #[test]
    #[ignore = "needs MSTS Chiltern content"]
    fn export_chiltern_tr_item_audit() {
        let route = std::env::var("CHILTERN_ROUTE").unwrap_or_else(|_| {
            format!(
                "{}/routes/Chiltern Mainline/Routes/Chiltern",
                std::env::var("HOME").unwrap_or_default()
            )
        });
        let route_dir = std::path::Path::new(&route);
        if !route_dir.is_dir() {
            return;
        }
        let report = run_tr_item_audit_for_route(route_dir, None, Some((-6080, 14925)));
        log_tr_item_audit(&report);
        if let Ok(path) = std::env::var("OPENRAILSRS_TR_ITEM_AUDIT") {
            std::fs::write(&path, serde_json::to_string_pretty(&report).expect("json"))
                .expect("write audit json");
        }
        assert!(report.summary.signal_items > 0);
        assert!(report.summary.pose_ok > 0);
    }
}

//! TDB `TrItem` audit (TSRE `checkDatabase` subset).

use std::collections::HashSet;
use std::path::Path;

use bevy::prelude::*;
use openrailsrs_formats::{
    TSectionCatalog, TrItemKind, TrackDbFile, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord,
};
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
    /// WORLD tiles present in the index coverage set.
    pub coverage_tiles: usize,
    /// Signals whose TDB pose tile lies inside WORLD coverage (evaluable).
    pub evaluated_signals: usize,
    /// Signals skipped because pose tile is outside loaded WORLD coverage.
    pub signals_outside_coverage: usize,
    /// Evaluable signals with zero WORLD refs (`missing_in_loaded_tile`).
    pub signals_missing_in_loaded: usize,
    /// Items not evaluated against WORLD (outside coverage / no index).
    pub not_evaluated: usize,
    /// Errors among evaluable items only.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pose_tile: Option<[i32; 2]>,
    pub world_refs: usize,
    pub delta_world_xz_m: Option<f32>,
    /// `ok` | `host_count=N` | `no_tdb_pose` | `missing_in_loaded_tile` |
    /// `outside_coverage` | `world_delta_Xm`
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
    let coverage: HashSet<(i32, i32)> = world_index
        .map(|idx| idx.loaded_tiles().clone())
        .unwrap_or_default();
    let mut items = Vec::new();
    let mut summary = TrItemAuditSummary {
        total_items: tdb.items.len(),
        coverage_tiles: coverage.len(),
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

        let is_signal = matches!(item.kind, TrItemKind::Signal { .. });
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
        // Prefer pose-derived tile; fall back to host vector anchor tile for coverage checks.
        let pose_tile = pose_msts
            .map(|p| {
                [
                    msts_tile_x_index_for_coord(p[0]),
                    msts_tile_z_index_for_coord(p[2]),
                ]
            })
            .or_else(|| host_vector_tile(tdb, host_vector_id));

        let world_refs = world_index
            .map(|idx| idx.objects_for_item(item.id).len())
            .unwrap_or(0);
        if world_refs > 0 {
            summary.world_linked += 1;
        }

        let in_coverage = match pose_tile {
            Some([tx, tz]) if world_index.is_some() => coverage.contains(&(tx, tz)),
            _ => false,
        };
        let evaluable_for_world = world_index.is_some() && in_coverage;

        if is_signal && world_index.is_some() {
            if in_coverage {
                summary.evaluated_signals += 1;
            } else if pose_tile.is_some() {
                summary.signals_outside_coverage += 1;
                summary.not_evaluated += 1;
            }
        }

        let delta_world_xz_m = match (pose_msts, world_index) {
            (Some([x, _, z]), Some(idx)) if world_refs > 0 => {
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

        // Outside WORLD coverage: not an error (absence of WORLD data is expected).
        let (status, is_error) = if world_index.is_some()
            && pose_tile.is_some()
            && !in_coverage
            && (is_signal || world_refs == 0)
        {
            ("outside_coverage".into(), false)
        } else if host_count != 1 {
            (format!("host_count={host_count}"), true)
        } else if is_signal && evaluable_for_world && world_refs == 0 {
            // Tile is loaded: missing WORLD Signal is actionable even without centreline pose.
            summary.signals_missing_in_loaded += 1;
            ("missing_in_loaded_tile".into(), true)
        } else if pose_msts.is_none() {
            ("no_tdb_pose".into(), true)
        } else if evaluable_for_world
            && delta_world_xz_m.is_some_and(|d| d > TR_ITEM_WORLD_MATCH_RADIUS_M)
        {
            (
                format!("world_delta_{:.1}m", delta_world_xz_m.unwrap_or(0.0)),
                true,
            )
        } else {
            ("ok".into(), false)
        };

        if is_error {
            summary.errors += 1;
        }

        let list_item = match tile_filter {
            None => {
                // Keep report actionable: list errors + linked samples; skip bulk outside_coverage.
                status != "outside_coverage" || is_error || world_refs > 0
            }
            Some(tile) => {
                pose_tile.is_some_and(|p| p[0] == tile.0 && p[1] == tile.1)
                    || (status != "outside_coverage" && status != "ok")
            }
        };

        if list_item {
            items.push(TrItemAuditSample {
                tr_item_id: item.id,
                kind,
                host_count,
                host_vector_id,
                distance_m: item.distance_m,
                pose_msts,
                pose_tile,
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

pub fn log_tr_item_audit(report: &TrItemAuditReport) {
    crate::viewer_log!(
        "openrailsrs-viewer3d: tr_item audit — {} item(s), {} signal(s), {} evaluated, {} outside coverage, {} missing in loaded, {} world linked, {} errors ({} coverage tile(s))",
        report.summary.total_items,
        report.summary.signal_items,
        report.summary.evaluated_signals,
        report.summary.signals_outside_coverage,
        report.summary.signals_missing_in_loaded,
        report.summary.world_linked,
        report.summary.errors,
        report.summary.coverage_tiles
    );
    for sample in report
        .items
        .iter()
        .filter(|s| s.status != "ok" && s.status != "outside_coverage")
        .take(8)
    {
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

fn host_vector_tile(tdb: &TrackDbFile, host_vector_id: Option<u32>) -> Option<[i32; 2]> {
    let id = host_vector_id?;
    let node = tdb.node_by_id(id)?;
    if let Some(pos) = node.position {
        return Some([pos.tile_x, pos.tile_z]);
    }
    if let openrailsrs_formats::TrackNodeKind::Vector { sections, .. } = &node.kind {
        let s = sections.first()?;
        return Some([s.start.tile_x, s.start.tile_z]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::{
        SignalAspectKind, TrItem, TrItemKind, TrVectorSectionRecord, TrackDbFile, TrackDbNode,
        TrackNodeKind, TrackVectorPoint,
    };
    use std::collections::HashSet;

    use crate::tr_item_index::TrItemWorldIndex;
    use crate::world::WorldObject;

    fn vector_host(
        id: u32,
        item_ids: Vec<u32>,
        tile: (i32, i32),
        local_x: f64,
        local_z: f64,
    ) -> TrackDbNode {
        TrackDbNode {
            id,
            position: Some(TrackVectorPoint {
                tile_x: tile.0,
                tile_z: tile.1,
                x: local_x,
                y: 0.0,
                z: local_z,
            }),
            pin_refs: Vec::new(),
            kind: TrackNodeKind::Vector {
                length_m: 25.0,
                speed_limit_mps: 0.0,
                pins: (0, 0),
                item_ids,
                sections: vec![TrVectorSectionRecord {
                    shape_idx: 1,
                    aux_shape_idx: 0,
                    header_tile_x: tile.0,
                    header_tile_z: tile.1,
                    start: TrackVectorPoint {
                        tile_x: tile.0,
                        tile_z: tile.1,
                        x: local_x,
                        y: 0.0,
                        z: local_z,
                    },
                    ax: 0.0,
                    ay: 0.0,
                    az: 0.0,
                }],
                geometry: None,
            },
        }
    }

    fn signal_item(id: u32, distance_m: f64) -> TrItem {
        TrItem {
            world: None,
            id,
            distance_m,
            kind: TrItemKind::Signal {
                aspect_initial: SignalAspectKind::Stop,
            },
        }
    }

    fn bevy_xz_on_tile(tile: (i32, i32), local_x: f64, local_z: f64) -> Vec3 {
        let (x, y, z) = TrackVectorPoint {
            tile_x: tile.0,
            tile_z: tile.1,
            x: local_x,
            y: 0.0,
            z: local_z,
        }
        .bevy_position();
        Vec3::new(x, y, z)
    }

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
        if report.summary.pose_ok == 0 {
            assert_eq!(report.items[0].status, "no_tdb_pose");
        }
    }

    #[test]
    fn signal_outside_coverage_is_not_error() {
        let loaded = (-6080, 14925);
        let outside = (-6090, 14900);
        let mut tdb = TrackDbFile::default();
        tdb.nodes.push(vector_host(2, vec![1], outside, 0.0, 0.0));
        tdb.items.push(signal_item(1, 0.0));

        let mut coverage = HashSet::new();
        coverage.insert(loaded);
        let index = TrItemWorldIndex::from_world_objects_with_coverage(&[], coverage);

        let report = run_tr_item_audit(Path::new("/tmp"), &tdb, None, Some(&index), None);
        assert_eq!(report.summary.coverage_tiles, 1);
        assert_eq!(report.summary.signals_outside_coverage, 1);
        assert_eq!(report.summary.signals_missing_in_loaded, 0);
        assert_eq!(report.summary.evaluated_signals, 0);
        assert_eq!(report.summary.errors, 0);
    }

    #[test]
    fn signal_missing_in_loaded_tile_is_error() {
        let tile = (-6080, 14925);
        let mut tdb = TrackDbFile::default();
        tdb.nodes.push(vector_host(2, vec![10], tile, 100.0, 200.0));
        tdb.items.push(signal_item(10, 0.0));

        let mut coverage = HashSet::new();
        coverage.insert(tile);
        let index = TrItemWorldIndex::from_world_objects_with_coverage(
            &[WorldObject {
                kind: "Static",
                uid: Some(1),
                label: "box".into(),
                shape_file: None,
                section_idx: None,
                position: bevy_xz_on_tile(tile, 100.0, 200.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
                tile_x: tile.0,
                tile_z: tile.1,
                forest: None,
                water: None,
                transfer: None,
                car_spawner: None,
                signal: None,
                tr_item_ids: vec![],
                static_detail_level: 0,
            }],
            coverage,
        );

        let report = run_tr_item_audit(Path::new("/tmp"), &tdb, None, Some(&index), None);
        assert_eq!(report.summary.evaluated_signals, 1);
        assert_eq!(report.summary.signals_missing_in_loaded, 1);
        assert_eq!(report.summary.errors, 1);
        assert!(
            report
                .items
                .iter()
                .any(|s| s.status == "missing_in_loaded_tile")
        );
    }

    #[test]
    fn signal_linked_in_loaded_tile_is_ok() {
        let tile = (-6080, 14925);
        let pos = bevy_xz_on_tile(tile, 100.0, 200.0);
        let mut tdb = TrackDbFile::default();
        tdb.nodes.push(vector_host(2, vec![11], tile, 100.0, 200.0));
        tdb.items.push(signal_item(11, 0.0));

        let mut coverage = HashSet::new();
        coverage.insert(tile);
        let index = TrItemWorldIndex::from_world_objects_with_coverage(
            &[WorldObject {
                kind: "Signal",
                uid: Some(7),
                label: "sig".into(),
                shape_file: Some("sig.s".into()),
                section_idx: None,
                position: pos,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
                tile_x: tile.0,
                tile_z: tile.1,
                forest: None,
                water: None,
                transfer: None,
                car_spawner: None,
                signal: None,
                tr_item_ids: vec![11],
                static_detail_level: 0,
            }],
            coverage,
        );

        let report = run_tr_item_audit(Path::new("/tmp"), &tdb, None, Some(&index), None);
        assert_eq!(report.summary.world_linked, 1);
        assert_eq!(report.summary.signals_missing_in_loaded, 0);
        assert_eq!(report.summary.evaluated_signals, 1);
        assert!(
            !report
                .items
                .iter()
                .any(|s| s.status == "missing_in_loaded_tile")
        );
        // May still error on no_tdb_pose if centreline unresolved; that is TDB-side, not WORLD coverage.
        assert_eq!(report.summary.signals_outside_coverage, 0);
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

//! Placement audit: compare graph, TDB, `.w` scenery and anchors (TSRE `checkDatabase` spec).

use bevy::prelude::*;
use openrailsrs_formats::RouteFile;
use openrailsrs_scenarios::ScenarioFile;
use serde::Serialize;
use std::path::Path;

use crate::shapes::RouteAssets;
use crate::track::TrackScene;
use crate::track_position::{
    TrackNodePlacement, TrackPositionResolver, anchor_delta_xz, graph_node_world,
    marker_render_world_at_node, route_start_bevy, track_position_on_graph_node,
};
use crate::world::{RouteWorldOffset, load_world_from_route_dir_near, msts_to_bevy};
use openrailsrs_formats::Vec3 as MstsVec3;

/// Paddington / London end of Chiltern (`RS_Let's go to Birmingham` **start**).
///
/// Historical name kept for callers; this is **not** Birmingham Snow Hill (dest ~−6111/14957).
pub const CHILTERN_BIRMINGHAM_TILE: (i32, i32) = (-6080, 14925);

/// Alias: scenario start / TrackPDP[0] area (Paddington platforms).
pub const CHILTERN_PADDINGTON_TILE: (i32, i32) = CHILTERN_BIRMINGHAM_TILE;

#[derive(Clone, Debug, Serialize)]
pub struct PlacementAuditReport {
    pub tile_x: i32,
    pub tile_z: i32,
    pub world_anchor: Option<AnchorSample>,
    pub trk_route_start: Option<AnchorSample>,
    pub route_world_offset: [f32; 3],
    pub graph_start: Option<PositionSample>,
    pub stops: Vec<StopPlacementSample>,
    pub scenery_candidates: Vec<SceneryCandidate>,
    pub deltas: PlacementDeltas,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnchorSample {
    pub tile_x: i32,
    pub tile_z: i32,
    pub local_x_m: f64,
    pub local_y_m: f64,
    pub local_z_m: f64,
    pub bevy: [f32; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct PositionSample {
    pub label: String,
    pub bevy: [f32; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct StopPlacementSample {
    pub node_id: String,
    pub graph_bevy: Option<[f32; 3]>,
    pub tdb_node_id: Option<u32>,
    pub tdb_bevy: Option<[f32; 3]>,
    /// Position used by gameplay markers (TDB when available).
    pub marker_bevy: Option<[f32; 3]>,
    pub delta_graph_tdb_xz_m: Option<f32>,
    /// Marker Y − TDB Y when both exist (render-space vs MSTS absolute — see note in audit).
    pub delta_marker_tdb_y_m: Option<f32>,
    /// `id_validated` | `nearest` | `graph_fallback`
    pub mapping_method: String,
    /// XZ distance to the raw/alias ID pose even when rejected.
    pub id_delta_m: Option<f32>,
    pub rejected_tdb_id: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SceneryCandidate {
    pub kind: String,
    pub uid: u32,
    pub file_name: Option<String>,
    pub bevy: [f32; 3],
    pub delta_to_nearest_tdb_xz_m: Option<f32>,
    /// Object Y − nearest TDB centreline Y (metres). Negative ⇒ object below rail.
    pub delta_y_m: Option<f32>,
    /// `|pitch_obj − pitch_tdb|` when both poses expose pitch (#65); else omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_pitch_rad: Option<f32>,
    /// `|roll_obj − roll_tdb|` when both poses expose roll (#65); else omitted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_roll_rad: Option<f32>,
    pub delta_to_graph_stop_xz_m: Option<f32>,
    /// Present when `delta_to_nearest_tdb_xz_m` is `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nearest_tdb_miss_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlacementDeltas {
    pub anchor_vs_trk_start_xz_m: Option<f32>,
    pub anchor_vs_graph_start_xz_m: Option<f32>,
    pub graph_start_vs_trk_start_xz_m: Option<f32>,
    pub max_scenery_to_tdb_xz_m: Option<f32>,
    /// Largest |ΔY| among scenery with a TDB match (metres).
    pub max_scenery_to_tdb_abs_dy_m: Option<f32>,
    /// Vector nodes indexed on the audit tile (exact).
    pub tdb_vector_nodes_on_tile: usize,
    /// Vector nodes in the 3×3 tile ring around the audit tile.
    pub tdb_vector_nodes_in_ring: usize,
    pub scenery_with_tdb_match: usize,
    pub scenery_without_tdb_match: usize,
    /// Scenery Y significantly below nearest TDB (`delta_y_m < -`[`BURIED_Y_THRESHOLD_M`]).
    pub scenery_buried_vs_tdb: usize,
    /// Scenery Y significantly above nearest TDB (`delta_y_m > `[`FLOATING_Y_THRESHOLD_M`]).
    pub scenery_floating_vs_tdb: usize,
}

/// Search radius for WORLD→TDB centreline (metres).
const SCENERY_TDB_RADIUS_M: f32 = 250.0;

/// Object below TDB rail by more than this (m) → buried (e.g. rail flattened to terrain).
pub const BURIED_Y_THRESHOLD_M: f32 = 2.0;
/// Object above TDB by more than this (m) → floating.
pub const FLOATING_Y_THRESHOLD_M: f32 = 5.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct WorldAnchorInput {
    pub tile_x: i32,
    pub tile_z: i32,
    pub local_x_m: f64,
    pub local_y_m: f64,
    pub local_z_m: f64,
}

impl WorldAnchorInput {
    pub fn bevy(self) -> Vec3 {
        msts_to_bevy(
            self.tile_x,
            self.tile_z,
            MstsVec3 {
                x: self.local_x_m,
                y: self.local_y_m,
                z: self.local_z_m,
            },
        )
    }
}

pub fn run_placement_audit(
    route_dir: &Path,
    scene: &TrackScene,
    scenario: &ScenarioFile,
    offset: RouteWorldOffset,
    world_anchor: Option<WorldAnchorInput>,
    tile: (i32, i32),
    stop_nodes: &[&str],
) -> PlacementAuditReport {
    let assets = RouteAssets::new(route_dir);
    let tdb = assets.track_db().cloned().unwrap_or_default();
    let tsection = assets.tsection();
    let resolver = TrackPositionResolver::from_track_scene(&tdb, Some(tsection), scene);

    let trk_start = match RouteFile::from_route_dir(route_dir) {
        Ok(route) => {
            if let Some(path) = route.source_path.as_ref() {
                eprintln!(
                    "openrailsrs-viewer3d: placement-audit .trk {}",
                    path.display()
                );
            }
            if route.route_start.is_none() {
                eprintln!(
                    "openrailsrs-viewer3d: placement-audit .trk has no RouteStart (fallback to overlay/graph)"
                );
            }
            route.route_start
        }
        Err(err) => {
            eprintln!("openrailsrs-viewer3d: placement-audit failed to load .trk: {err}");
            None
        }
    };
    let anchor_bevy = world_anchor.map(|a| a.bevy());
    let trk_bevy = trk_start.map(route_start_bevy);

    let graph_start = graph_start_from_scenario(scene, scenario, offset);

    let focus_center = anchor_bevy
        .or(trk_bevy)
        .or(graph_start)
        .unwrap_or(scene.bounds.center);
    let focus = crate::world::RouteFocus::at_world_center(focus_center, None);

    let mut stops = Vec::new();
    for node_id in stop_nodes {
        let placement =
            track_position_on_graph_node(scene, scenario, &resolver, offset, node_id, 0.0);
        stops.push(stop_sample(
            placement, node_id, &resolver, scene, offset, &focus,
        ));
    }

    let world = load_world_from_route_dir_near(route_dir, Some(focus_center), 8000.0);
    let scenery_stop = stops
        .first()
        .and_then(|s| s.tdb_bevy.map(vec3_from_arr))
        .or_else(|| stops.first().and_then(|s| s.graph_bevy.map(vec3_from_arr)));
    let tdb_on_tile = resolver.vector_node_count_on_tile(tile.0, tile.1);
    let mut tdb_in_ring = 0usize;
    for dx in -1..=1 {
        for dz in -1..=1 {
            tdb_in_ring += resolver.vector_node_count_on_tile(tile.0 + dx, tile.1 + dz);
        }
    }
    eprintln!(
        "openrailsrs-viewer3d: placement-audit TDB vectors on tile {},{}: {tdb_on_tile} (3×3 ring: {tdb_in_ring})",
        tile.0, tile.1
    );
    let scenery_candidates = collect_scenery_candidates(
        &world,
        tile,
        &resolver,
        scenery_stop,
        tdb_on_tile,
        tdb_in_ring,
    );

    let max_scenery_tdb = scenery_candidates
        .iter()
        .filter_map(|c| c.delta_to_nearest_tdb_xz_m)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let max_scenery_abs_dy = scenery_candidates
        .iter()
        .filter_map(|c| c.delta_y_m.map(f32::abs))
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let scenery_with_tdb_match = scenery_candidates
        .iter()
        .filter(|c| c.delta_to_nearest_tdb_xz_m.is_some())
        .count();
    let scenery_without_tdb_match = scenery_candidates.len() - scenery_with_tdb_match;
    let scenery_buried_vs_tdb = scenery_candidates
        .iter()
        .filter(|c| c.delta_y_m.is_some_and(|dy| dy < -BURIED_Y_THRESHOLD_M))
        .count();
    let scenery_floating_vs_tdb = scenery_candidates
        .iter()
        .filter(|c| c.delta_y_m.is_some_and(|dy| dy > FLOATING_Y_THRESHOLD_M))
        .count();

    PlacementAuditReport {
        tile_x: tile.0,
        tile_z: tile.1,
        world_anchor: world_anchor.map(|a| anchor_sample(a.bevy(), a)),
        trk_route_start: trk_start.map(|s| {
            let b = route_start_bevy(s);
            AnchorSample {
                tile_x: s.tile_x,
                tile_z: s.tile_z,
                local_x_m: s.local_x_m,
                local_y_m: 0.0,
                local_z_m: s.local_z_m,
                bevy: vec3_to_arr(b),
            }
        }),
        route_world_offset: vec3_to_arr(offset.delta),
        graph_start: graph_start.map(|p| PositionSample {
            label: "graph_start".into(),
            bevy: vec3_to_arr(p),
        }),
        stops,
        scenery_candidates,
        deltas: PlacementDeltas {
            anchor_vs_trk_start_xz_m: match (anchor_bevy, trk_bevy) {
                (Some(a), Some(t)) => Some(anchor_delta_xz(a, t)),
                _ => None,
            },
            anchor_vs_graph_start_xz_m: match (anchor_bevy, graph_start) {
                (Some(a), Some(g)) => Some(anchor_delta_xz(a, g)),
                _ => None,
            },
            graph_start_vs_trk_start_xz_m: match (graph_start, trk_bevy) {
                (Some(g), Some(t)) => Some(anchor_delta_xz(g, t)),
                _ => None,
            },
            max_scenery_to_tdb_xz_m: max_scenery_tdb,
            max_scenery_to_tdb_abs_dy_m: max_scenery_abs_dy,
            tdb_vector_nodes_on_tile: tdb_on_tile,
            tdb_vector_nodes_in_ring: tdb_in_ring,
            scenery_with_tdb_match,
            scenery_without_tdb_match,
            scenery_buried_vs_tdb,
            scenery_floating_vs_tdb,
        },
    }
}

fn graph_start_from_scenario(
    scene: &TrackScene,
    scenario: &ScenarioFile,
    offset: RouteWorldOffset,
) -> Option<Vec3> {
    let start = &scenario.route.start;
    graph_node_world(scene, offset, start).or_else(|| {
        crate::track_position::graph_position_at_route_node(scene, scenario, offset, start)
    })
}

fn anchor_sample(bevy: Vec3, a: WorldAnchorInput) -> AnchorSample {
    AnchorSample {
        tile_x: a.tile_x,
        tile_z: a.tile_z,
        local_x_m: a.local_x_m,
        local_y_m: a.local_y_m,
        local_z_m: a.local_z_m,
        bevy: vec3_to_arr(bevy),
    }
}

fn stop_sample(
    p: TrackNodePlacement,
    node_id: &str,
    resolver: &TrackPositionResolver<'_>,
    scene: &TrackScene,
    offset: RouteWorldOffset,
    focus: &crate::world::RouteFocus,
) -> StopPlacementSample {
    let delta = p.delta_graph_tdb_xz_m();
    let marker = marker_render_world_at_node(
        node_id,
        0.0,
        Some(resolver),
        scene,
        offset,
        None,
        focus,
        p.graph_world,
    );
    // Compare absolute TDB Y to marker after undoing height_origin (same MSTS frame).
    let delta_marker_tdb_y = match (marker, p.tdb_pose) {
        (Some(m), Some(pose)) => {
            let marker_msl = m.y + focus.height_origin;
            Some(marker_msl - pose.position.y)
        }
        _ => None,
    };
    StopPlacementSample {
        node_id: p.node_id,
        graph_bevy: p.graph_world.map(vec3_to_arr),
        tdb_node_id: p.tdb_node_id,
        tdb_bevy: p.tdb_pose.map(|pose| vec3_to_arr(pose.position)),
        marker_bevy: marker.map(vec3_to_arr),
        delta_graph_tdb_xz_m: delta,
        delta_marker_tdb_y_m: delta_marker_tdb_y,
        mapping_method: p.method.as_str().to_string(),
        id_delta_m: p.id_delta_m,
        rejected_tdb_id: p.rejected_tdb_id,
    }
}

fn collect_scenery_candidates(
    world: &crate::world::WorldScene,
    tile: (i32, i32),
    resolver: &TrackPositionResolver<'_>,
    graph_stop: Option<Vec3>,
    tdb_on_tile: usize,
    tdb_in_ring: usize,
) -> Vec<SceneryCandidate> {
    // Preselect near the stop (or all TrackObj) before the expensive TDB nearest query.
    const PRESELECT: usize = 80;
    let mut pre: Vec<&crate::world::WorldObject> = Vec::new();
    for obj in &world.items {
        if obj.tile_x != tile.0 || obj.tile_z != tile.1 {
            continue;
        }
        if obj.kind != "Static" && obj.kind != "TrackObj" {
            continue;
        }
        let is_candidate =
            obj.shape_file.as_deref().is_some_and(canopy_like_shape) || obj.kind == "TrackObj";
        if !is_candidate {
            continue;
        }
        pre.push(obj);
    }
    if let Some(g) = graph_stop {
        pre.sort_by(|a, b| {
            let da = Vec2::new(a.position.x - g.x, a.position.z - g.z).length();
            let db = Vec2::new(b.position.x - g.x, b.position.z - g.z).length();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    pre.truncate(PRESELECT);

    let mut out = Vec::new();
    for obj in pre {
        let name = obj.shape_file.clone();
        let xz = Vec2::new(obj.position.x, obj.position.z);
        let tdb_near = resolver.nearest_on_tile_ring(xz, SCENERY_TDB_RADIUS_M, tile.0, tile.1);
        let delta_tdb = tdb_near.map(|p| {
            Vec2::new(obj.position.x - p.position.x, obj.position.z - p.position.z).length()
        });
        let delta_y = tdb_near.map(|p| obj.position.y - p.position.y);
        // WORLD Matrix3×3 / QDirection pitch-roll vs TDB pose when available (#65/#70).
        let (delta_pitch, delta_roll) = match tdb_near {
            Some(pose) => {
                let (obj_pitch, obj_roll) = world_object_pitch_roll(obj);
                (
                    Some((obj_pitch - pose.pitch_rad).abs()),
                    Some((obj_roll - pose.roll_rad).abs()),
                )
            }
            None => (None, None),
        };
        let miss_reason = if delta_tdb.is_some() {
            None
        } else {
            Some(nearest_tdb_miss_reason(
                name.as_deref(),
                tdb_on_tile,
                tdb_in_ring,
                SCENERY_TDB_RADIUS_M,
            ))
        };
        let delta_graph =
            graph_stop.map(|g| Vec2::new(obj.position.x - g.x, obj.position.z - g.z).length());
        out.push(SceneryCandidate {
            kind: obj.kind.to_string(),
            uid: obj.uid.unwrap_or(0),
            file_name: name,
            bevy: vec3_to_arr(obj.position),
            delta_to_nearest_tdb_xz_m: delta_tdb,
            delta_y_m: delta_y,
            delta_pitch_rad: delta_pitch,
            delta_roll_rad: delta_roll,
            delta_to_graph_stop_xz_m: delta_graph,
            nearest_tdb_miss_reason: miss_reason,
        });
    }
    // Prefer candidates with a TDB match; among matches, largest delta first (worst cases).
    out.sort_by(
        |a, b| match (a.delta_to_nearest_tdb_xz_m, b.delta_to_nearest_tdb_xz_m) {
            (Some(da), Some(db)) => db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
    );
    out.truncate(25);
    out
}

fn nearest_tdb_miss_reason(
    file_name: Option<&str>,
    tdb_on_tile: usize,
    tdb_in_ring: usize,
    radius_m: f32,
) -> String {
    if tdb_on_tile == 0 && tdb_in_ring == 0 {
        return "tile_without_tdb_vectors".into();
    }
    if file_name.is_some_and(|n| {
        let lower = n.to_ascii_lowercase();
        lower.contains("road") || lower.contains("street") || lower.contains("hwy")
    }) {
        return "likely_road_shape_outside_rail_radius".into();
    }
    format!("outside_{radius_m:.0}m_tdb_centreline_ring")
}

fn canopy_like_shape(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("canop")
        || lower.contains("roof")
        || lower.contains("shelter")
        || lower.contains("platform")
        || lower.contains("station")
        || lower.contains("marques")
}

fn vec3_to_arr(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}

fn vec3_from_arr(a: [f32; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

pub fn log_placement_audit(report: &PlacementAuditReport) {
    crate::viewer_log!(
        "openrailsrs-viewer3d: placement-audit tile {},{} — anchor vs trk {} | anchor vs graph {} | max scenery→tdb {} | max |ΔY| {} | buried/floating {}/{} | tdb match {}/{} (vectors tile/ring {}/{})",
        report.tile_x,
        report.tile_z,
        fmt_opt_m(report.deltas.anchor_vs_trk_start_xz_m),
        fmt_opt_m(report.deltas.anchor_vs_graph_start_xz_m),
        fmt_opt_m(report.deltas.max_scenery_to_tdb_xz_m),
        fmt_opt_m(report.deltas.max_scenery_to_tdb_abs_dy_m),
        report.deltas.scenery_buried_vs_tdb,
        report.deltas.scenery_floating_vs_tdb,
        report.deltas.scenery_with_tdb_match,
        report.deltas.scenery_with_tdb_match + report.deltas.scenery_without_tdb_match,
        report.deltas.tdb_vector_nodes_on_tile,
        report.deltas.tdb_vector_nodes_in_ring,
    );
    for stop in &report.stops {
        crate::viewer_log!(
            "  stop {} — graph/tdb Δxz {} ΔY {} ({})",
            stop.node_id,
            fmt_opt_m(stop.delta_graph_tdb_xz_m),
            fmt_opt_m(stop.delta_marker_tdb_y_m),
            stop.mapping_method,
        );
    }
}

/// Best-effort pitch/roll from a WORLD object transform (radians).
fn world_object_pitch_roll(obj: &crate::world::WorldObject) -> (f32, f32) {
    let (_yaw, pitch, roll) = obj.rotation.to_euler(EulerRot::YXZ);
    (pitch, roll)
}

fn fmt_opt_m(v: Option<f32>) -> String {
    match v {
        Some(m) if m.is_finite() => format!("{m:.1}m"),
        _ => "n/a".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_route::load_track_graph_from_route_dir;
    use openrailsrs_scenarios::load_scenario;

    #[test]
    fn chiltern_placement_audit_synthetic_without_msts() {
        let scenario_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let scenario_path = scenario_dir.join("scenario.toml");
        if !scenario_path.is_file() {
            return;
        }
        let scenario = load_scenario(&scenario_path).expect("scenario");
        let graph = load_track_graph_from_route_dir(&scenario_dir).expect("graph");
        let scene = TrackScene::from_graph(graph);
        let anchor = WorldAnchorInput {
            tile_x: -6080,
            tile_z: 14925,
            local_x_m: 891.831,
            local_y_m: 35.7818,
            local_z_m: 582.756,
        };
        let graph_start = graph_start_from_scenario(&scene, &scenario, RouteWorldOffset::default());
        let offset = RouteWorldOffset {
            delta: Vec3::new(
                anchor.bevy().x - graph_start.unwrap().x,
                0.0,
                anchor.bevy().z - graph_start.unwrap().z,
            ),
        };
        let report = run_placement_audit(
            &scenario_dir,
            &scene,
            &scenario,
            offset,
            Some(anchor),
            CHILTERN_BIRMINGHAM_TILE,
            &["n10778", "n3"],
        );
        assert!(report.deltas.anchor_vs_graph_start_xz_m.unwrap_or(999.0) < 1.0);
        assert!(report.stops.iter().any(|s| s.node_id == "n10778"));
    }

    #[test]
    fn nearest_miss_reason_codes_are_structured() {
        assert_eq!(
            nearest_tdb_miss_reason(None, 0, 0, 250.0),
            "tile_without_tdb_vectors"
        );
        assert_eq!(
            nearest_tdb_miss_reason(Some("hwy2l2wnaT20m90dhwy2l.s"), 10, 20, 250.0),
            "likely_road_shape_outside_rail_radius"
        );
        assert_eq!(
            nearest_tdb_miss_reason(Some("A1t100mStrt.s"), 10, 20, 250.0),
            "outside_250m_tdb_centreline_ring"
        );
        assert_eq!(fmt_opt_m(None), "n/a");
        assert_eq!(fmt_opt_m(Some(12.34)), "12.3m");
    }

    #[test]
    fn delta_y_fields_and_buried_threshold_flag_rail_at_terrain() {
        // Birmingham-like: TDB rail Y=35.8, object flattened to terrain Y=28.5 → ΔY≈−7.3.
        let dy = 28.5 - 35.7818;
        assert!(dy < -BURIED_Y_THRESHOLD_M, "must count as buried, dy={dy}");
        let floating_dy = 35.7818 + FLOATING_Y_THRESHOLD_M + 0.1 - 35.7818;
        assert!(floating_dy > FLOATING_Y_THRESHOLD_M);

        let candidate = SceneryCandidate {
            kind: "TrackObj".into(),
            uid: 1,
            file_name: Some("A1t100mStrt.s".into()),
            bevy: [0.0, 28.5, 0.0],
            delta_to_nearest_tdb_xz_m: Some(0.5),
            delta_y_m: Some(dy),
            delta_pitch_rad: Some(0.0),
            delta_roll_rad: Some(0.0),
            delta_to_graph_stop_xz_m: None,
            nearest_tdb_miss_reason: None,
        };
        assert!(candidate.delta_y_m.is_some());
        let buried = candidate
            .delta_y_m
            .is_some_and(|d| d < -BURIED_Y_THRESHOLD_M);
        assert!(buried, "rail-at-terrain must be flagged buried");

        let deltas = PlacementDeltas {
            anchor_vs_trk_start_xz_m: None,
            anchor_vs_graph_start_xz_m: None,
            graph_start_vs_trk_start_xz_m: None,
            max_scenery_to_tdb_xz_m: Some(0.5),
            max_scenery_to_tdb_abs_dy_m: Some(dy.abs()),
            tdb_vector_nodes_on_tile: 1,
            tdb_vector_nodes_in_ring: 1,
            scenery_with_tdb_match: 1,
            scenery_without_tdb_match: 0,
            scenery_buried_vs_tdb: 1,
            scenery_floating_vs_tdb: 0,
        };
        assert!(deltas.max_scenery_to_tdb_abs_dy_m.unwrap() > BURIED_Y_THRESHOLD_M);
        assert_eq!(deltas.scenery_buried_vs_tdb, 1);
    }

    #[test]
    #[ignore = "requires OPENRAILSRS_MSTS_CONTENT / Chiltern route"]
    fn chiltern_birmingham_trackobj_has_finite_tdb_nearest() {
        let route = std::env::var("CHILTERN_ROUTE")
            .or_else(|_| {
                std::env::var("OPENRAILSRS_MSTS_CONTENT")
                    .map(|root| format!("{root}/Chiltern/ROUTES/Chiltern"))
            })
            .map(std::path::PathBuf::from)
            .expect("CHILTERN_ROUTE or OPENRAILSRS_MSTS_CONTENT");
        if !route.is_dir() {
            return;
        }
        let scenario_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        let scenario = load_scenario(scenario_dir.join("scenario.toml")).expect("scenario");
        let graph = load_track_graph_from_route_dir(&scenario_dir).expect("graph");
        let scene = TrackScene::from_graph(graph);
        let anchor = WorldAnchorInput {
            tile_x: -6080,
            tile_z: 14925,
            local_x_m: 891.831,
            local_y_m: 35.7818,
            local_z_m: 582.756,
        };
        let graph_start = graph_start_from_scenario(&scene, &scenario, RouteWorldOffset::default());
        let offset = RouteWorldOffset {
            delta: Vec3::new(
                anchor.bevy().x - graph_start.unwrap().x,
                0.0,
                anchor.bevy().z - graph_start.unwrap().z,
            ),
        };
        let report = run_placement_audit(
            &route,
            &scene,
            &scenario,
            offset,
            Some(anchor),
            CHILTERN_BIRMINGHAM_TILE,
            &["n10778", "n3"],
        );
        assert!(
            report.deltas.tdb_vector_nodes_on_tile > 0,
            "expected TDB vectors on Birmingham tile"
        );
        assert!(
            report.deltas.scenery_with_tdb_match > 0,
            "expected finite TrackObj→TDB deltas"
        );
        assert!(
            report
                .scenery_candidates
                .iter()
                .filter_map(|c| c.delta_to_nearest_tdb_xz_m)
                .all(|d| d.is_finite()),
            "deltas must be finite (not NaN)"
        );
        assert!(report.deltas.max_scenery_to_tdb_xz_m.is_some());
    }
}

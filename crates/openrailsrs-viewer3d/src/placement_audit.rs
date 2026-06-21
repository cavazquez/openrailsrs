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

/// Default Birmingham station tile for Chiltern validation.
pub const CHILTERN_BIRMINGHAM_TILE: (i32, i32) = (-6080, 14925);

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
}

#[derive(Clone, Debug, Serialize)]
pub struct SceneryCandidate {
    pub kind: String,
    pub uid: u32,
    pub file_name: Option<String>,
    pub bevy: [f32; 3],
    pub delta_to_nearest_tdb_xz_m: Option<f32>,
    pub delta_to_graph_stop_xz_m: Option<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlacementDeltas {
    pub anchor_vs_trk_start_xz_m: Option<f32>,
    pub anchor_vs_graph_start_xz_m: Option<f32>,
    pub graph_start_vs_trk_start_xz_m: Option<f32>,
    pub max_scenery_to_tdb_xz_m: Option<f32>,
}

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
    let resolver = TrackPositionResolver::new(&tdb, Some(tsection));

    let trk_start = RouteFile::from_route_dir(route_dir)
        .ok()
        .and_then(|r| r.route_start);
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
    let scenery_candidates = collect_scenery_candidates(&world, tile, &resolver, scenery_stop);

    let max_scenery_tdb = scenery_candidates
        .iter()
        .filter_map(|c| c.delta_to_nearest_tdb_xz_m)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

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
    StopPlacementSample {
        node_id: p.node_id,
        graph_bevy: p.graph_world.map(vec3_to_arr),
        tdb_node_id: p.tdb_node_id,
        tdb_bevy: p.tdb_pose.map(|pose| vec3_to_arr(pose.position)),
        marker_bevy: marker.map(vec3_to_arr),
        delta_graph_tdb_xz_m: delta,
    }
}

fn collect_scenery_candidates(
    world: &crate::world::WorldScene,
    tile: (i32, i32),
    resolver: &TrackPositionResolver<'_>,
    graph_stop: Option<Vec3>,
) -> Vec<SceneryCandidate> {
    let mut out = Vec::new();
    for obj in &world.items {
        if obj.tile_x != tile.0 || obj.tile_z != tile.1 {
            continue;
        }
        if obj.kind != "Static" && obj.kind != "TrackObj" {
            continue;
        }
        let name = obj.shape_file.clone();
        let is_candidate = name.as_deref().is_some_and(canopy_like_shape) || obj.kind == "TrackObj";
        if !is_candidate && obj.kind == "Static" {
            continue;
        }
        let xz = Vec2::new(obj.position.x, obj.position.z);
        let tdb_near = resolver.nearest_on_tile(xz, 250.0, tile.0, tile.1);
        let delta_tdb = tdb_near.map(|p| {
            Vec2::new(obj.position.x - p.position.x, obj.position.z - p.position.z).length()
        });
        let delta_graph =
            graph_stop.map(|g| Vec2::new(obj.position.x - g.x, obj.position.z - g.z).length());
        out.push(SceneryCandidate {
            kind: obj.kind.to_string(),
            uid: obj.uid.unwrap_or(0),
            file_name: name,
            bevy: vec3_to_arr(obj.position),
            delta_to_nearest_tdb_xz_m: delta_tdb,
            delta_to_graph_stop_xz_m: delta_graph,
        });
    }
    out.sort_by(|a, b| {
        b.delta_to_nearest_tdb_xz_m
            .unwrap_or(0.0)
            .partial_cmp(&a.delta_to_nearest_tdb_xz_m.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(25);
    out
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
        "openrailsrs-viewer3d: placement-audit tile {},{} — anchor vs trk {:.1}m | anchor vs graph {:.1}m | max scenery→tdb {:.1}m",
        report.tile_x,
        report.tile_z,
        report.deltas.anchor_vs_trk_start_xz_m.unwrap_or(f32::NAN),
        report.deltas.anchor_vs_graph_start_xz_m.unwrap_or(f32::NAN),
        report.deltas.max_scenery_to_tdb_xz_m.unwrap_or(f32::NAN),
    );
    for stop in &report.stops {
        crate::viewer_log!(
            "  stop {} — graph/tdb delta {:.1}m",
            stop.node_id,
            stop.delta_graph_tdb_xz_m.unwrap_or(f32::NAN),
        );
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
}

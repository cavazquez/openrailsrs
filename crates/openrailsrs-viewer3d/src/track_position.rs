//! Track position on the logical graph and MSTS `.tdb` centreline.
//!
//! Spec reference: TSRE5 `getDrawPositionOnTrNode`, OR `FindLocationInSection`.
//!
//! Visual graph↔TDB correspondence must be spatially validated: a raw `nNNNN` /
//! import alias ID is accepted only when the TDB pose lies within
//! [`TDB_ID_MAX_DELTA_M`] of the graph hint. Otherwise nearest-centreline snap
//! or graph fallback is used (simulation odometry stays on the graph).

use std::collections::HashMap;

use bevy::prelude::*;
use openrailsrs_bevy_scenery::spawn::tdb_track::{
    TrackPose, nearest_track_position, tdb_node_track_pose,
};
use openrailsrs_formats::{
    RouteStart, TSectionCatalog, TrackDbFile, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord,
};
use openrailsrs_route::edge_path;
use openrailsrs_scenarios::ScenarioFile;
use openrailsrs_track::TrackGraph;

use crate::launch::{RunCorridorPath, run_corridor_half_width_m};
use crate::terrain::{TerrainElevation, ground_y_at};
use crate::track::{TrackScene, graph_to_world, graph_to_world_with_offset};
use crate::train::graph_point_msts_world;
use crate::world::{RouteFocus, RouteWorldOffset};

/// Max XZ distance (m) between graph hint and TDB node pose for accepting an ID.
pub const TDB_ID_MAX_DELTA_M: f32 = 25.0;

/// Max horizontal snap from graph hint to `.tdb` centreline (nearest fallback).
pub const TDB_GRAPH_SNAP_RADIUS_M: f32 = 2500.0;

/// Override via `OPENRAILSRS_TDB_SNAP_RADIUS_M` (50–10000 m).
pub fn tdb_snap_radius_m() -> f32 {
    std::env::var("OPENRAILSRS_TDB_SNAP_RADIUS_M")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|r: &f32| (50.0..=10_000.0).contains(r))
        .unwrap_or(TDB_GRAPH_SNAP_RADIUS_M)
}

/// How a graph node was placed onto the TDB centreline for rendering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphTdbMethod {
    /// Import alias / `nNNNN` candidate within [`TDB_ID_MAX_DELTA_M`].
    IdValidated,
    /// Spatial nearest centreline within snap radius.
    Nearest,
    /// No usable TDB pose; keep graph hint.
    #[default]
    GraphFallback,
}

impl GraphTdbMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdValidated => "id_validated",
            Self::Nearest => "nearest",
            Self::GraphFallback => "graph_fallback",
        }
    }
}

/// Result of resolving a graph node to a visual TDB pose.
#[derive(Clone, Debug)]
pub struct GraphTdbResolution {
    pub method: GraphTdbMethod,
    pub tdb_node_id: Option<u32>,
    pub pose: Option<TrackPose>,
    /// XZ distance from graph hint to the candidate ID pose (even if rejected).
    pub id_delta_m: Option<f32>,
    pub rejected_tdb_id: Option<u32>,
}

pub fn msts_to_render_surface(
    msts: Vec3,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    focus: &RouteFocus,
) -> Vec3 {
    let y = ground_y_at(terrain, msts.x, msts.z, scene);
    focus.to_render_surface(Vec3::new(msts.x, y, msts.z))
}

pub fn snap_msts_to_tdb(
    resolver: &TrackPositionResolver<'_>,
    msts: Vec3,
    snap_radius_m: f32,
) -> Option<TrackPose> {
    let hint_xz = Vec2::new(msts.x, msts.z);
    let tile_x = msts_tile_x_index_for_coord(msts.x);
    let tile_z = msts_tile_z_index_for_coord(msts.z);
    resolver
        .nearest_on_tile(hint_xz, snap_radius_m, tile_x, tile_z)
        .or_else(|| {
            nearest_track_position(
                resolver.tdb,
                hint_xz,
                snap_radius_m,
                resolver.tsection,
                None,
            )
        })
}

/// Render-space position: snap graph hint to TDB when possible, else graph render fallback.
pub fn marker_render_world_from_msts_hint(
    msts_hint: Vec3,
    resolver: Option<&TrackPositionResolver<'_>>,
    graph_render_fallback: Vec3,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    focus: &RouteFocus,
) -> Vec3 {
    if let Some(res) = resolver {
        if let Some(pose) = snap_msts_to_tdb(res, msts_hint, tdb_snap_radius_m()) {
            return msts_to_render_surface(pose.position, terrain, scene, focus);
        }
    }
    graph_render_fallback
}

/// Shared resolver for graph + TDB placement (TSRE-style).
pub struct TrackPositionResolver<'a> {
    pub tdb: &'a TrackDbFile,
    pub tsection: Option<&'a TSectionCatalog>,
    pub tile_index: HashMap<(i32, i32), Vec<u32>>,
    /// Graph node id → TDB id from import aliases (preferred over `n` prefix).
    pub graph_node_to_tdb: HashMap<String, u32>,
}

impl<'a> TrackPositionResolver<'a> {
    pub fn new(tdb: &'a TrackDbFile, tsection: Option<&'a TSectionCatalog>) -> Self {
        Self {
            tdb,
            tsection,
            tile_index: tdb.index_nodes_by_tile(),
            graph_node_to_tdb: HashMap::new(),
        }
    }

    pub fn from_track_scene(
        tdb: &'a TrackDbFile,
        tsection: Option<&'a TSectionCatalog>,
        scene: &TrackScene,
    ) -> Self {
        Self::new(tdb, tsection).with_graph_tdb_map(scene.graph_node_to_tdb.clone())
    }

    pub fn with_graph_tdb_map(mut self, map: HashMap<String, u32>) -> Self {
        self.graph_node_to_tdb = map;
        self
    }

    /// Legacy helper: parse `nNNNN` only (no alias, no spatial check).
    pub fn parse_n_prefix_tdb_id(node_id: &str) -> Option<u32> {
        node_id.strip_prefix('n')?.parse().ok()
    }

    /// Candidate TDB id from import alias, else `nNNNN` prefix.
    pub fn candidate_tdb_id(&self, node_id: &str) -> Option<u32> {
        self.graph_node_to_tdb
            .get(node_id)
            .copied()
            .or_else(|| Self::parse_n_prefix_tdb_id(node_id))
    }

    /// Resolve a graph node to a visual TDB pose with spatial ID validation.
    pub fn resolve_graph_node_visual(
        &self,
        node_id: &str,
        chainage_m: f64,
        graph_hint: Option<Vec3>,
        snap_radius_m: f32,
    ) -> GraphTdbResolution {
        let mut id_delta_m = None;
        let mut rejected_tdb_id = None;

        if let Some(id) = self.candidate_tdb_id(node_id) {
            if let Some(pose) = self.tdb_pose(id, chainage_m, graph_hint) {
                match graph_hint {
                    Some(hint) => {
                        let delta =
                            Vec2::new(hint.x - pose.position.x, hint.z - pose.position.z).length();
                        id_delta_m = Some(delta);
                        if delta <= TDB_ID_MAX_DELTA_M {
                            return GraphTdbResolution {
                                method: GraphTdbMethod::IdValidated,
                                tdb_node_id: Some(id),
                                pose: Some(pose),
                                id_delta_m,
                                rejected_tdb_id: None,
                            };
                        }
                        rejected_tdb_id = Some(id);
                    }
                    // Without a spatial hint the numeric ID alone is not trusted.
                    None => {
                        rejected_tdb_id = Some(id);
                    }
                }
            }
        }

        if let Some(hint) = graph_hint {
            if let Some(pose) = snap_msts_to_tdb(self, hint, snap_radius_m) {
                return GraphTdbResolution {
                    method: GraphTdbMethod::Nearest,
                    tdb_node_id: None,
                    pose: Some(pose),
                    id_delta_m,
                    rejected_tdb_id,
                };
            }
        }

        GraphTdbResolution {
            method: GraphTdbMethod::GraphFallback,
            tdb_node_id: None,
            pose: None,
            id_delta_m,
            rejected_tdb_id,
        }
    }

    pub fn tdb_pose(
        &self,
        tdb_node_id: u32,
        chainage_m: f64,
        near: Option<Vec3>,
    ) -> Option<TrackPose> {
        tdb_node_track_pose(self.tdb, tdb_node_id, chainage_m, self.tsection, near)
    }

    pub fn nearest_on_tile(
        &self,
        world_xz: Vec2,
        radius_m: f32,
        tile_x: i32,
        tile_z: i32,
    ) -> Option<TrackPose> {
        openrailsrs_bevy_scenery::spawn::tdb_track::nearest_track_position(
            self.tdb,
            world_xz,
            radius_m,
            self.tsection,
            Some((tile_x, tile_z)),
        )
    }

    /// Nearest TDB centreline within `radius_m`, searching the query tile and its 8 neighbours.
    ///
    /// Sections often cross tile boundaries; exact-tile filtering alone misses edge TrackObj.
    /// Tries the exact tile first (common case) before expanding the ring.
    pub fn nearest_on_tile_ring(
        &self,
        world_xz: Vec2,
        radius_m: f32,
        tile_x: i32,
        tile_z: i32,
    ) -> Option<TrackPose> {
        if let Some(pose) = self.nearest_on_tile(world_xz, radius_m, tile_x, tile_z) {
            return Some(pose);
        }
        let mut best: Option<TrackPose> = None;
        let mut best_dist = f64::from(radius_m);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let Some(pose) = self.nearest_on_tile(world_xz, radius_m, tile_x + dx, tile_z + dz)
                else {
                    continue;
                };
                let d = f64::from(
                    Vec2::new(world_xz.x - pose.position.x, world_xz.y - pose.position.z).length(),
                );
                if d < best_dist {
                    best_dist = d;
                    best = Some(pose);
                }
            }
        }
        best
    }

    /// Count vector nodes indexed on `tile` (diagnostic for placement audit).
    pub fn vector_node_count_on_tile(&self, tile_x: i32, tile_z: i32) -> usize {
        self.tile_index
            .get(&(tile_x, tile_z))
            .map(|ids| {
                ids.iter()
                    .filter(|id| {
                        self.tdb.node_by_id(**id).is_some_and(|n| {
                            matches!(
                                n.kind,
                                openrailsrs_formats::typed::TrackNodeKind::Vector { .. }
                            )
                        })
                    })
                    .count()
            })
            .unwrap_or(0)
    }
}

/// Graph node id → Bevy world (with route offset applied).
pub fn graph_node_world(
    scene: &TrackScene,
    offset: RouteWorldOffset,
    node_id: &str,
) -> Option<Vec3> {
    let node = scene.graph.node(node_id)?;
    Some(graph_to_world_with_offset(offset.delta, node.x_m, node.y_m))
}

/// Position along the scenario path at the given graph node (planar graph interpolation).
pub fn graph_position_at_route_node(
    scene: &TrackScene,
    scenario: &ScenarioFile,
    offset: RouteWorldOffset,
    target_node: &str,
) -> Option<Vec3> {
    let path_edges = edge_path(
        &scene.graph,
        &scenario.route.start,
        &scenario.route.destination,
    )
    .ok()?;
    let mut remaining_start = scenario.route.start_offset_m.unwrap_or(0.0).max(0.0);
    for edge_id in path_edges {
        let edge = scene.graph.edge(&edge_id)?;
        let from = scene.graph.node(&edge.from.0)?;
        let to = scene.graph.node(&edge.to.0)?;
        if edge.from.0 == target_node {
            return Some(graph_to_world_with_offset(offset.delta, from.x_m, from.y_m));
        }
        if edge.to.0 == target_node {
            return Some(graph_to_world_with_offset(offset.delta, to.x_m, to.y_m));
        }
        if edge.from.0 == scenario.route.start && remaining_start > 0.0 {
            if remaining_start <= edge.length_m || edge.length_m <= 0.0 {
                let frac = if edge.length_m > 0.0 {
                    (remaining_start / edge.length_m).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let x_m = from.x_m + frac * (to.x_m - from.x_m);
                let y_m = from.y_m + frac * (to.y_m - from.y_m);
                if target_node == scenario.route.start {
                    return Some(graph_to_world_with_offset(offset.delta, x_m, y_m));
                }
            }
            remaining_start -= edge.length_m;
        }
        if edge.to.0 == target_node {
            break;
        }
    }
    graph_node_world(scene, offset, target_node)
}

/// Combined graph + TDB pose for a stop node (spatially validated correspondence).
pub fn track_position_on_graph_node(
    scene: &TrackScene,
    scenario: &ScenarioFile,
    resolver: &TrackPositionResolver<'_>,
    offset: RouteWorldOffset,
    node_id: &str,
    chainage_m: f64,
) -> TrackNodePlacement {
    let graph_world = graph_position_at_route_node(scene, scenario, offset, node_id);
    let resolved =
        resolver.resolve_graph_node_visual(node_id, chainage_m, graph_world, tdb_snap_radius_m());
    TrackNodePlacement {
        node_id: node_id.to_string(),
        graph_world,
        tdb_node_id: resolved.tdb_node_id,
        tdb_pose: resolved.pose,
        method: resolved.method,
        id_delta_m: resolved.id_delta_m,
        rejected_tdb_id: resolved.rejected_tdb_id,
    }
}

#[derive(Clone, Debug)]
pub struct TrackNodePlacement {
    pub node_id: String,
    pub graph_world: Option<Vec3>,
    pub tdb_node_id: Option<u32>,
    pub tdb_pose: Option<TrackPose>,
    pub method: GraphTdbMethod,
    pub id_delta_m: Option<f32>,
    pub rejected_tdb_id: Option<u32>,
}

impl TrackNodePlacement {
    pub fn delta_graph_tdb_xz_m(&self) -> Option<f32> {
        let g = self.graph_world?;
        let t = self.tdb_pose?.position;
        Some(Vec2::new(g.x - t.x, g.z - t.z).length())
    }

    /// Render-space position for gameplay markers: TDB when available, else graph.
    pub fn marker_render_world(
        &self,
        terrain: Option<&TerrainElevation>,
        scene: &TrackScene,
        focus: &RouteFocus,
        graph_fallback: Option<Vec3>,
    ) -> Option<Vec3> {
        let mut world = if let Some(pose) = self.tdb_pose {
            pose.position
        } else {
            graph_fallback.or(self.graph_world)?
        };
        world.y = ground_y_at(terrain, world.x, world.z, scene);
        Some(focus.to_render_surface(world))
    }
}

/// Render-space world position for a stop/dest marker at a graph node.
///
/// When `.tdb` resolves the node, uses the MSTS centreline (no graph offset).
/// Otherwise uses `graph_fallback` (typically from `position_on_graph`, already in render space).
#[allow(clippy::too_many_arguments)]
pub fn marker_render_world_at_node(
    node_id: &str,
    chainage_m: f64,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    offset: RouteWorldOffset,
    terrain: Option<&TerrainElevation>,
    focus: &RouteFocus,
    graph_fallback: Option<Vec3>,
) -> Option<Vec3> {
    if let Some(res) = resolver {
        let near = graph_node_world(scene, offset, node_id).or(graph_fallback);
        let resolved =
            res.resolve_graph_node_visual(node_id, chainage_m, near, tdb_snap_radius_m());
        if let Some(pose) = resolved.pose {
            let mut world = pose.position;
            world.y = ground_y_at(terrain, world.x, world.z, scene);
            return Some(focus.to_render_surface(world));
        }
    }
    graph_fallback
}

/// Render-space position for a graph node (TDB node pose, then nearest snap, then graph).
pub fn marker_render_world_at_graph_node(
    node_id: &str,
    msts: Vec3,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    offset: RouteWorldOffset,
    terrain: Option<&TerrainElevation>,
    focus: &RouteFocus,
) -> Vec3 {
    let graph_render = msts_to_render_surface(msts, terrain, scene, focus);
    marker_render_world_at_node(
        node_id,
        0.0,
        resolver,
        scene,
        offset,
        terrain,
        focus,
        Some(graph_render),
    )
    .unwrap_or_else(|| {
        marker_render_world_from_msts_hint(msts, resolver, graph_render, terrain, scene, focus)
    })
}

/// Render-space position + yaw for a point on a graph edge (signals, edge markers).
#[allow(clippy::too_many_arguments)]
pub fn marker_render_world_on_edge(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    offset: RouteWorldOffset,
    terrain: Option<&TerrainElevation>,
    focus: &RouteFocus,
) -> Option<(Vec3, f32)> {
    let (msts, graph_yaw) =
        graph_point_msts_world(graph, edge_id, pos_on_edge_m, terrain, scene, offset.delta)?;
    let graph_render = msts_to_render_surface(msts, terrain, scene, focus);
    let render =
        marker_render_world_from_msts_hint(msts, resolver, graph_render, terrain, scene, focus);
    let yaw = resolver
        .and_then(|res| snap_msts_to_tdb(res, msts, tdb_snap_radius_m()))
        .map(|p| -p.yaw_deg.to_radians())
        .unwrap_or(graph_yaw);
    Some((render, yaw))
}

pub fn route_start_bevy(start: RouteStart) -> Vec3 {
    let (x, y, z) = start.bevy_position();
    Vec3::new(x, y, z)
}

pub fn anchor_delta_xz(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

const CORRIDOR_LONG_EDGE_M: f64 = 200.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CorridorSnapStats {
    pub total: usize,
    pub snapped_tdb_node: usize,
    pub snapped_nearest: usize,
    pub fallback_graph: usize,
    pub rejected_tdb_id: usize,
}

#[derive(Clone, Debug)]
struct CorridorWaypoint {
    msts_hint: Vec3,
    node_id: Option<String>,
}

fn snap_waypoint_msts(
    wp: &CorridorWaypoint,
    resolver: &TrackPositionResolver<'_>,
    snap_radius_m: f32,
) -> (Vec3, GraphTdbResolution) {
    if let Some(node_id) = &wp.node_id {
        let resolved =
            resolver.resolve_graph_node_visual(node_id, 0.0, Some(wp.msts_hint), snap_radius_m);
        let pos = resolved
            .pose
            .as_ref()
            .map(|p| p.position)
            .unwrap_or(wp.msts_hint);
        return (pos, resolved);
    }
    if let Some(pose) = snap_msts_to_tdb(resolver, wp.msts_hint, snap_radius_m) {
        return (
            pose.position,
            GraphTdbResolution {
                method: GraphTdbMethod::Nearest,
                tdb_node_id: None,
                pose: Some(pose),
                id_delta_m: None,
                rejected_tdb_id: None,
            },
        );
    }
    (
        wp.msts_hint,
        GraphTdbResolution {
            method: GraphTdbMethod::GraphFallback,
            tdb_node_id: None,
            pose: None,
            id_delta_m: None,
            rejected_tdb_id: None,
        },
    )
}

/// Snap scenario corridor polyline vertices to the `.tdb` centreline (MSTS world space).
pub fn snap_corridor_path_to_tdb(
    points: &mut [Vec3],
    node_ids: &[Option<String>],
    resolver: &TrackPositionResolver<'_>,
    snap_radius_m: f32,
) -> CorridorSnapStats {
    let mut stats = CorridorSnapStats {
        total: points.len(),
        ..Default::default()
    };
    for (pt, node_id) in points.iter_mut().zip(node_ids.iter()) {
        let wp = CorridorWaypoint {
            msts_hint: *pt,
            node_id: node_id.clone(),
        };
        let (snapped, resolved) = snap_waypoint_msts(&wp, resolver, snap_radius_m);
        *pt = snapped;
        if resolved.rejected_tdb_id.is_some() {
            stats.rejected_tdb_id += 1;
        }
        match resolved.method {
            GraphTdbMethod::IdValidated => stats.snapped_tdb_node += 1,
            GraphTdbMethod::Nearest => stats.snapped_nearest += 1,
            GraphTdbMethod::GraphFallback => stats.fallback_graph += 1,
        }
    }
    stats
}

/// Build corridor waypoints from the scenario graph path (before TDB snap).
fn build_graph_corridor_waypoints(
    scene: &TrackScene,
    scenario: &ScenarioFile,
    route_delta: Vec3,
) -> Result<Vec<CorridorWaypoint>, String> {
    let path_edges = edge_path(
        &scene.graph,
        &scenario.route.start,
        &scenario.route.destination,
    )
    .map_err(|e| e.to_string())?;
    let mut waypoints: Vec<CorridorWaypoint> = Vec::new();
    for edge_id in path_edges {
        let edge = scene
            .graph
            .edge(&edge_id)
            .ok_or_else(|| format!("missing edge {edge_id}"))?;
        let from = scene
            .graph
            .node(&edge.from.0)
            .ok_or_else(|| format!("missing node {}", edge.from.0))?;
        let to = scene
            .graph
            .node(&edge.to.0)
            .ok_or_else(|| format!("missing node {}", edge.to.0))?;
        let mut a = graph_to_world(from.x_m, from.y_m) + route_delta;
        let mut b = graph_to_world(to.x_m, to.y_m) + route_delta;
        if let Some(last) = waypoints.last() {
            if last.msts_hint.distance_squared(b) < last.msts_hint.distance_squared(a) {
                std::mem::swap(&mut a, &mut b);
            }
        }
        let push = |wps: &mut Vec<CorridorWaypoint>, msts: Vec3, node_id: Option<String>| {
            if wps
                .last()
                .is_none_or(|w| w.msts_hint.distance_squared(msts) > 1.0)
            {
                wps.push(CorridorWaypoint {
                    msts_hint: msts,
                    node_id,
                });
            }
        };
        push(&mut waypoints, a, Some(edge.from.0.clone()));
        if edge.length_m > CORRIDOR_LONG_EDGE_M {
            let frac = 0.5;
            let x_m = from.x_m + frac * (to.x_m - from.x_m);
            let y_m = from.y_m + frac * (to.y_m - from.y_m);
            let mid = graph_to_world(x_m, y_m) + route_delta;
            push(&mut waypoints, mid, None);
        }
        push(&mut waypoints, b, Some(edge.to.0.clone()));
    }
    Ok(waypoints)
}

fn waypoints_to_corridor_path(waypoints: &[CorridorWaypoint]) -> RunCorridorPath {
    RunCorridorPath {
        points_world: waypoints.iter().map(|w| w.msts_hint).collect(),
        half_width_m: run_corridor_half_width_m(),
    }
}

/// Scenario graph path snapped to `.tdb` centreline for `--run-corridor` filtering.
pub fn build_snapped_corridor_path(
    scene: &TrackScene,
    scenario: &ScenarioFile,
    route_delta: Vec3,
    tdb: &TrackDbFile,
    tsection: Option<&TSectionCatalog>,
) -> Result<RunCorridorPath, String> {
    let mut waypoints = build_graph_corridor_waypoints(scene, scenario, route_delta)?;
    let resolver = TrackPositionResolver::from_track_scene(tdb, tsection, scene);
    let node_ids: Vec<Option<String>> = waypoints.iter().map(|w| w.node_id.clone()).collect();
    let mut points: Vec<Vec3> = waypoints.iter().map(|w| w.msts_hint).collect();
    let stats = snap_corridor_path_to_tdb(&mut points, &node_ids, &resolver, tdb_snap_radius_m());
    for (wp, pt) in waypoints.iter_mut().zip(points.iter()) {
        wp.msts_hint = *pt;
    }
    crate::viewer_log!(
        "openrailsrs-viewer3d: run_corridor TDB snap — {}/{} id_validated, {}/{} nearest, {}/{} graph fallback, {} id_rejected (id≤{:.0}m, nearest≤{:.0}m)",
        stats.snapped_tdb_node,
        stats.total,
        stats.snapped_nearest,
        stats.total,
        stats.fallback_graph,
        stats.total,
        stats.rejected_tdb_id,
        TDB_ID_MAX_DELTA_M,
        tdb_snap_radius_m()
    );
    Ok(waypoints_to_corridor_path(&waypoints))
}

/// MSTS world position for a `TrItem` on its host vector (TSRE `getDrawPositionOnTrNode`).
pub fn tr_item_msts_world(
    tdb: &TrackDbFile,
    item_id: u32,
    tsection: Option<&TSectionCatalog>,
) -> Option<Vec3> {
    let item = tdb.item_by_id(item_id)?;
    let host = tdb.host_vector_for_item(item_id)?;
    let resolver = TrackPositionResolver::new(tdb, tsection);
    resolver
        .tdb_pose(host, item.distance_m, None)
        .map(|p| p.position)
}

pub fn parse_signal_tr_item_id(signal_id: &str) -> Option<u32> {
    signal_id.strip_prefix("sig")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TrackDbFile;
    use openrailsrs_track::TrackGraph;

    fn test_focus() -> RouteFocus {
        RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        }
    }

    fn empty_scene() -> TrackScene {
        TrackScene::from_graph(TrackGraph::default())
    }

    #[test]
    fn candidate_tdb_id_prefers_alias_over_n_prefix() {
        let tdb = TrackDbFile::default();
        let resolver = TrackPositionResolver::new(&tdb, None).with_graph_tdb_map(HashMap::from([
            ("jn_alpha".to_string(), 42),
            ("n10778".to_string(), 999),
        ]));
        assert_eq!(resolver.candidate_tdb_id("jn_alpha"), Some(42));
        assert_eq!(resolver.candidate_tdb_id("n10778"), Some(999));
        assert_eq!(resolver.candidate_tdb_id("n12"), Some(12));
        assert_eq!(resolver.candidate_tdb_id("bad"), None);
    }

    #[test]
    fn resolve_rejects_numeric_id_far_from_graph_hint() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_pos = resolver
            .tdb_pose(2, 0.0, None)
            .expect("vector node 2 on fixture");
        let far_hint = tdb_pos.position + Vec3::new(1800.0, 0.0, 1800.0);
        let resolved =
            resolver.resolve_graph_node_visual("n2", 0.0, Some(far_hint), tdb_snap_radius_m());
        assert_eq!(resolved.rejected_tdb_id, Some(2));
        assert_ne!(resolved.method, GraphTdbMethod::IdValidated);
        assert!(resolved.id_delta_m.unwrap_or(0.0) > TDB_ID_MAX_DELTA_M);
    }

    #[test]
    fn resolve_accepts_numeric_id_near_graph_hint() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_pos = resolver
            .tdb_pose(2, 0.0, None)
            .expect("vector node 2 on fixture");
        let near_hint = tdb_pos.position + Vec3::new(5.0, 0.0, 0.0);
        let resolved =
            resolver.resolve_graph_node_visual("n2", 0.0, Some(near_hint), tdb_snap_radius_m());
        assert_eq!(resolved.method, GraphTdbMethod::IdValidated);
        assert_eq!(resolved.tdb_node_id, Some(2));
        assert!(resolved.id_delta_m.unwrap_or(999.0) <= TDB_ID_MAX_DELTA_M);
    }

    #[test]
    fn route_start_bevy_uses_msts_convention() {
        let start = RouteStart {
            tile_x: -6080,
            tile_z: 14925,
            local_x_m: 891.831,
            local_z_m: 582.756,
        };
        let p = route_start_bevy(start);
        assert!(p.x < 0.0);
        assert!(p.z < 0.0);
    }

    #[test]
    fn marker_render_world_uses_validated_tdb_when_near() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_pos = resolver
            .tdb_pose(2, 0.0, None)
            .expect("vector node 2 on fixture");
        let scene = empty_scene();
        let focus = test_focus();
        let graph_hint = tdb_pos.position + Vec3::new(8.0, 0.0, 0.0);
        let world = marker_render_world_at_node(
            "n2",
            0.0,
            Some(&resolver),
            &scene,
            RouteWorldOffset::default(),
            None,
            &focus,
            Some(graph_hint),
        )
        .expect("tdb marker");
        let expected = msts_to_render_surface(tdb_pos.position, None, &scene, &focus);
        assert!(
            Vec2::new(world.x - expected.x, world.z - expected.z).length() < 5.0,
            "near ID should keep validated TDB centreline"
        );
    }

    #[test]
    fn marker_render_world_does_not_teleport_on_far_raw_id() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_pos = resolver
            .tdb_pose(2, 0.0, None)
            .expect("vector node 2 on fixture");
        let scene = empty_scene();
        let focus = test_focus();
        let graph_hint = tdb_pos.position + Vec3::new(1800.0, 0.0, 1800.0);
        let world = marker_render_world_at_node(
            "n2",
            0.0,
            Some(&resolver),
            &scene,
            RouteWorldOffset::default(),
            None,
            &focus,
            Some(graph_hint),
        )
        .expect("marker");
        let teleported = msts_to_render_surface(tdb_pos.position, None, &scene, &focus);
        let dist_to_raw_id = Vec2::new(world.x - teleported.x, world.z - teleported.z).length();
        assert!(
            dist_to_raw_id > 100.0,
            "far raw ID must not place the marker on the distant TDB node"
        );
    }

    #[test]
    fn marker_render_world_falls_back_without_tdb() {
        let scene = empty_scene();
        let focus = test_focus();
        let fallback = Vec3::new(1.0, 2.0, 3.0);
        let world = marker_render_world_at_node(
            "n999",
            0.0,
            None,
            &scene,
            RouteWorldOffset::default(),
            None,
            &focus,
            Some(fallback),
        )
        .expect("fallback");
        assert_eq!(world, fallback);
    }

    #[test]
    fn marker_render_world_on_edge_snaps_to_tdb_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_mid = resolver
            .tdb_pose(2, 50.0, None)
            .expect("chainage on vector 2");
        let scene = empty_scene();
        let focus = test_focus();
        let graph_hint = tdb_mid.position + Vec3::new(100.0, 0.0, 100.0);
        if snap_msts_to_tdb(&resolver, graph_hint, tdb_snap_radius_m()).is_none() {
            // Minimal fixture lacks tsection spans for spatial nearest; node pose tests cover TDB path.
            return;
        }
        let graph_render = msts_to_render_surface(graph_hint, None, &scene, &focus);
        let render = marker_render_world_from_msts_hint(
            graph_hint,
            Some(&resolver),
            graph_render,
            None,
            &scene,
            &focus,
        );
        let expected = msts_to_render_surface(tdb_mid.position, None, &scene, &focus);
        let dist_graph = Vec2::new(
            graph_hint.x - tdb_mid.position.x,
            graph_hint.z - tdb_mid.position.z,
        )
        .length();
        let dist_render = Vec2::new(render.x - expected.x, render.z - expected.z).length();
        assert!(
            dist_render < dist_graph && dist_render < 100.0,
            "edge-style hint should snap toward TDB (graph={dist_graph:.0}m render={dist_render:.0}m)"
        );
    }

    #[test]
    fn snap_corridor_path_validates_near_id() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_pos = resolver
            .tdb_pose(2, 0.0, None)
            .expect("vector node 2 on fixture");
        let hint = tdb_pos.position + Vec3::new(8.0, 0.0, 0.0);
        let mut points = vec![hint];
        let node_ids = vec![Some("n2".to_string())];
        let stats = snap_corridor_path_to_tdb(&mut points, &node_ids, &resolver, 2500.0);
        assert_eq!(stats.snapped_tdb_node, 1);
        assert_eq!(stats.rejected_tdb_id, 0);
        assert!((points[0] - tdb_pos.position).length() < 5.0);
    }

    #[test]
    fn snap_corridor_path_rejects_far_raw_id() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let tdb_pos = resolver
            .tdb_pose(2, 0.0, None)
            .expect("vector node 2 on fixture");
        let hint = tdb_pos.position + Vec3::new(1800.0, 0.0, 1800.0);
        let mut points = vec![hint];
        let node_ids = vec![Some("n2".to_string())];
        let stats = snap_corridor_path_to_tdb(&mut points, &node_ids, &resolver, 2500.0);
        assert_eq!(stats.snapped_tdb_node, 0);
        assert_eq!(stats.rejected_tdb_id, 1);
        assert!((points[0] - tdb_pos.position).length() > 100.0);
    }

    #[test]
    fn parse_signal_tr_item_id_from_sig_prefix() {
        assert_eq!(parse_signal_tr_item_id("sig39"), Some(39));
        assert_eq!(parse_signal_tr_item_id("bad"), None);
    }

    #[test]
    fn tr_item_pose_matches_snap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let msts = tr_item_msts_world(&tdb, 1, None).expect("tr item pose");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let host = tdb.host_vector_for_item(1).expect("host");
        let item = tdb.item_by_id(1).expect("item");
        let pose = resolver
            .tdb_pose(host, item.distance_m, None)
            .expect("tdb pose");
        assert!((msts - pose.position).length() < 0.01);
    }

    #[test]
    fn tdb_id_max_delta_is_tight() {
        const {
            assert!(TDB_ID_MAX_DELTA_M <= 25.0);
        }
        assert!(tdb_snap_radius_m() >= TDB_ID_MAX_DELTA_M);
    }
}

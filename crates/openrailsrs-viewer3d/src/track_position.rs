//! Track position on the logical graph and MSTS `.tdb` centreline.
//!
//! Spec reference: TSRE5 `getDrawPositionOnTrNode`, OR `FindLocationInSection`.
//!
//! Visual graph↔TDB correspondence must be spatially validated: a raw `nNNNN` /
//! import alias ID is accepted only when the TDB pose lies within
//! [`TDB_ID_MAX_DELTA_M`] of the **absolute** graph position (before
//! [`RouteWorldOffset`]). The offset is re-applied to the accepted pose so
//! markers stay in the same placement frame as the trimmed graph (Chiltern
//! `world_anchor`). Otherwise nearest-centreline snap or graph fallback is used
//! (simulation odometry stays on the graph).
//!
//! Rolling stock (#67) uses [`vehicle_pose_on_graph_edge`]: imported `e{N}` edges
//! map to TDB vector `N` with `pos_on_edge_m` as chainage. That path **keeps**
//! TDB elevation (no `ground_y_at`) and applies full yaw/pitch/roll when the
//! pose resolves.

use std::collections::HashMap;

use bevy::prelude::*;
use openrailsrs_bevy_scenery::spawn::tdb_track::{
    TrackPose, bevy_track_quat, nearest_track_position, tdb_node_track_pose,
};
use openrailsrs_formats::{
    RouteStart, TSectionCatalog, TrackDbFile, msts_tile_x_index_for_coord,
    msts_tile_z_index_for_coord,
};
use openrailsrs_scenarios::ScenarioFile;
use openrailsrs_sim::path::resolve_scenario_route_edges;
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

/// Reject `e{N}`→vector mapping only when endpoints disagree by more than this (m).
/// Mid-edge chord vs centreline can be far on long MSTS vectors; do not use
/// [`TDB_ID_MAX_DELTA_M`] for vehicle chainage.
pub const TDB_EDGE_ENDPOINT_MAX_DELTA_M: f32 = 250.0;

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
    let _ = (terrain, scene); // retained for API; graph fallback already has terrain Y
    if let Some(res) = resolver {
        if let Some(pose) = snap_msts_to_tdb(res, msts_hint, tdb_snap_radius_m()) {
            // Keep TDB Y — do not flatten with ground_y_at (#65 / #67).
            return focus.to_render_surface(pose.position);
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

    /// Parse import edge id `eNNNN` → TDB vector node id.
    pub fn parse_e_prefix_tdb_id(edge_id: &str) -> Option<u32> {
        edge_id.trim().strip_prefix('e')?.parse().ok()
    }

    /// Candidate TDB id from import alias, else `nNNNN` prefix.
    pub fn candidate_tdb_id(&self, node_id: &str) -> Option<u32> {
        self.graph_node_to_tdb
            .get(node_id)
            .copied()
            .or_else(|| Self::parse_n_prefix_tdb_id(node_id))
    }

    /// Resolve a graph node to a visual TDB pose with spatial ID validation.
    ///
    /// `graph_hint` is in the **placement frame** (graph + `route_offset`).
    /// ID acceptance compares `graph_hint - route_offset` to the absolute TDB
    /// pose; on success the returned pose is shifted by `route_offset` so it
    /// stays aligned with the trimmed graph / `world_anchor` scenery.
    pub fn resolve_graph_node_visual(
        &self,
        node_id: &str,
        chainage_m: f64,
        graph_hint: Option<Vec3>,
        snap_radius_m: f32,
        route_offset: Vec3,
    ) -> GraphTdbResolution {
        let mut id_delta_m = None;
        let mut rejected_tdb_id = None;
        let absolute_hint = graph_hint.map(|h| h - route_offset);

        if let Some(id) = self.candidate_tdb_id(node_id) {
            if let Some(pose) = self.tdb_pose(id, chainage_m, absolute_hint) {
                match absolute_hint {
                    Some(hint) => {
                        let delta =
                            Vec2::new(hint.x - pose.position.x, hint.z - pose.position.z).length();
                        id_delta_m = Some(delta);
                        if delta <= TDB_ID_MAX_DELTA_M {
                            let mut placed = pose;
                            placed.position += route_offset;
                            return GraphTdbResolution {
                                method: GraphTdbMethod::IdValidated,
                                tdb_node_id: Some(id),
                                pose: Some(placed),
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
    let path_edges = resolve_scenario_route_edges(&scene.graph, &scenario.route).ok()?;
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
    let resolved = resolver.resolve_graph_node_visual(
        node_id,
        chainage_m,
        graph_world,
        tdb_snap_radius_m(),
        offset.delta,
    );
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
    ///
    /// When a TDB pose exists, keeps its absolute Y (#65 / #67). Graph fallback still
    /// samples terrain via [`ground_y_at`].
    pub fn marker_render_world(
        &self,
        terrain: Option<&TerrainElevation>,
        scene: &TrackScene,
        focus: &RouteFocus,
        graph_fallback: Option<Vec3>,
    ) -> Option<Vec3> {
        if let Some(pose) = self.tdb_pose {
            return Some(focus.to_render_surface(pose.position));
        }
        let mut world = graph_fallback.or(self.graph_world)?;
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
    _terrain: Option<&TerrainElevation>,
    focus: &RouteFocus,
    graph_fallback: Option<Vec3>,
) -> Option<Vec3> {
    if let Some(res) = resolver {
        let near = graph_node_world(scene, offset, node_id).or(graph_fallback);
        let resolved = res.resolve_graph_node_visual(
            node_id,
            chainage_m,
            near,
            tdb_snap_radius_m(),
            offset.delta,
        );
        if let Some(pose) = resolved.pose {
            // Keep TDB Y — do not flatten with ground_y_at (#65 / #67).
            return Some(focus.to_render_surface(pose.position));
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

/// Planar graph point on an edge (Bevy XZ, Y=0) without terrain or route offset.
fn graph_edge_planar_msts(graph: &TrackGraph, edge_id: &str, pos_on_edge_m: f64) -> Option<Vec3> {
    let edge = graph.edge(edge_id.trim())?;
    let from = graph.node(&edge.from.0)?;
    let to = graph.node(&edge.to.0)?;
    let frac = if edge.length_m > 0.0 {
        (pos_on_edge_m / edge.length_m).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x_m = from.x_m + frac * (to.x_m - from.x_m);
    let y_m = from.y_m + frac * (to.y_m - from.y_m);
    Some(graph_to_world(x_m, y_m))
}

fn xz_delta(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Map graph `pos_on_edge_m` to TDB chainage, flipping when the edge is stored reverse of the vector.
fn tdb_chainage_for_graph_edge(
    resolver: &TrackPositionResolver<'_>,
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    tdb_node_id: u32,
) -> Option<f64> {
    let edge = graph.edge(edge_id.trim())?;
    let len = edge.length_m.max(0.0);
    let pos = pos_on_edge_m.clamp(0.0, len);
    let from = graph_edge_planar_msts(graph, edge_id, 0.0)?;
    let to = graph_edge_planar_msts(graph, edge_id, len)?;
    // Prefer start/end samples; if the vector is shorter than `len`, clamp samples still work.
    let pose0 = resolver.tdb_pose(tdb_node_id, 0.0, Some(from))?;
    let pose_end = resolver
        .tdb_pose(tdb_node_id, len, Some(to))
        .or_else(|| resolver.tdb_pose(tdb_node_id, 0.0, Some(to)))?;
    let forward = xz_delta(from, pose0.position) + xz_delta(to, pose_end.position);
    let reverse = xz_delta(from, pose_end.position) + xz_delta(to, pose0.position);
    // Soft check: imported `e{N}` is authoritative; only skip reverse detection when
    // endpoints are absurdly far (wrong id). Still return forward chainage.
    if forward.min(reverse) > TDB_EDGE_ENDPOINT_MAX_DELTA_M * 2.0 {
        return Some(pos);
    }
    if reverse + 1.0 < forward {
        Some((len - pos).clamp(0.0, len))
    } else {
        Some(pos)
    }
}

/// Bevy vehicle orientation from a TDB [`TrackPose`] (#67).
///
/// Yaw uses the established vehicle convention (`−yaw_deg` vs track ribbon);
/// pitch/roll follow [`bevy_track_quat`] (OR `CreateFromYawPitchRoll`).
pub fn vehicle_rotation_from_track_pose(pose: &TrackPose) -> Quat {
    bevy_track_quat(
        -f64::from(pose.yaw_deg),
        f64::from(pose.pitch_rad),
        f64::from(pose.roll_rad),
    )
}

/// Render-space vehicle pose on a graph edge (#67).
///
/// Prefer TDB centreline position (including elevation) and full orientation
/// when the edge is an imported `e{N}` vector. Falls back to nearest snap, then
/// graph+terrain (yaw-only).
#[allow(clippy::too_many_arguments)]
pub fn vehicle_pose_on_graph_edge(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    route_offset: Vec3,
    focus: &RouteFocus,
    terrain: Option<&TerrainElevation>,
) -> Option<(Vec3, Quat)> {
    if let Some(res) = resolver {
        if let Some(tdb_id) = TrackPositionResolver::parse_e_prefix_tdb_id(edge_id) {
            if let Some(chainage) =
                tdb_chainage_for_graph_edge(res, graph, edge_id, pos_on_edge_m, tdb_id)
            {
                let near = graph_edge_planar_msts(graph, edge_id, pos_on_edge_m);
                if let Some(pose) = res.tdb_pose(tdb_id, chainage, near) {
                    let placed = pose.position + route_offset;
                    // Keep TDB Y — do not flatten with ground_y_at (#67).
                    return Some((
                        focus.to_render_surface(placed),
                        vehicle_rotation_from_track_pose(&pose),
                    ));
                }
            }
        }
        if let Some(planar) = graph_edge_planar_msts(graph, edge_id, pos_on_edge_m) {
            let hint = planar + route_offset;
            if let Some(pose) = snap_msts_to_tdb(res, hint, tdb_snap_radius_m()) {
                return Some((
                    focus.to_render_surface(pose.position),
                    vehicle_rotation_from_track_pose(&pose),
                ));
            }
        }
    }
    let (pos, yaw) = crate::train::position_on_graph(
        graph,
        edge_id,
        pos_on_edge_m,
        terrain,
        scene,
        route_offset,
        focus,
    )?;
    Some((pos, Quat::from_rotation_y(yaw)))
}

/// Same as [`vehicle_pose_on_graph_edge`] but returns yaw in radians (around +Y).
#[allow(clippy::too_many_arguments)]
pub fn vehicle_position_yaw_on_graph_edge(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    resolver: Option<&TrackPositionResolver<'_>>,
    scene: &TrackScene,
    route_offset: Vec3,
    focus: &RouteFocus,
    terrain: Option<&TerrainElevation>,
) -> Option<(Vec3, f32)> {
    let (pos, rot) = vehicle_pose_on_graph_edge(
        graph,
        edge_id,
        pos_on_edge_m,
        resolver,
        scene,
        route_offset,
        focus,
        terrain,
    )?;
    let (yaw, _, _) = rot.to_euler(EulerRot::YXZ);
    Some((pos, yaw))
}

/// Move `delta_m` along the directed graph from `(edge_id, pos_on_edge_m)`.
///
/// Positive distance follows the edge toward `to` and continues on an outgoing
/// edge that does not U-turn; negative walks toward `from` via an incoming edge.
/// Used for bogie track samples (#69) when a full path odometer is unavailable.
pub fn advance_along_graph(
    graph: &TrackGraph,
    edge_id: &str,
    pos_on_edge_m: f64,
    delta_m: f64,
) -> Option<(String, f64)> {
    const MAX_HOPS: usize = 48;
    let mut eid = edge_id.trim().to_string();
    let mut pos = pos_on_edge_m;
    let mut remaining = delta_m;
    for _ in 0..MAX_HOPS {
        let edge = graph.edge(&eid)?;
        let len = edge.length_m.max(0.0);
        pos = pos.clamp(0.0, len);
        if remaining >= 0.0 {
            let room = len - pos;
            if remaining <= room + 1e-9 {
                return Some((eid, (pos + remaining).clamp(0.0, len)));
            }
            remaining -= room;
            let to = edge.to.0.as_str();
            let from = edge.from.0.as_str();
            let next = graph
                .outgoing_edges(to)
                .iter()
                .find(|cand| {
                    graph
                        .edge(cand)
                        .is_some_and(|e| e.to.0.as_str() != from)
                })
                .cloned()
                .or_else(|| graph.outgoing_edges(to).first().cloned())?;
            eid = next;
            pos = 0.0;
        } else {
            if pos + remaining >= -1e-9 {
                return Some((eid, (pos + remaining).clamp(0.0, len)));
            }
            remaining += pos;
            let from = edge.from.0.as_str();
            let to = edge.to.0.as_str();
            // Prefer the geometric reverse (to→from), else any edge ending at `from`.
            let prev = graph
                .edges_iter()
                .find(|(id, e)| {
                    e.to.0.as_str() == from
                        && e.from.0.as_str() == to
                        && *id != eid.as_str()
                })
                .map(|(id, _)| id.to_string())
                .or_else(|| {
                    graph
                        .edges_iter()
                        .find(|(id, e)| e.to.0.as_str() == from && *id != eid.as_str())
                        .map(|(id, _)| id.to_string())
                })?;
            let prev_len = graph.edge(&prev)?.length_m.max(0.0);
            eid = prev;
            pos = prev_len;
        }
    }
    None
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
    route_offset: Vec3,
) -> (Vec3, GraphTdbResolution) {
    if let Some(node_id) = &wp.node_id {
        let resolved = resolver.resolve_graph_node_visual(
            node_id,
            0.0,
            Some(wp.msts_hint),
            snap_radius_m,
            route_offset,
        );
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
    route_offset: Vec3,
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
        let (snapped, resolved) = snap_waypoint_msts(&wp, resolver, snap_radius_m, route_offset);
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
    let path_edges = resolve_scenario_route_edges(&scene.graph, &scenario.route)
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
    let stats = snap_corridor_path_to_tdb(
        &mut points,
        &node_ids,
        &resolver,
        tdb_snap_radius_m(),
        route_delta,
    );
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
        let resolved = resolver.resolve_graph_node_visual(
            "n2",
            0.0,
            Some(far_hint),
            tdb_snap_radius_m(),
            Vec3::ZERO,
        );
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
        let resolved = resolver.resolve_graph_node_visual(
            "n2",
            0.0,
            Some(near_hint),
            tdb_snap_radius_m(),
            Vec3::ZERO,
        );
        assert_eq!(resolved.method, GraphTdbMethod::IdValidated);
        assert_eq!(resolved.tdb_node_id, Some(2));
        assert!(resolved.id_delta_m.unwrap_or(999.0) <= TDB_ID_MAX_DELTA_M);
    }

    #[test]
    fn resolve_accepts_id_when_hint_includes_world_anchor_offset() {
        // Chiltern-style: placement hint carries RouteWorldOffset (~km), but the
        // absolute graph↔TDB match is centimetres — must still IdValidate.
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
        let offset = Vec3::new(1835.0, 0.0, 12.0);
        let placement_hint = tdb_pos.position + Vec3::new(3.0, 0.0, 0.0) + offset;
        let resolved = resolver.resolve_graph_node_visual(
            "n2",
            0.0,
            Some(placement_hint),
            tdb_snap_radius_m(),
            offset,
        );
        assert_eq!(resolved.method, GraphTdbMethod::IdValidated);
        assert_eq!(resolved.tdb_node_id, Some(2));
        assert!(resolved.rejected_tdb_id.is_none());
        assert!(resolved.id_delta_m.unwrap_or(999.0) <= TDB_ID_MAX_DELTA_M);
        let placed = resolved.pose.expect("placed pose").position;
        let expected = tdb_pos.position + offset;
        assert!(
            Vec2::new(placed.x - expected.x, placed.z - expected.z).length() < 1.0,
            "validated pose must re-apply route offset for placement frame"
        );
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
        let stats =
            snap_corridor_path_to_tdb(&mut points, &node_ids, &resolver, 2500.0, Vec3::ZERO);
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
        let stats =
            snap_corridor_path_to_tdb(&mut points, &node_ids, &resolver, 2500.0, Vec3::ZERO);
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

    #[test]
    fn parse_e_prefix_tdb_id_from_edge() {
        assert_eq!(TrackPositionResolver::parse_e_prefix_tdb_id("e10783"), Some(10783));
        assert_eq!(TrackPositionResolver::parse_e_prefix_tdb_id("e2"), Some(2));
        assert_eq!(TrackPositionResolver::parse_e_prefix_tdb_id("n2"), None);
        // Reverse import edges must not parse as TDB vector ids.
        assert_eq!(TrackPositionResolver::parse_e_prefix_tdb_id("e17466_r"), None);
    }

    #[test]
    fn vehicle_rotation_includes_tdb_pitch_and_roll() {
        let pose = TrackPose {
            position: Vec3::new(0.0, 35.8, 0.0),
            yaw_deg: 30.0,
            pitch_rad: 0.12,
            roll_rad: -0.04,
        };
        let rot = vehicle_rotation_from_track_pose(&pose);
        let (yaw, pitch, roll) = rot.to_euler(EulerRot::YXZ);
        assert!(
            (yaw - (-30.0f32).to_radians()).abs() < 1e-3,
            "vehicle yaw convention is −TrackPose.yaw_deg, got {yaw}"
        );
        assert!(
            (pitch - 0.12).abs() < 1e-3,
            "pitch must come from TDB AX, got {pitch}"
        );
        assert!(
            (roll - 0.04).abs() < 1e-3,
            "roll must follow bevy_track_quat AZ negation, got {roll}"
        );
        let yaw_only = Quat::from_rotation_y(yaw);
        assert!(
            rot.angle_between(yaw_only) > 0.05,
            "full pose must differ from yaw-only"
        );
    }

    #[test]
    fn vehicle_pose_keeps_tdb_y_not_terrain() {
        use openrailsrs_core::{EdgeId, NodeId};
        use openrailsrs_track::{Edge, Node, NodeKind};

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let pose0 = resolver.tdb_pose(2, 0.0, None).expect("vector 2 start");
        let pose_mid = resolver.tdb_pose(2, 50.0, None).expect("vector 2 mid");
        let pose_end = resolver
            .tdb_pose(2, 100.0, None)
            .or_else(|| resolver.tdb_pose(2, 99.0, None))
            .expect("vector 2 end");

        let mut graph = TrackGraph::new();
        // Import stores Bevy Z in `y_m` already (`graph_to_world` maps y_m → Z).
        graph
            .insert_node(Node {
                id: NodeId("n_from".into()),
                x_m: pose0.position.x as f64,
                y_m: pose0.position.z as f64,
                kind: NodeKind::Plain,
            })
            .unwrap();
        graph
            .insert_node(Node {
                id: NodeId("n_to".into()),
                x_m: pose_end.position.x as f64,
                y_m: pose_end.position.z as f64,
                kind: NodeKind::Plain,
            })
            .unwrap();
        let len = f64::from(xz_delta(pose0.position, pose_end.position)).max(100.0);
        graph
            .insert_edge(Edge {
                id: EdgeId("e2".into()),
                from: NodeId("n_from".into()),
                to: NodeId("n_to".into()),
                length_m: len,
                speed_limit_mps: 30.0,
                grade_percent: 0.0,
            })
            .unwrap();

        let scene = TrackScene::from_graph(graph.clone());
        let focus = RouteFocus {
            center: Vec3::ZERO,
            // Fake terrain origin well below TDB rail so a terrain flatten would show.
            height_origin: pose_mid.position.y - 10.0,
        };
        let (render, _yaw) = vehicle_position_yaw_on_graph_edge(
            &graph,
            "e2",
            50.0,
            Some(&resolver),
            &scene,
            Vec3::ZERO,
            &focus,
            None,
        )
        .expect("vehicle pose");
        let expected_y = pose_mid.position.y - focus.height_origin;
        assert!(
            (render.y - expected_y).abs() < 0.2,
            "vehicle must keep TDB Y (got {}, expected ~{})",
            render.y,
            expected_y
        );
        // Terrain fallback would sit near height_origin → render.y ≈ 0 (+rail bias).
        assert!(
            render.y > 5.0,
            "pose must not collapse to terrain height_origin (y={})",
            render.y
        );
    }

    #[test]
    fn vehicle_pose_reapplies_route_offset() {
        use openrailsrs_core::{EdgeId, NodeId};
        use openrailsrs_track::{Edge, Node, NodeKind};

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let resolver = TrackPositionResolver::new(&tdb, None);
        let pose0 = resolver.tdb_pose(2, 0.0, None).expect("start");
        let pose_end = resolver
            .tdb_pose(2, 100.0, None)
            .or_else(|| resolver.tdb_pose(2, 99.0, None))
            .expect("end");
        let mut graph = TrackGraph::new();
        graph
            .insert_node(Node {
                id: NodeId("a".into()),
                x_m: pose0.position.x as f64,
                y_m: pose0.position.z as f64,
                kind: NodeKind::Plain,
            })
            .unwrap();
        graph
            .insert_node(Node {
                id: NodeId("b".into()),
                x_m: pose_end.position.x as f64,
                y_m: pose_end.position.z as f64,
                kind: NodeKind::Plain,
            })
            .unwrap();
        graph
            .insert_edge(Edge {
                id: EdgeId("e2".into()),
                from: NodeId("a".into()),
                to: NodeId("b".into()),
                length_m: 100.0,
                speed_limit_mps: 30.0,
                grade_percent: 0.0,
            })
            .unwrap();
        let scene = TrackScene::from_graph(graph.clone());
        let focus = test_focus();
        let offset = Vec3::new(1835.0, 0.0, 12.0);
        let (render, _) = vehicle_position_yaw_on_graph_edge(
            &graph,
            "e2",
            0.0,
            Some(&resolver),
            &scene,
            offset,
            &focus,
            None,
        )
        .expect("pose");
        let expected = focus.to_render_surface(pose0.position + offset);
        assert!(
            Vec2::new(render.x - expected.x, render.z - expected.z).length() < 1.0,
            "placement frame must include RouteWorldOffset"
        );
    }
}

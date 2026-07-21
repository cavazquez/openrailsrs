//! 3D signal markers on track edges (static aspect from track.toml).

use bevy::prelude::*;
use openrailsrs_track::{SignalAspect, TrackGraph, TrackSignal};

use crate::launch::ViewerSceneryMode;
use crate::shapes::RouteAssets;
use crate::terrain::TerrainElevation;
use crate::tr_item_audit::TR_ITEM_WORLD_MATCH_RADIUS_M;
use crate::tr_item_index::TrItemWorldIndex;
use crate::track::TrackScene;
use crate::track_position::{
    TrackPositionResolver, marker_render_world_on_edge, msts_to_render_surface,
    parse_signal_tr_item_id, tr_item_msts_world,
};
use crate::world::{RouteFocus, RouteWorldOffset};

const COLOR_SIG_STOP: Color = Color::srgb(1.0, 0.133, 0.133);
const COLOR_SIG_CAUTION: Color = Color::srgb(1.0, 0.8, 0.0);
const COLOR_SIG_CLEAR: Color = Color::srgb(0.133, 1.0, 0.333);
const COLOR_SIG_POLE: Color = Color::srgb(0.533, 0.533, 0.533);

pub fn aspect_color(aspect: SignalAspect) -> Color {
    match aspect {
        SignalAspect::Stop => COLOR_SIG_STOP,
        SignalAspect::Caution => COLOR_SIG_CAUTION,
        SignalAspect::Clear => COLOR_SIG_CLEAR,
    }
}

/// World position for a signal on its edge (graph interpolation, snapped to `.tdb` when loaded).
pub fn signal_position_on_edge(
    graph: &TrackGraph,
    signal: &TrackSignal,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    world_offset: RouteWorldOffset,
    focus: &RouteFocus,
    resolver: Option<&TrackPositionResolver<'_>>,
) -> Option<Vec3> {
    marker_render_world_on_edge(
        graph,
        &signal.edge_id,
        signal.position_m,
        resolver,
        scene,
        world_offset,
        terrain,
        focus,
    )
    .map(|(p, _)| p)
}

/// Render position for a signal marker: TDB `TrItem` pose when available, else graph+snap.
/// Returns `None` when a `.w` Signal mesh already covers this `TrItem`.
#[allow(clippy::too_many_arguments)]
pub fn signal_render_world(
    graph: &TrackGraph,
    signal: &TrackSignal,
    assets: &RouteAssets,
    terrain: Option<&TerrainElevation>,
    scene: &TrackScene,
    world_offset: RouteWorldOffset,
    focus: &RouteFocus,
    world_index: Option<&TrItemWorldIndex>,
) -> Option<Vec3> {
    if let Some(tdb) = assets.track_db() {
        let tsection = Some(assets.tsection());
        if let Some(item_id) = parse_signal_tr_item_id(&signal.id) {
            if let Some(msts) = tr_item_msts_world(tdb, item_id, tsection) {
                if world_index.is_some_and(|idx| {
                    idx.has_world_object_near(item_id, msts, TR_ITEM_WORLD_MATCH_RADIUS_M)
                }) {
                    return None;
                }
                return Some(msts_to_render_surface(msts, terrain, scene, focus));
            }
        }
        let resolver = TrackPositionResolver::from_track_scene(tdb, tsection, scene);
        return signal_position_on_edge(
            graph,
            signal,
            terrain,
            scene,
            world_offset,
            focus,
            Some(&resolver),
        );
    }
    signal_position_on_edge(graph, signal, terrain, scene, world_offset, focus, None)
}

/// Spawn diamond markers and poles for all signals in the graph.
#[allow(clippy::too_many_arguments)]
pub fn spawn_signal_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<TrackScene>,
    assets: Res<RouteAssets>,
    offset: Res<RouteWorldOffset>,
    focus: Res<RouteFocus>,
    terrain: Option<Res<TerrainElevation>>,
    mode: Res<ViewerSceneryMode>,
    world_index: Option<Res<TrItemWorldIndex>>,
) {
    if mode.is_track_focused() {
        return;
    }
    let signal_count = scene.graph.signals().count();
    if signal_count == 0 {
        return;
    }

    let terrain_ref = terrain.as_deref();
    let index_ref = world_index.as_deref();
    let diamond_size = scene.bounds.edge_radius().max(1.5) * 1.2;
    let pole_radius = diamond_size * 0.15;
    let pole_height = diamond_size * 2.5;

    let diamond_mesh = meshes.add(Cuboid::new(diamond_size, diamond_size, diamond_size));
    let pole_mesh = meshes.add(Cylinder::new(pole_radius, pole_height));

    let stop_mat = materials.add(StandardMaterial {
        base_color: COLOR_SIG_STOP,
        perceptual_roughness: 0.5,
        metallic: 0.2,
        emissive: LinearRgba::from(COLOR_SIG_STOP) * 0.4,
        ..default()
    });
    let caution_mat = materials.add(StandardMaterial {
        base_color: COLOR_SIG_CAUTION,
        perceptual_roughness: 0.5,
        metallic: 0.2,
        emissive: LinearRgba::from(COLOR_SIG_CAUTION) * 0.4,
        ..default()
    });
    let clear_mat = materials.add(StandardMaterial {
        base_color: COLOR_SIG_CLEAR,
        perceptual_roughness: 0.5,
        metallic: 0.2,
        emissive: LinearRgba::from(COLOR_SIG_CLEAR) * 0.4,
        ..default()
    });
    let pole_material = materials.add(StandardMaterial {
        base_color: COLOR_SIG_POLE,
        perceptual_roughness: 0.9,
        metallic: 0.05,
        ..default()
    });

    let aspect_mat = |aspect: &SignalAspect| -> Handle<StandardMaterial> {
        match aspect {
            SignalAspect::Stop => stop_mat.clone(),
            SignalAspect::Caution => caution_mat.clone(),
            SignalAspect::Clear => clear_mat.clone(),
        }
    };

    for signal in scene.graph.signals() {
        let Some(pos) = signal_render_world(
            &scene.graph,
            signal,
            &assets,
            terrain_ref,
            &scene,
            *offset,
            &focus,
            index_ref,
        ) else {
            continue;
        };
        let material = aspect_mat(&signal.aspect);

        let pole_y = pos.y + pole_height * 0.5;
        commands.spawn((
            Mesh3d(pole_mesh.clone()),
            MeshMaterial3d(pole_material.clone()),
            Transform::from_translation(Vec3::new(pos.x, pole_y, pos.z)),
            Name::new(format!("signal-pole:{}", signal.id)),
        ));

        commands.spawn((
            SignalMarker {
                id: signal.id.clone(),
            },
            Mesh3d(diamond_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(pos)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_4)),
            Name::new(format!("signal:{}", signal.id)),
        ));
    }
}

/// Diamond mesh for a track signal; aspect is updated in live mode from [`LiveDriveSession`].
#[derive(Component)]
pub struct SignalMarker {
    pub id: String,
}

/// Refresh signal colours from the live sim's `signal_runtime` map.
pub fn update_live_signal_markers(
    live: Option<Res<crate::live::LiveDrive>>,
    scene: Res<TrackScene>,
    query: Query<(&SignalMarker, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(live) = live else {
        return;
    };
    if live.session.assume_signals_clear {
        return;
    }
    for (marker, mat_handle) in &query {
        let aspect = live
            .session
            .signal_aspect(&marker.id)
            .or_else(|| scene.graph.signal(&marker.id).map(|s| s.aspect))
            .unwrap_or(SignalAspect::Stop);
        let color = aspect_color(aspect);
        if let Some(mut mat) = materials.get_mut(mat_handle) {
            mat.base_color = color;
            mat.emissive = LinearRgba::from(color) * 0.4;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_core::{EdgeId, NodeId};
    use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph, TrackSignal};

    fn line_graph_with_signal() -> (TrackGraph, TrackSignal) {
        let mut g = TrackGraph::new();
        g.insert_node(Node {
            id: NodeId("a".into()),
            kind: NodeKind::Plain,
            x_m: 0.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_node(Node {
            id: NodeId("b".into()),
            kind: NodeKind::Plain,
            x_m: 100.0,
            y_m: 0.0,
        })
        .unwrap();
        g.insert_edge(Edge {
            id: EdgeId("e1".into()),
            from: NodeId("a".into()),
            to: NodeId("b".into()),
            length_m: 100.0,
            speed_limit_mps: 30.0,
            grade_percent: 0.0,
        })
        .unwrap();
        let sig = TrackSignal {
            id: "sig1".into(),
            edge_id: "e1".into(),
            position_m: 50.0,
            aspect: SignalAspect::Caution,
            clear_after_s: None,
            script: None,
        };
        g.insert_signal(sig.clone()).unwrap();
        (g, sig)
    }

    #[test]
    fn signal_uses_tdb_pose_when_resolver() {
        use crate::track_position::{parse_signal_tr_item_id, tr_item_msts_world};
        use openrailsrs_formats::TrackDbFile;

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-msts/tests/fixtures/native_msts.tdb");
        if !path.is_file() {
            return;
        }
        let tdb = TrackDbFile::from_path(&path).expect("tdb");
        let item_id = parse_signal_tr_item_id("sig1").expect("sig1 → TrItem 1");
        let msts = tr_item_msts_world(&tdb, item_id, None).expect("TrItem pose");
        let (g, sig) = line_graph_with_signal();
        let scene = TrackScene::from_graph(g.clone());
        let focus = RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let graph_pos = signal_position_on_edge(
            &g,
            &sig,
            None,
            &scene,
            RouteWorldOffset::default(),
            &focus,
            None,
        )
        .unwrap();
        assert!(
            (msts.x - graph_pos.x).abs() > 1.0 || (msts.z - graph_pos.z).abs() > 1.0,
            "TrItem pose should differ from patched-graph interpolation"
        );
    }

    #[test]
    fn signal_at_mid_edge() {
        let (g, sig) = line_graph_with_signal();
        let scene = TrackScene::from_graph(g.clone());
        let focus = RouteFocus {
            center: Vec3::ZERO,
            height_origin: 0.0,
        };
        let pos = signal_position_on_edge(
            &g,
            &sig,
            None,
            &scene,
            RouteWorldOffset::default(),
            &focus,
            None,
        )
        .unwrap();
        assert!((pos.x - 50.0).abs() < 1e-3);
        assert!(pos.y > 0.0);
    }
}

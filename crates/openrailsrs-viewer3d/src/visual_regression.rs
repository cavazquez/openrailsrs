//! Structural visual-regression checks without GPU (#43).

#![cfg(test)]

use crate::test_harness::{load_smoke_route_bundle, smoke_route_dir};
use crate::track::TrackScene;
use crate::world::WorldScene;

#[test]
fn smoke_route_structural_metrics() {
    let route_dir = smoke_route_dir();
    assert!(
        route_dir.join("track.toml").exists(),
        "smoke fixture missing track.toml"
    );

    let Some((track, world, terrain, _elev, _focus, _offset, _consist, _assets)) =
        load_smoke_route_bundle()
    else {
        panic!("failed to load smoke route bundle");
    };

    assert_track_finite(&track);
    assert_world_structural(&world);
    assert!(
        terrain.tiles_loaded >= 1 || world.tiles_loaded >= 1,
        "expected terrain tiles or at least one WORLD tile"
    );
}

fn assert_track_finite(track: &TrackScene) {
    let b = &track.bounds;
    assert!(
        b.center.is_finite() && b.half_extent.is_finite() && b.half_extent >= 0.0,
        "track bounds must be finite: center={:?} half={}",
        b.center,
        b.half_extent
    );
    assert!(
        track.edge_count >= 1 && track.graph.edges_iter().count() >= 1,
        "smoke track must have ≥1 edge"
    );
    assert!(
        track.graph.nodes_iter().count() >= 2,
        "smoke track must have ≥2 nodes"
    );
}

fn assert_world_structural(world: &WorldScene) {
    assert!(
        world.tiles_loaded >= 1 && !world.items.is_empty(),
        "smoke WORLD must load ≥1 tile with objects (tiles={} items={})",
        world.tiles_loaded,
        world.items.len()
    );

    let mut kinds = std::collections::HashSet::new();
    for obj in &world.items {
        assert!(
            obj.position.is_finite(),
            "world object {} has non-finite position {:?}",
            obj.label,
            obj.position
        );
        assert!(
            obj.rotation.is_finite() && obj.scale.is_finite(),
            "world object {} has non-finite transform",
            obj.label
        );
        kinds.insert(obj.kind);
    }

    for required in ["Static", "TrackObj", "Forest", "Signal", "Transfer"] {
        assert!(
            kinds.contains(required),
            "smoke WORLD missing required kind {required}; have {kinds:?}"
        );
    }

    if let Some(center) = world.position_center() {
        assert!(
            center.is_finite(),
            "world position_center must be finite: {center:?}"
        );
    }
}

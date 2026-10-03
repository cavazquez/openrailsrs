//! MSTS dynamic track segments from `.w` `Dyntrack` items (order 9 / issue #8).
//!
//! Geometry lives in [`openrailsrs_bevy_scenery::spawn::dyntrack`]; this module
//! wires viewer-specific WORLD spawn and tile streaming.

use bevy::prelude::*;

use crate::track::{SceneBounds, TrackScene};
use crate::viewer_log;
use crate::world::WorldScene;

pub use openrailsrs_bevy_scenery::spawn::dyntrack::{
    DyntrackDimensions, MSTS_DEFAULT_SECTION_LENGTH_M, MSTS_STANDARD_HALF_GAUGE_M,
    ProceduralTrackSegment, ProceduralTrackStyle, append_procedural_track_segment, arc_local_frame,
    dyntrack_dimensions_from_edge_radius, msts_track_visual_dims, part_transform,
    procedural_segment_visual_dims, segment_end_world, sleeper_local_z_positions,
    sleeper_path_distances, spawn_procedural_track_batch,
};

/// Match the `base` used by [`crate::world::spawn_world_boxes`] so dyntrack is visible
/// next to scaled graph cylinders and world cuboids.
pub fn dyntrack_dimensions(bounds: &SceneBounds) -> DyntrackDimensions {
    dyntrack_dimensions_from_edge_radius(bounds.edge_radius())
}

/// Spawn all dyntrack geometry as two merged meshes (sleepers + rails).
pub fn spawn_dyntrack_segments(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: Res<WorldScene>,
    focus: Res<crate::world::RouteFocus>,
    wire: Res<crate::overhead_wire::RouteWireConfig>,
) {
    spawn_dyntrack_objects_with_wire(
        &mut commands,
        &mut meshes,
        &mut materials,
        &world.items,
        &focus,
        Some(&wire),
        &crate::floating_origin::FloatingOrigin::default(),
    );
}

/// Spawn dyntrack for a slice of world objects (tile streaming).
pub fn spawn_dyntrack_objects(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    items: &[crate::world::WorldObject],
    _track: &TrackScene,
    focus: &crate::world::RouteFocus,
) {
    spawn_dyntrack_objects_with_wire(
        commands,
        meshes,
        materials,
        items,
        focus,
        None,
        &crate::floating_origin::FloatingOrigin::default(),
    );
}

/// Same as [`spawn_dyntrack_objects`], optionally drawing overhead wire (#36).
pub fn spawn_dyntrack_objects_with_wire(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    items: &[crate::world::WorldObject],
    focus: &crate::world::RouteFocus,
    wire: Option<&crate::overhead_wire::RouteWireConfig>,
    origin: &crate::floating_origin::FloatingOrigin,
) {
    // Tile groups own their meshes so leaving a tile releases both rails and
    // sleepers. A single route-wide batch cannot be unloaded independently.
    let mut by_tile = std::collections::HashMap::<_, Vec<&crate::world::WorldObject>>::new();
    for obj in items.iter().filter(|obj| obj.kind == "Dyntrack") {
        by_tile
            .entry((obj.tile_x, obj.tile_z))
            .or_default()
            .push(obj);
    }
    let mut segment_count = 0;
    for ((tile_x, tile_z), items) in by_tile {
        let bound = crate::world::WorldTileBound::new(tile_x, tile_z);
        let segments: Vec<_> = items.iter().flat_map(|obj| {
            openrailsrs_bevy_scenery::spawn::dyntrack::procedural_segments_from_dyntrack_sections(
                crate::floating_origin::view_translation(obj.render_position(focus), origin),
                obj.rotation, &obj.dyntrack_sections,
            )
        }).collect();
        segment_count += segments.len();
        let entities = spawn_procedural_track_batch(
            commands,
            meshes,
            materials,
            &segments,
            "dyntrack",
            ProceduralTrackStyle::Full,
        );
        for entity in entities {
            commands.entity(entity).insert(bound);
        }
        if let Some(wire) = wire.filter(|w| w.enabled) {
            let wire_segs: Vec<ProceduralTrackSegment> = items
                .iter()
                .filter(|obj| {
                    obj.kind == "Dyntrack"
                        && !crate::overhead_wire::is_hide_wire_detail_level(obj.static_detail_level)
                })
                .map(|obj| ProceduralTrackSegment {
                    position: crate::floating_origin::view_translation(
                        obj.render_position(focus),
                        origin,
                    ),
                    rotation: obj.rotation,
                    length_m: Some(MSTS_DEFAULT_SECTION_LENGTH_M),
                    half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
                    curve_radius_m: None,
                    curve_angle_deg: None,
                })
                .collect();
            if !wire_segs.is_empty()
                && let Some(entity) = crate::overhead_wire::spawn_overhead_wire_batch(
                    commands, meshes, materials, &wire_segs, wire.style, "dyntrack",
                )
            {
                commands.entity(entity).insert(bound);
            }
        }
    }
    viewer_log!(
        "openrailsrs-viewer3d: {} dyntrack segment(s)",
        segment_count
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::world::load_world_from_route_dir;

    #[test]
    fn dimensions_scale_with_route_bounds() {
        let small = dyntrack_dimensions(&SceneBounds::default_sandbox());
        assert!(small.length > 5.0);

        let large = dyntrack_dimensions(&SceneBounds {
            half_extent: 5_000.0,
            ..SceneBounds::default_sandbox()
        });
        assert!(large.length > small.length);
        assert!(large.half_gauge > small.half_gauge);
    }

    #[test]
    fn smoke_route_has_dyntrack_on_e1() {
        let route_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke/routes/test");
        let scene = load_world_from_route_dir(&route_dir);
        let dyntrack = scene
            .items
            .iter()
            .find(|o| o.kind == "Dyntrack")
            .expect("dyntrack");
        assert!((dyntrack.position.x - 80.0).abs() < 0.1);
        assert!((dyntrack.position.z - (-0.8)).abs() < 0.1);
    }
}

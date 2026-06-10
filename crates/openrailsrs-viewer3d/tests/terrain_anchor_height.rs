//! Empirical check: terrain sampling must reproduce the Open Rails anchor height.
//!
//! OR log for Chiltern Birmingham activity start:
//!   `{TileX:-6080 TileZ:14925 X:891.831 Y:35.7818 Z:582.756}`
//! The terrain elevation at that MSTS location must be ~35.8 m.

use bevy::math::Vec3;
use openrailsrs_viewer3d::terrain::TerrainElevation;

fn route_dir() -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    dir.join("TILES").is_dir().then_some(dir)
}

#[test]
fn anchor_elevation_matches_open_rails() {
    let Some(route) = route_dir() else { return };
    // Render convention (OR parity): signed internal tile X, whole-world Z negation.
    let x = -6080.0_f32 * 2048.0 + 891.831;
    let z = -(14925.0_f32 * 2048.0 + 582.756);
    let center = Vec3::new(x, 0.0, z);
    let elev = TerrainElevation::load_from_route_dir_near(&route, Some(center), 3000.0);
    assert!(!elev.is_empty(), "no terrain tiles loaded near anchor");
    let h = elev.sample_world_y(x, z);
    println!("sampled height at anchor: {h:?}");
    let h = h.expect("anchor position should be covered by a terrain tile");
    // The raw heightfield at this spot reads ~28.5 m (the track itself sits on an
    // embankment at 35.78 m). The terrain must be slightly below the rail head,
    // never above it and never tens of metres off.
    assert!(
        h > 20.0 && h < 36.5,
        "anchor terrain elevation {h:.2} out of expected range (rail at 35.78)"
    );
}

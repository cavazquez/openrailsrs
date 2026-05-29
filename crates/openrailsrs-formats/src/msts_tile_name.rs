//! MSTS terrain tile filenames (`-11cf297c.t`) from tile coordinates.
//!
//! Ported from Open Rails `Orts.Formats.Msts.TileName.FromTileXZ` (zoom 15 = 2 km tiles).

/// MSTS zoom level for standard 2048 m route tiles.
pub const MSTS_TILE_ZOOM_SMALL: u32 = 15;

/// Build the `TILES/` filename stem (e.g. `-11cf297c`) for a tile.
///
/// `tile_x` / `tile_z` must use MSTS internal coordinates (X is typically negative on
/// UK routes while `.w` filenames use positive display values).
pub fn msts_tile_name_from_xz(tile_x: i32, tile_z: i32) -> String {
    msts_tile_name_from_xz_zoom(tile_x, tile_z, MSTS_TILE_ZOOM_SMALL)
}

pub fn msts_tile_name_from_xz_zoom(tile_x: i32, tile_z: i32, zoom: u32) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut rect_x = -16_384i32;
    let mut rect_z = -16_384i32;
    let mut rect_w = 16_384i32;
    let mut rect_h = 16_384i32;
    let mut name = String::with_capacity(12);
    if zoom % 2 == 1 {
        name.push('-');
    } else {
        name.push('_');
    }
    let mut partial = 0u32;
    for z in 0..zoom {
        let east = tile_x >= rect_x + rect_w;
        let north = tile_z >= rect_z + rect_h;
        partial =
            (partial << 2) + ((if north { 0 } else { 2 }) + (if east ^ north { 0 } else { 1 }));
        if z % 2 == 1 {
            name.push(HEX[partial as usize] as char);
            partial = 0;
        }
        if east {
            rect_x += rect_w;
        }
        if north {
            rect_z += rect_h;
        }
        rect_w /= 2;
        rect_h /= 2;
    }
    if zoom % 2 == 1 {
        name.push(HEX[(partial << 2) as usize] as char);
    }
    name
}

/// Display coords from a `.w` filename (`w-006084+014924`) → internal coords for `TILES/`.
pub fn msts_internal_tile_x_from_world_display(display_x: i32) -> i32 {
    -display_x
}

/// Internal `TILES/` X index → display/world tile X (inverse of [`msts_internal_tile_x_from_world_display`]).
pub fn msts_display_tile_x_from_internal(internal_x: i32) -> i32 {
    -internal_x
}

/// World-space metres of the south-west corner of a display tile (`.w` / `msts_to_bevy` convention).
pub fn msts_tile_world_origin(display_x: i32, display_z: i32) -> (f32, f32) {
    let tile = 2048.0_f32;
    (display_x as f32 * tile, display_z as f32 * tile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn display_internal_x_roundtrip() {
        assert_eq!(msts_display_tile_x_from_internal(-6084), 6084);
        assert_eq!(msts_internal_tile_x_from_world_display(6084), -6084);
    }

    #[test]
    fn chiltern_hash_matches_open_rails() {
        assert_eq!(
            msts_tile_name_from_xz(-6084, 14924).to_ascii_lowercase(),
            "-11cf297c"
        );
        assert_eq!(
            msts_tile_name_from_xz(-6080, 14925).to_ascii_lowercase(),
            "-11cf7c30"
        );
    }

    #[test]
    fn chiltern_hashes_exist_when_tiles_copied() {
        let tiles = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern/TILES");
        if !tiles.is_dir() {
            return;
        }
        let name = msts_tile_name_from_xz(-6084, 14924).to_ascii_lowercase();
        assert!(
            tiles.join(format!("{name}.t")).is_file(),
            "expected {name}.t under Chiltern TILES"
        );
    }
}

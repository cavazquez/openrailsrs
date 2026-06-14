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

/// Render-space metres of the minimum corner of a tile (Bevy/XNA axes).
///
/// `tile_x` / `tile_z` use MSTS internal (signed) coordinates.  Following Open
/// Rails, render space is `x = msts_x`, `z = -msts_z` (whole-world Z negation),
/// and a tile is centred on `tile * 2048` in MSTS space spanning ±1024 m:
/// - render X span: `[tile_x*2048 - 1024, tile_x*2048 + 1024)`
/// - render Z span: `[-tile_z*2048 - 1024, -tile_z*2048 + 1024)`
pub fn msts_tile_world_origin(tile_x: i32, tile_z: i32) -> (f32, f32) {
    let tile = 2048.0_f32;
    let half = 1024.0_f32;
    (tile_x as f32 * tile - half, -(tile_z as f32) * tile - half)
}

/// Internal (signed) tile X index containing render-space X coordinate (metres).
pub fn msts_tile_x_index_for_coord(x: f32) -> i32 {
    ((x + 1024.0) / 2048.0).floor() as i32
}

/// Internal (signed) tile Z index containing render-space Z coordinate (metres).
/// Render Z is the negated MSTS world Z, so the index search is mirrored.
pub fn msts_tile_z_index_for_coord(z: f32) -> i32 {
    ((-z + 1024.0) / 2048.0).floor() as i32
}

/// Signed tile coords from `WORLD/w-006074+014924.w` filenames.
///
/// Matches Open Rails `WorldFile`: the sign characters are part of the value
/// (`w-006084+014923.w` → `(-6084, 14923)`).
pub fn parse_world_w_tile_xz(path: &std::path::Path) -> Option<(i32, i32)> {
    let stem = path.file_stem()?.to_str()?;
    let rest = stem.strip_prefix(['w', 'W'])?;
    if rest.len() < 14 {
        return None;
    }
    let (x_part, z_part) = rest.split_at(7);
    let parse = |s: &str| -> Option<i32> {
        let (sign, digits) = s.split_at(1);
        let v: i32 = digits.parse().ok()?;
        match sign {
            "-" => Some(-v),
            "+" | "_" => Some(v),
            _ => None,
        }
    };
    Some((parse(x_part)?, parse(&z_part[..7])?))
}

/// `.w` filename (`w-006084+014923.w`) from signed tile coords (Open Rails
/// `WorldFileNameFromTileCoordinates`).
pub fn world_w_filename_from_tile_xz(tile_x: i32, tile_z: i32) -> String {
    let fmt = |v: i32| format!("{}{:06}", if v < 0 { '-' } else { '+' }, v.abs());
    format!("w{}{}.w", fmt(tile_x), fmt(tile_z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn tile_origin_is_centre_minus_half() {
        let (ox, oz) = msts_tile_world_origin(-6084, 14923);
        assert_eq!(ox, -6084.0 * 2048.0 - 1024.0);
        assert_eq!(oz, -14923.0 * 2048.0 - 1024.0);
    }

    #[test]
    fn tile_index_rounds_to_nearest_centre() {
        assert_eq!(msts_tile_x_index_for_coord(0.0), 0);
        assert_eq!(msts_tile_x_index_for_coord(1023.9), 0);
        assert_eq!(msts_tile_x_index_for_coord(1024.1), 1);
        assert_eq!(msts_tile_x_index_for_coord(-1023.9), 0);
        assert_eq!(msts_tile_x_index_for_coord(-1024.1), -1);
        // Render Z is negated MSTS Z: tile 1 is centred on render z = -2048.
        assert_eq!(msts_tile_z_index_for_coord(-2048.0), 1);
        assert_eq!(msts_tile_z_index_for_coord(2048.0), -1);
        assert_eq!(msts_tile_z_index_for_coord(0.0), 0);
    }

    #[test]
    fn tile_origin_and_index_roundtrip() {
        for (tx, tz) in [(-6084, 14923), (0, 0), (7, -3)] {
            let (ox, oz) = msts_tile_world_origin(tx, tz);
            // Interior points map back to the same tile (offsets > f32 ulp at ~30 Mm).
            assert_eq!(msts_tile_x_index_for_coord(ox + 8.0), tx);
            assert_eq!(msts_tile_x_index_for_coord(ox + 2040.0), tx);
            assert_eq!(msts_tile_z_index_for_coord(oz + 8.0), tz);
            assert_eq!(msts_tile_z_index_for_coord(oz + 2040.0), tz);
        }
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

    #[test]
    fn parse_world_w_filename_is_signed() {
        use std::path::Path;
        assert_eq!(
            parse_world_w_tile_xz(Path::new("w-006084+014923.w")),
            Some((-6084, 14923))
        );
        assert_eq!(
            parse_world_w_tile_xz(Path::new("w-001000-001000.w")),
            Some((-1000, -1000))
        );
        assert_eq!(
            parse_world_w_tile_xz(Path::new("w+001000+001000.w")),
            Some((1000, 1000))
        );
    }

    #[test]
    fn world_filename_roundtrip() {
        use std::path::Path;
        let name = world_w_filename_from_tile_xz(-6084, 14923);
        assert_eq!(name, "w-006084+014923.w");
        assert_eq!(
            parse_world_w_tile_xz(Path::new(&name)),
            Some((-6084, 14923))
        );
    }
}

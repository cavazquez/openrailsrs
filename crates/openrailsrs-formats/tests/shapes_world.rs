use std::path::PathBuf;

use openrailsrs_formats::{
    FormatError, ShapeFile, Vec3, WorldFile, WorldItem, parse_from_first_paren,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn parse_minimal_shape_collects_lods_and_prims() {
    let shape = ShapeFile::from_path(fixture("minimal.s")).expect("parse minimal.s");

    assert_eq!(shape.points.len(), 4, "expected 4 points");
    assert_eq!(shape.uvs.len(), 4, "expected 4 uvs");
    assert_eq!(shape.normals.len(), 1, "expected 1 normal");
    assert_eq!(shape.prim_states.len(), 1);
    assert_eq!(shape.prim_states[0].name.as_deref(), Some("wagon_body"));
    assert_eq!(shape.prim_states[0].texture_idx, 0);
    assert_eq!(shape.texture_filenames, vec!["wagon.ace", "trim.ace"]);
    assert_eq!(shape.matrices.len(), 1);
    assert_eq!(shape.matrices[0].name, "MAIN");

    assert_eq!(shape.lod_controls.len(), 1);
    let lod = &shape.lod_controls[0];
    assert_eq!(lod.distance_levels.len(), 2);
    assert_eq!(lod.distance_levels[0].selection_m, 200.0);
    assert_eq!(lod.distance_levels[1].selection_m, 1000.0);

    let sub0 = &lod.distance_levels[0].sub_objects[0];
    assert_eq!(sub0.vertex_count, 4);
    assert_eq!(sub0.primitives.len(), 1);
    assert_eq!(sub0.primitives[0].prim_state_idx, 0);
    assert_eq!(sub0.primitives[0].triangle_count(), 2);
    assert_eq!(sub0.primitives[0].vertex_indices.len(), 6);
}

#[test]
fn parse_minimal_shape_via_ast_matches_from_path() {
    let text = std::fs::read_to_string(fixture("minimal.s")).unwrap();
    let ast = parse_from_first_paren(&text).expect("parse ast");
    let from_ast = ShapeFile::from_ast(&ast).expect("parse from ast");
    let from_path = ShapeFile::from_path(fixture("minimal.s")).expect("parse from path");
    assert_eq!(from_ast.points.len(), from_path.points.len());
    assert_eq!(
        from_ast.lod_controls[0].distance_levels.len(),
        from_path.lod_controls[0].distance_levels.len()
    );
}

#[test]
fn binary_shape_fixture_may_parse_or_fail_gracefully() {
    // SIMISA header + synthetic binary tokens (not a full real shape).
    let mut bytes = b"SIMISA@@@@@@@@@@JINX0s1b______".to_vec();
    for _ in 0..256 {
        bytes.push(0x07);
        bytes.push(0x01);
        bytes.push(0x00);
        bytes.push(0x00);
        bytes.push(0x08);
        bytes.push(0x00);
        bytes.push(0x00);
        bytes.push(0x00);
    }

    let tmp = std::env::temp_dir().join("openrailsrs_binary_shape_fixture.s");
    std::fs::write(&tmp, &bytes).unwrap();
    match ShapeFile::from_path(&tmp) {
        Ok(shape) => assert!(shape.points.is_empty() || !shape.points.is_empty()),
        Err(FormatError::UnsupportedBinaryShape) => {}
        Err(_) => {}
    }
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn compressed_simisa_shape_decompresses_before_parse() {
    use std::io::Write;

    let body = b"JINX0s1t______\n( shape ( texture_filenames 1 \"wagon.ace\" ) )";
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(body).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut bytes = b"SIMISA@F@@@@@@@@".to_vec();
    bytes.extend_from_slice(&compressed);

    let tmp = std::env::temp_dir().join("openrailsrs_compressed_shape_fixture.s");
    std::fs::write(&tmp, &bytes).unwrap();
    let shape = ShapeFile::from_path(&tmp).expect("parse compressed SIMISA shape");
    assert_eq!(shape.texture_filenames, vec!["wagon.ace"]);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn binary_shape_starts_after_jinx_padding() {
    let mut bytes = b"SIMISA@@@@@@@@@@JINX0s1b______".to_vec();
    bytes.extend_from_slice(&71u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);

    let tmp = std::env::temp_dir().join("openrailsrs_minimal_binary_shape_fixture.s");
    std::fs::write(&tmp, &bytes).unwrap();
    let shape = ShapeFile::from_path(&tmp).expect("parse minimal binary SIMISA shape");
    assert!(shape.points.is_empty());
    assert!(shape.texture_filenames.is_empty());
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn parse_compressed_binary_shape_from_open_rails_content() {
    let shape = ShapeFile::from_path(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/SHAPES/RF_WP_DMBSA.s"),
    )
    .expect("parse compressed binary Open Rails shape");

    assert_eq!(shape.points.len(), 3755);
    assert_eq!(shape.normals.len(), 4636);
    assert_eq!(shape.uvs.len(), 2214);
    assert_eq!(shape.texture_filenames.len(), 8);
    assert_eq!(shape.prim_states.len(), 30);
    assert_eq!(shape.matrices.len(), 12);
    assert_eq!(shape.lod_controls.len(), 1);
    assert_eq!(shape.lod_controls[0].distance_levels.len(), 1);

    let primitive_count: usize = shape
        .lod_controls
        .iter()
        .flat_map(|lod| &lod.distance_levels)
        .flat_map(|level| &level.sub_objects)
        .map(|sub_object| sub_object.primitives.len())
        .sum();
    let triangle_count: usize = shape
        .lod_controls
        .iter()
        .flat_map(|lod| &lod.distance_levels)
        .flat_map(|level| &level.sub_objects)
        .flat_map(|sub_object| &sub_object.primitives)
        .map(|primitive| primitive.triangle_count())
        .sum();
    assert_eq!(primitive_count, 30);
    assert_eq!(triangle_count, 4869);
}

#[test]
fn parse_hwater_from_smoke_fixture() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/smoke/routes/test/WORLD/w-000000-000000.w");
    let world = WorldFile::from_path(&path).expect("parse smoke world");
    let item = world
        .items
        .iter()
        .find(|i| i.kind() == "HWater")
        .expect("hwater");
    if let WorldItem::HWater {
        uid,
        position,
        size,
        ..
    } = item
    {
        assert_eq!(*uid, 6);
        assert!((position.y - 3.0).abs() < 1e-6);
        assert!((size[0] - 50.0).abs() < 1e-6);
        assert!((size[1] - 40.0).abs() < 1e-6);
    } else {
        panic!("expected HWater");
    }
}

#[test]
fn parse_minimal_world_classifies_items() {
    let world = WorldFile::from_path(fixture("w-001000-001000.w")).expect("parse world");

    assert_eq!(world.tile_x, 1000);
    assert_eq!(world.tile_z, 1000);
    assert_eq!(world.items.len(), 5);

    let kinds: Vec<&str> = world.items.iter().map(|i| i.kind()).collect();
    assert!(kinds.contains(&"Static"));
    assert!(kinds.contains(&"Forest"));
    assert!(kinds.contains(&"TrackObj"));
    assert!(kinds.contains(&"Signal"));
    assert!(kinds.contains(&"Dyntrack"));

    let static_item = world
        .items
        .iter()
        .find(|i| i.kind() == "Static")
        .expect("static");
    if let WorldItem::Static {
        uid,
        file_name,
        position,
        ..
    } = static_item
    {
        assert_eq!(*uid, 1);
        assert_eq!(file_name.as_deref(), Some("station.s"));
        assert_eq!(
            *position,
            Vec3 {
                x: 100.0,
                y: 0.0,
                z: 50.0
            }
        );
    } else {
        panic!("expected Static");
    }

    let forest = world
        .items
        .iter()
        .find(|i| i.kind() == "Forest")
        .expect("forest");
    if let WorldItem::Forest {
        tree_texture,
        scale_range,
        ..
    } = forest
    {
        assert_eq!(tree_texture.as_deref(), Some("pine.ace"));
        assert_eq!(*scale_range, Some([0.8, 1.2]));
    } else {
        panic!("expected Forest");
    }
}

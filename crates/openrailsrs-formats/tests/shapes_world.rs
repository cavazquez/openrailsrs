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
fn binary_shape_returns_unsupported_error() {
    // SIMISA header + a stretch of binary tokens (lots of low/non-printable bytes).
    let mut bytes = b"SIMISA@@@@@@@@@@JINX0s1t______".to_vec();
    for _ in 0..128 {
        bytes.push(0x07);
        bytes.push(0x01);
        bytes.push(0x02);
        bytes.push(0x00);
    }

    let tmp = std::env::temp_dir().join("openrailsrs_binary_shape_fixture.s");
    std::fs::write(&tmp, &bytes).unwrap();
    let err = ShapeFile::from_path(&tmp).expect_err("binary should fail");
    assert!(matches!(err, FormatError::UnsupportedBinaryShape));
    let _ = std::fs::remove_file(&tmp);
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
        tree_texture, area, ..
    } = forest
    {
        assert_eq!(tree_texture.as_deref(), Some("pine.ace"));
        assert_eq!(*area, Some([0.8, 1.2]));
    } else {
        panic!("expected Forest");
    }
}

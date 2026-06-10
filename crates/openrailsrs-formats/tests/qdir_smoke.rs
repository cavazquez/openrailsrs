//! Regression: binary `.w` tiles must keep object rotation (`QDirection` token 945).
use openrailsrs_formats::{WorldFile, WorldItem};

#[test]
fn chiltern_binary_world_items_have_rotation() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/chiltern/WORLD/w-006084+014923.w");
    if !path.exists() {
        return;
    }
    let world = WorldFile::from_path(&path).expect("parse");
    let statics: Vec<_> = world
        .items
        .iter()
        .filter(|i| matches!(i, WorldItem::Static { .. }))
        .collect();
    let with_rot = statics
        .iter()
        .filter(|i| i.qdirection().is_some() || i.matrix3x3().is_some())
        .count();
    println!("statics={} with_rotation={}", statics.len(), with_rot);
    assert!(
        with_rot * 2 >= statics.len(),
        "most binary Static items should carry rotation, got {with_rot}/{}",
        statics.len()
    );
}

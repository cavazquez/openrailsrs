#[path = "../openrailsrs-content/cache_fingerprint_build.rs"]
mod cache_fingerprint;
fn main() {
    cache_fingerprint::emit(
        "OPENRAILSRS_GEOMETRY_COMPILER",
        &[
            "src/prepared_geometry.rs",
            "src/shapes.rs",
            "../openrailsrs-bevy-scenery/src/shapes/mesh.rs",
            "../openrailsrs-formats/src/typed/shape.rs",
            "../openrailsrs-formats/src/shape_binary_reader.rs",
            "../openrailsrs-formats/src/shape_binary.rs",
            "../openrailsrs-formats/src/encoding.rs",
            "../openrailsrs-formats/src/lexer.rs",
            "../openrailsrs-formats/src/parser.rs",
            "../openrailsrs-content/cache_fingerprint_build.rs",
            "Cargo.toml",
            "../../Cargo.lock",
        ],
    );
}

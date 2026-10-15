#[path = "../openrailsrs-content/cache_fingerprint_build.rs"]
mod cache_fingerprint;
fn main() {
    cache_fingerprint::emit(
        "OPENRAILSRS_IMPORT_COMPILER",
        &[
            "src/import_route.rs",
            "src/native_signal_import.rs",
            "src/native_cant_import.rs",
            "src/native_timetable.rs",
            "src/import_activity.rs",
            "src/path_placement.rs",
            "src/operational_path.rs",
            "src/import_timetable.rs",
            "../openrailsrs-track/src/sigscript.rs",
            "../openrailsrs-track/src/graph.rs",
            "../openrailsrs-formats/src/typed/track_db.rs",
            "../openrailsrs-formats/src/typed/world.rs",
            "../openrailsrs-formats/src/typed/activity.rs",
            "../openrailsrs-formats/src/typed/path.rs",
            "../openrailsrs-formats/src/typed/tsection.rs",
            "../openrailsrs-formats/src/typed/cant.rs",
            "../openrailsrs-formats/src/typed/cant_speeds.rs",
            "../openrailsrs-formats/src/typed/route.rs",
            "../openrailsrs-formats/src/encoding.rs",
            "../openrailsrs-formats/src/lexer.rs",
            "../openrailsrs-formats/src/parser.rs",
            "../openrailsrs-content/cache_fingerprint_build.rs",
            "Cargo.toml",
            "../../Cargo.lock",
        ],
    );
}

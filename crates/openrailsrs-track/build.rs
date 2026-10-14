#[path = "../openrailsrs-content/cache_fingerprint_build.rs"]
mod cache_fingerprint;
fn main() {
    cache_fingerprint::emit(
        "OPENRAILSRS_SIGNAL_COMPILER",
        &[
            "src/sigscript.rs",
            "src/graph.rs",
            "../openrailsrs-content/cache_fingerprint_build.rs",
            "Cargo.toml",
            "../../Cargo.lock",
        ],
    );
}

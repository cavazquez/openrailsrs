//! Build-time cache identity. Only the digest enters runtime binaries.
use sha2::{Digest, Sha256};
use std::path::Path;
pub fn emit(variable: &str, files: &[&str]) {
    let root = std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
    let root = Path::new(&root);
    let mut digest = Sha256::new();
    digest.update(b"openrailsrs-cache-reader-v2\0");
    for relative in files {
        println!("cargo::rerun-if-changed={relative}");
        let bytes = std::fs::read(root.join(relative))
            .expect("Cache reader source is available during build");
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(relative.as_bytes());
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    println!("cargo::rustc-env={variable}={:x}", digest.finalize());
}

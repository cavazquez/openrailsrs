//! Smoke tests for the `shape-dump`, `world-dump` and `ace-decode` CLI
//! subcommands.  We invoke the compiled binary directly via the
//! `CARGO_BIN_EXE_openrailsrs` env var that Cargo provides for integration
//! tests, which keeps the dev-dep surface flat.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_openrailsrs"))
}

fn formats_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("openrailsrs-formats")
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn synth_rgba8_ace(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut bytes = b"@ACE".to_vec();
    bytes.extend_from_slice(&2u32.to_le_bytes()); // width = 2
    bytes.extend_from_slice(&2u32.to_le_bytes()); // height = 2
    bytes.extend_from_slice(&0u32.to_le_bytes()); // RGBA8
    bytes.push(1);
    bytes.push(4);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&[
        0xFF, 0x00, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF,
    ]);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn shape_dump_runs_on_minimal_fixture() {
    let out = Command::new(bin())
        .args(["shape-dump", formats_fixture("minimal.s").to_str().unwrap()])
        .output()
        .expect("run shape-dump");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("shape-dump"));
    assert!(stdout.contains("lod_controls"));
    assert!(stdout.contains("textures"));
}

#[test]
fn shape_dump_json_emits_structured_stats() {
    let out = Command::new(bin())
        .args([
            "shape-dump",
            "--json",
            formats_fixture("minimal.s").to_str().unwrap(),
        ])
        .output()
        .expect("run shape-dump --json");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid json");
    assert_eq!(value["lod_controls"], 1);
    assert_eq!(value["distance_levels"], 2);
    assert_eq!(value["primitives"], 2);
    assert_eq!(value["textures"], 2);
}

#[test]
fn world_dump_writes_csv() {
    let csv_out = std::env::temp_dir().join("openrailsrs_world_dump.csv");
    let _ = std::fs::remove_file(&csv_out);

    let out = Command::new(bin())
        .args([
            "world-dump",
            formats_fixture("w-001000-001000.w").to_str().unwrap(),
            "--csv",
            csv_out.to_str().unwrap(),
        ])
        .output()
        .expect("run world-dump");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Static"));
    assert!(stdout.contains("Forest"));
    assert!(stdout.contains("Signal"));

    let csv = std::fs::read_to_string(&csv_out).expect("csv created");
    assert!(csv.starts_with("kind,uid,file_name,x,y,z"));
    assert!(csv.contains("Static,1,station.s"));
    let _ = std::fs::remove_file(&csv_out);
}

#[test]
fn terrain_dump_runs_on_minimal_fixture() {
    let out = Command::new(bin())
        .args([
            "terrain-dump",
            formats_fixture("minimal_terrain.y").to_str().unwrap(),
        ])
        .output()
        .expect("run terrain-dump");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("terrain-dump"));
    assert!(stdout.contains("256"));
    assert!(stdout.contains("vertices"));
}

#[test]
fn ace_decode_writes_png() {
    let ace_path = std::env::temp_dir().join("openrailsrs_cli_test.ace");
    let png_out = std::env::temp_dir().join("openrailsrs_cli_test.png");
    let _ = std::fs::remove_file(&png_out);
    synth_rgba8_ace(&ace_path);

    let out = Command::new(bin())
        .args([
            "ace-decode",
            ace_path.to_str().unwrap(),
            png_out.to_str().unwrap(),
        ])
        .output()
        .expect("run ace-decode");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(png_out.exists(), "png not written");
    let _ = std::fs::remove_file(&ace_path);
    let _ = std::fs::remove_file(&png_out);
}

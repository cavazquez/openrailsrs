//! Integration tests for MSTS file encoding detection (Fase 25b).
//!
//! Fixtures:
//!   `minimal.eng`        — plain UTF-8 (reference)
//!   `minimal_utf16le.eng`— same content, UTF-16-LE with BOM (FF FE)
//!   `minimal_win1252.eng`— same structure but WagonShape name uses 0xE9 (é)
//!
//! All three should parse correctly with `parse_msts_file` / `EngineFile::from_ast`.

use std::path::PathBuf;

use openrailsrs_formats::{MstsFile, decode_msts_bytes, parse_msts_file, read_msts_file_to_string};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ── decode_msts_bytes unit tests ──────────────────────────────────────────────

#[test]
fn decode_pure_ascii() {
    let bytes = b"( Engine ( Mass 80000 ) )";
    let s = decode_msts_bytes(bytes);
    assert_eq!(s, "( Engine ( Mass 80000 ) )");
}

#[test]
fn decode_utf8_bom_stripped() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"( Engine )");
    let s = decode_msts_bytes(&bytes);
    assert_eq!(s, "( Engine )");
    assert!(
        !s.starts_with('\u{FEFF}'),
        "BOM should not appear in output"
    );
}

#[test]
fn decode_utf16_le_with_bom() {
    // Encode a simple token list as UTF-16-LE with BOM.
    let text = "( Test )";
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let s = decode_msts_bytes(&bytes);
    assert_eq!(s.trim(), "( Test )");
}

#[test]
fn decode_utf16_be_with_bom() {
    let text = "( Test )";
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    let s = decode_msts_bytes(&bytes);
    assert_eq!(s.trim(), "( Test )");
}

#[test]
fn decode_windows1252_high_byte() {
    // 0xE9 = 'é' in Windows-1252
    let bytes = b"caf\xe9";
    let s = decode_msts_bytes(bytes);
    assert_eq!(s, "café");
}

// ── read_msts_file_to_string round-trip ───────────────────────────────────────

#[test]
fn read_utf8_fixture() {
    let path = fixtures().join("minimal.eng");
    let s = read_msts_file_to_string(&path).expect("read utf-8 .eng");
    assert!(s.contains("Engine"));
    assert!(s.contains("MaxPower"));
}

#[test]
fn read_utf16le_fixture() {
    let path = fixtures().join("minimal_utf16le.eng");
    let s = read_msts_file_to_string(&path).expect("read utf-16le .eng");
    // After decoding, the content should be identical to the UTF-8 version.
    assert!(s.contains("Engine"), "decoded text should contain 'Engine'");
    assert!(
        s.contains("MaxPower"),
        "decoded text should contain 'MaxPower'"
    );
    assert!(s.contains("80000"), "mass value should survive decoding");
}

// ── parse_msts_file integration ───────────────────────────────────────────────

#[test]
fn parse_utf8_eng() {
    let path = fixtures().join("minimal.eng");
    let result = parse_msts_file(&path).expect("parse utf-8 .eng");
    assert!(
        matches!(result, MstsFile::Engine(_)),
        "expected Engine variant"
    );
}

#[test]
fn parse_utf16le_eng_gives_same_result() {
    let path_utf8 = fixtures().join("minimal.eng");
    let path_utf16 = fixtures().join("minimal_utf16le.eng");

    let utf8_result = parse_msts_file(&path_utf8).expect("parse utf-8 .eng");
    let utf16_result = parse_msts_file(&path_utf16).expect("parse utf-16le .eng");

    // Both should parse to an EngineFile with the same numeric values.
    match (&utf8_result, &utf16_result) {
        (MstsFile::Engine(e1), MstsFile::Engine(e2)) => {
            assert_eq!(e1.mass_kg, e2.mass_kg, "mass_kg mismatch between encodings");
            assert_eq!(
                e1.max_power_w, e2.max_power_w,
                "max_power_w mismatch between encodings"
            );
        }
        _ => panic!("expected Engine variant for both files"),
    }
}

#[test]
fn parse_win1252_eng() {
    // The fixture has WagonShape "café.s" encoded in Windows-1252 (0xE9 = é).
    // EngineFile does not expose the shape name, but the file should parse
    // without error and yield the correct numeric fields.
    let path = fixtures().join("minimal_win1252.eng");
    let result = parse_msts_file(&path).expect("parse windows-1252 .eng");
    match result {
        MstsFile::Engine(e) => {
            assert_eq!(
                e.mass_kg, 80_000.0,
                "mass_kg should survive Windows-1252 decoding"
            );
            assert_eq!(
                e.max_power_w, 2_000_000.0,
                "max_power_w should survive decoding"
            );
        }
        other => panic!("expected Engine variant, got {other:?}"),
    }
}

#[test]
fn decode_win1252_fixture_contains_accented_char() {
    // Verify the raw decoding step gives us 'é' from the binary fixture.
    let path = fixtures().join("minimal_win1252.eng");
    let s = read_msts_file_to_string(&path).expect("read win1252 .eng");
    assert!(
        s.contains('é'),
        "decoded text should contain 'é'; got: {s:?}"
    );
}

#[test]
fn chiltern_birmingham_act_player_path() {
    let path = std::path::Path::new(
        "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/ACTIVITIES/RS_Let's go to Birmingham.act",
    );
    if !path.exists() {
        return;
    }
    let act = openrailsrs_formats::ActivityFile::from_path(path).expect("parse act");
    assert!(
        act.player_path.contains("Birmingham"),
        "path={} service={:?}",
        act.player_path,
        act.player_service_id
    );
}

#[test]
fn chiltern_birmingham_pat_start_node() {
    let path = std::path::Path::new(
        "/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern/PATHS/RS_Let's go to Birmingham.pat",
    );
    if !path.exists() {
        return;
    }
    let pat = openrailsrs_formats::PathFile::from_path(path).expect("parse pat");
    assert!(pat.pdps.len() > 10);
    assert!(pat.has_world_pdps());
    // Native TrackPDP: last two ints are junction/invalid flags, not TDB node ids.
    assert!(pat.pdps[0].node_id.is_none());
    assert_eq!(pat.pdps[0].junction_flag, 1);
    assert_eq!(pat.pdps[0].invalid_flag, 0);
    let w = pat.pdps[0].world.expect("PAT start world");
    assert_eq!(w.tile_x, -6079);
    assert_eq!(w.tile_z, 14925);
}

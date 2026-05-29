//! Headless tests for the `openrailsrs-ace` decoder.  Fixtures are generated
//! programmatically (synthetic ACE bytes) so the repo doesn't need to ship any
//! MSTS-licensed assets.

use std::path::PathBuf;

use openrailsrs_ace::{AceFile, AceFormat, read_ace, write_png};

/// Returns a unique temporary path per test so parallel runs don't race.
fn tmp_path(suffix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("openrailsrs_ace_{suffix}"))
}

/// Encode a synthetic ACE blob with the `@ACE` magic.
fn synth_ace(width: u32, height: u32, format_id: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = b"@ACE".to_vec();
    bytes.extend_from_slice(&width.to_le_bytes());
    bytes.extend_from_slice(&height.to_le_bytes());
    bytes.extend_from_slice(&format_id.to_le_bytes());
    bytes.push(1); // mip count
    bytes.push(4); // channels
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(payload);
    bytes
}

/// Build an OR-format structured RGBA8 ACE with the `SIMISA@@@@@@@@@@` header.
///
/// `rgba_pixels` is the raw RGBA8 data for all `width × height` pixels.
/// Channels are written in R, G, B, A order (matching Open Rails `AceFile.cs`).
fn synth_or_ace_uncompressed(width: u32, height: u32, rgba_pixels: &[u8]) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;

    // 16-byte uncompressed SIMISA header
    let mut out = b"SIMISA@@@@@@@@@@".to_vec();

    // OR body: magic + options + dims + surfaceFormat + channelCount
    out.extend_from_slice(b"\x01\x00\x00\x00"); // magic
    out.extend_from_slice(&0u32.to_le_bytes()); // options = 0 (no mipmaps, structured)
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // surfaceFormat (ignored)
    out.extend_from_slice(&4u32.to_le_bytes()); // channelCount = 4 (R, G, B, A)
    out.extend_from_slice(&[0u8; 128]); // misc block

    // Channel descriptors: (u64 size=8, u64 type) for R=3, G=4, B=5, A=6
    for ch_type in [3u64, 4u64, 5u64, 6u64] {
        out.extend_from_slice(&8u64.to_le_bytes()); // size = 8 bpp
        out.extend_from_slice(&ch_type.to_le_bytes());
    }

    // Scanline offset table: image_count=1, height scanlines (OR reads and discards these)
    out.extend_from_slice(&vec![0u8; 4 * h]);

    // Image data: for each scanline, write R, G, B, A channels sequentially
    for y in 0..h {
        // R channel for scanline y
        for x in 0..w {
            out.push(rgba_pixels[(y * w + x) * 4]);
        }
        // G channel
        for x in 0..w {
            out.push(rgba_pixels[(y * w + x) * 4 + 1]);
        }
        // B channel
        for x in 0..w {
            out.push(rgba_pixels[(y * w + x) * 4 + 2]);
        }
        // A channel
        for x in 0..w {
            out.push(rgba_pixels[(y * w + x) * 4 + 3]);
        }
    }

    out
}

fn write_rgba8_4x4(path: &std::path::Path) {
    // 4x4 = 16 pixels.  Two horizontal stripes: red top, blue bottom.
    let mut payload = Vec::with_capacity(16 * 4);
    for y in 0..4 {
        for _x in 0..4 {
            if y < 2 {
                payload.extend_from_slice(&[0xFF, 0x00, 0x00, 0xFF]); // red
            } else {
                payload.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]); // blue
            }
        }
    }
    let bytes = synth_ace(4, 4, 0, &payload);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn write_dxt1_4x4(path: &std::path::Path) {
    // 4x4 single DXT1 block: solid red.  Block layout:
    //   color0 = R5G6B5(255, 0, 0)
    //   color1 = R5G6B5(0, 0, 0)
    //   indices = 16 × 0  (all reference color0)
    let color0: u16 = 0b1111_1000_0000_0000; // R=31, G=0, B=0 → red
    let color1: u16 = 0;
    let mut block = Vec::with_capacity(8);
    block.extend_from_slice(&color0.to_le_bytes());
    block.extend_from_slice(&color1.to_le_bytes());
    block.extend_from_slice(&[0u8; 4]); // 16 × 2-bit indices = 4 bytes, all 0
    let bytes = synth_ace(4, 4, 1, &block);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn read_rgba8_4x4_decodes_known_stripes() {
    let path = tmp_path("rgba8_4x4_stripes.ace");
    write_rgba8_4x4(&path);

    let ace = read_ace(&path).expect("decode rgba8");
    assert_eq!(ace.width, 4);
    assert_eq!(ace.height, 4);
    assert_eq!(ace.format, AceFormat::Rgba8);
    assert_eq!(ace.mip0.len(), 4 * 4 * 4);

    // Pixel (0, 0) red, pixel (0, 3) blue.
    assert_eq!(&ace.mip0[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    let last_row_offset = 4 * 4 * 3; // y=3
    assert_eq!(
        &ace.mip0[last_row_offset..last_row_offset + 4],
        &[0x00, 0x00, 0xFF, 0xFF]
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn read_dxt1_4x4_decodes_solid_red() {
    let path = tmp_path("dxt1_4x4.ace");
    write_dxt1_4x4(&path);

    let ace = read_ace(&path).expect("decode dxt1");
    assert_eq!(ace.width, 4);
    assert_eq!(ace.height, 4);
    assert_eq!(ace.format, AceFormat::Dxt1);
    assert_eq!(ace.mip0.len(), 4 * 4 * 4);

    // Every pixel should be (~) red.  texpresso decompresses to 8-bit RGBA;
    // R=31/31 → 255 exactly, G=B=0.
    for chunk in ace.mip0.chunks_exact(4) {
        assert_eq!(chunk[0], 0xFF, "R");
        assert_eq!(chunk[1], 0x00, "G");
        assert_eq!(chunk[2], 0x00, "B");
        assert_eq!(chunk[3], 0xFF, "A");
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn write_png_produces_valid_image() {
    let path = tmp_path("rgba8_4x4_png.ace");
    write_rgba8_4x4(&path);

    let ace = read_ace(&path).expect("decode rgba8");
    let out = tmp_path("rgba8_4x4_png.png");
    write_png(&ace, &out).expect("write png");

    let img = image::open(&out).expect("open png");
    assert_eq!(img.width(), 4);
    assert_eq!(img.height(), 4);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn read_simisa_uncompressed_or_format() {
    // Build a proper OR-format uncompressed ACE (SIMISA@@@@@@@@@@  header) with
    // a single 1×1 pixel of known RGBA colour.
    let bytes = synth_or_ace_uncompressed(1, 1, &[0x33, 0x66, 0x99, 0xCC]);
    let ace = AceFile::read_bytes(&bytes).expect("decode OR uncompressed ACE");
    assert_eq!(ace.width, 1);
    assert_eq!(ace.height, 1);
    assert_eq!(ace.mip0, vec![0x33, 0x66, 0x99, 0xCC]);
}

#[test]
fn read_simisa_uncompressed_2x2_or_format() {
    // 2×2 checkerboard: top-left red, top-right green, bottom-left blue, bottom-right white.
    let pixels: &[u8] = &[
        0xFF, 0x00, 0x00, 0xFF, // (0,0) red
        0x00, 0xFF, 0x00, 0xFF, // (1,0) green
        0x00, 0x00, 0xFF, 0xFF, // (0,1) blue
        0xFF, 0xFF, 0xFF, 0xFF, // (1,1) white
    ];
    let bytes = synth_or_ace_uncompressed(2, 2, pixels);
    let ace = AceFile::read_bytes(&bytes).expect("decode 2x2 OR ACE");
    assert_eq!(ace.width, 2);
    assert_eq!(ace.height, 2);
    assert_eq!(ace.mip0, pixels);
}

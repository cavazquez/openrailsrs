//! Decoder for MSTS `.ace` (Asynchronous Compressed Encoding) textures.
//!
//! Implements the format exactly as Open Rails does in `AceFile.cs`.
//!
//! # SIMISA container
//!
//! Two on-disk layouts are recognised:
//!
//! - **Compressed** (`SIMISA@F` magic): 8-byte magic + `u32` uncompressed size +
//!   `@@@@` + ZLIB-framed DEFLATE payload.  The payload is decompressed before
//!   parsing the ACE body.
//! - **Uncompressed** (`SIMISA@@@@@@@@@@` magic): 16-byte magic, ACE body follows
//!   immediately.
//!
//! A third, project-internal `@ACE` magic is used only by synthetic test
//! fixtures produced by [`write_synthetic_ace`].
//!
//! # ACE body
//!
//! ```text
//! [0..3]    u32  magic = 0x00000001
//! [4..7]    i32  options  (bit 0x01 = mipmaps; bit 0x10 = RawData/DXT)
//! [8..11]   i32  width
//! [12..15]  i32  height
//! [16..19]  i32  surfaceFormat  (only when RawData: 0x12=DXT1, 0x14=DXT3, 0x16=DXT5)
//! [20..23]  i32  channelCount
//! [24..151] u8×128  misc (ignored)
//! [152..]   channelCount × { u64 size (1 or 8 bpp), u64 type (2=Mask 3=R 4=G 5=B 6=A) }
//! ```
//!
//! Followed by image data in one of two formats:
//!
//! - **RawData** (`options & 0x10`): per-mip DXT blocks prefixed with a `i32` length.
//! - **Structured** (otherwise): per-scanline channels packed sequentially.

use std::{io::Read, path::Path};

use thiserror::Error;

// ── Synthetic test-fixture magic (project-internal, not MSTS) ───────────────
const ACE_MAGIC: &[u8; 4] = b"@ACE";
// ── SIMISA container prefixes ────────────────────────────────────────────────
const SIMISA_COMPRESSED: &[u8; 8] = b"SIMISA@F";
const SIMISA_UNCOMPRESSED: &[u8; 16] = b"SIMISA@@@@@@@@@@";

// ── OR SimisAceFormatOptions flags ───────────────────────────────────────────
const OPT_MIPMAPS: u32 = 0x01;
const OPT_RAW_DATA: u32 = 0x10;

// ── OR SimisAceChannelId ─────────────────────────────────────────────────────
const CH_MASK: u64 = 2;
const CH_RED: u64 = 3;
const CH_GREEN: u64 = 4;
const CH_BLUE: u64 = 5;
const CH_ALPHA: u64 = 6;

/// Pixel format of the decoded texture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AceFormat {
    /// 8 bits per channel, RGBA byte order.
    Rgba8,
    Dxt1,
    Dxt3,
    Dxt5,
}

impl AceFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            AceFormat::Rgba8 => "RGBA8",
            AceFormat::Dxt1 => "DXT1",
            AceFormat::Dxt3 => "DXT3",
            AceFormat::Dxt5 => "DXT5",
        }
    }
}

/// In-memory ACE file with mip 0 decoded to RGBA8.
#[derive(Clone, Debug)]
pub struct AceFile {
    pub width: u32,
    pub height: u32,
    pub format: AceFormat,
    pub mips_count: u8,
    /// Decoded mip 0, RGBA8 byte order, length = `width * height * 4`.
    pub mip0: Vec<u8>,
    /// True if the alpha channel originated from a 1-bit MASK channel (kind=2),
    /// meaning pixels are binary (0 or 255 only). Use `AlphaMode::Mask` not
    /// `AlphaMode::Blend` for correct cutout rendering without depth-sort artefacts.
    /// False when alpha comes from a full 8-bit ALPHA channel (kind=6) or when
    /// there is no alpha at all.
    pub has_mask_channel: bool,
    /// Number of alpha bits (0, 1 for Mask, 8 for Alpha) as defined in Open Rails.
    pub alpha_bits: u8,
}

#[derive(Debug, Error)]
pub enum AceError {
    #[error("ace file is too short ({0} bytes)")]
    Truncated(usize),
    #[error("unknown ace magic at byte {offset}")]
    UnknownMagic { offset: usize },
    #[error("unsupported ace pixel format {0:#X}")]
    UnsupportedFormat(u32),
    #[error("invalid dimensions {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("png encoding error: {0}")]
    Png(#[from] image::ImageError),
    #[error("decompression error: {0}")]
    Decompress(String),
}

/// Read and decode an `.ace` file from disk.
pub fn read_ace(path: impl AsRef<Path>) -> Result<AceFile, AceError> {
    let bytes = std::fs::read(path.as_ref())?;
    AceFile::read_bytes(&bytes)
}

/// Encode `mip0` to a PNG file at `path`.
pub fn write_png(ace: &AceFile, path: impl AsRef<Path>) -> Result<(), AceError> {
    let img = image::RgbaImage::from_raw(ace.width, ace.height, ace.mip0.clone()).ok_or(
        AceError::InvalidDimensions {
            width: ace.width,
            height: ace.height,
        },
    )?;
    img.save(path.as_ref())?;
    Ok(())
}

impl AceFile {
    /// Parse an in-memory ACE byte stream.
    ///
    /// Detects all supported container formats and dispatches to the
    /// appropriate body parser.
    pub fn read_bytes(bytes: &[u8]) -> Result<Self, AceError> {
        if bytes.starts_with(SIMISA_COMPRESSED) {
            // Compressed: SIMISA@F + u32 uncompressed_size + @@@@ + zlib(DEFLATE)
            // The zlib stream (0x78 0x9C …) starts at byte 16.
            if bytes.len() < 18 {
                return Err(AceError::Truncated(bytes.len()));
            }
            let decompressed = zlib_decompress(&bytes[16..])?;
            parse_or_body(&decompressed)
        } else if bytes.starts_with(SIMISA_UNCOMPRESSED) {
            // Uncompressed: 16-byte SIMISA header, body at offset 16.
            parse_or_body(&bytes[16..])
        } else if bytes.len() >= 4 && bytes[..4] == *ACE_MAGIC {
            // Project-internal synthetic format used by test fixtures.
            parse_synthetic_body(&bytes[4..])
        } else {
            Err(AceError::UnknownMagic { offset: 0 })
        }
    }
}

// ── OR body parser ────────────────────────────────────────────────────────────

/// Parse an ACE body after the SIMISA container header has been consumed.
///
/// Layout matches Open Rails `AceFile.cs :: Texture2DFromReader`.
fn parse_or_body(body: &[u8]) -> Result<AceFile, AceError> {
    // 4-byte magic
    if body.get(..4) != Some(b"\x01\x00\x00\x00") {
        return Err(AceError::UnknownMagic { offset: 0 });
    }

    if body.len() < 152 {
        return Err(AceError::Truncated(body.len()));
    }

    let options = read_u32_le(body, 4);
    let width = read_u32_le(body, 8);
    let height = read_u32_le(body, 12);
    let surface_format = read_u32_le(body, 16);
    let channel_count = read_u32_le(body, 20) as usize;
    // 128 bytes misc at [24..151] — skip

    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(AceError::InvalidDimensions { width, height });
    }

    // Channel descriptors — each is u64 size + u64 type (16 bytes)
    let channels_start = 152;
    let channels_end = channels_start + channel_count * 16;
    if body.len() < channels_end {
        return Err(AceError::Truncated(body.len()));
    }

    let mut channels = Vec::with_capacity(channel_count);
    for i in 0..channel_count {
        let base = channels_start + i * 16;
        let size = read_u64_le(body, base) as u8; // 1 or 8 (bpp)
        let kind = read_u64_le(body, base + 8); // CH_RED..CH_ALPHA
        channels.push((size, kind));
    }

    let has_mipmaps = (options & OPT_MIPMAPS) != 0;
    let is_raw_data = (options & OPT_RAW_DATA) != 0;
    let image_count = if has_mipmaps {
        1 + f64::log2(width as f64) as usize
    } else {
        1
    };

    let data_offset = channels_end;

    if is_raw_data {
        parse_or_raw_data(
            body,
            data_offset,
            width,
            height,
            surface_format,
            image_count,
            &channels,
        )
    } else {
        parse_or_structured(body, data_offset, width, height, &channels, image_count)
    }
}

/// RawData path: per-mip DXT blocks prefixed with an i32 length.
fn parse_or_raw_data(
    body: &[u8],
    data_offset: usize,
    width: u32,
    height: u32,
    surface_format: u32,
    image_count: usize,
    channels: &[(u8, u64)],
) -> Result<AceFile, AceError> {
    let format = match surface_format {
        0x12 => AceFormat::Dxt1,
        0x14 => AceFormat::Dxt3,
        0x16 => AceFormat::Dxt5,
        other => return Err(AceError::UnsupportedFormat(other)),
    };

    // Skip offset table (imageCount × i32)
    let after_table = data_offset + image_count * 4;

    // Mip 0 block: i32 size + data (only when width >= 4 && height >= 4)
    let mip0 = if width >= 4 && height >= 4 {
        if body.len() < after_table + 4 {
            return Err(AceError::Truncated(body.len()));
        }
        let mip_len = read_u32_le(body, after_table) as usize;
        let mip_data = body
            .get(after_table + 4..after_table + 4 + mip_len)
            .ok_or(AceError::Truncated(body.len()))?;
        decode_dxt(format, width, height, mip_data)?
    } else {
        vec![0xFFu8; width as usize * height as usize * 4]
    };

    let alpha_bits = if channels.iter().any(|(_, k)| *k == CH_ALPHA) {
        8
    } else if channels.iter().any(|(_, k)| *k == CH_MASK) {
        1
    } else {
        0
    };

    Ok(AceFile {
        width,
        height,
        format,
        mips_count: image_count as u8,
        mip0,
        // DXT raw-data path has no explicit alpha/mask channels.
        has_mask_channel: false,
        alpha_bits,
    })
}

/// Structured path: channels packed sequentially, scanline by scanline.
///
/// Matches OR exactly:
/// ```csharp
/// for each mip: reader.ReadBytes(4 * height / 2^mip);  // skip offsets
/// for each mip:
///   for each y:
///     for each channel: read size data
///   assemble RGBA
/// ```
fn parse_or_structured(
    body: &[u8],
    data_offset: usize,
    width: u32,
    height: u32,
    channels: &[(u8, u64)],
    image_count: usize,
) -> Result<AceFile, AceError> {
    let w = width as usize;
    let h = height as usize;

    // Skip scanline offset tables for ALL mips
    let mut pos = data_offset;
    for i in 0..image_count {
        let mip_h = (h >> i).max(1);
        pos += 4 * mip_h;
    }

    // Read ALL mip levels (as OR does), but only assemble mip 0 into the output.
    let mut mip0 = vec![0u8; w * h * 4];

    for img_idx in 0..image_count {
        let mip_w = (w >> img_idx).max(1);
        let mip_h = (h >> img_idx).max(1);

        // Per-channel row buffers (indexed by channel kind 0..=7)
        let mut ch_buf: [Vec<u8>; 8] = Default::default();
        for (_, kind) in channels {
            let k = *kind as usize;
            if k < 8 {
                ch_buf[k] = vec![0xFFu8; mip_w];
            }
        }

        for y in 0..mip_h {
            for &(size, kind) in channels {
                let bytes_needed = if size == 1 {
                    mip_w.div_ceil(8)
                } else {
                    mip_w // 8 bpp: one byte per pixel
                };

                let slice = body.get(pos..pos + bytes_needed).unwrap_or(&[]);
                pos += bytes_needed;

                let k = kind as usize;
                if k >= 8 {
                    continue;
                }

                if size == 1 {
                    // 1 bpp: MSB first
                    for (x, dst) in ch_buf[k].iter_mut().enumerate().take(mip_w) {
                        *dst = if slice
                            .get(x / 8)
                            .is_some_and(|&b| (b >> (7 - x % 8)) & 1 != 0)
                        {
                            0xFF
                        } else {
                            0
                        };
                    }
                } else {
                    // 8 bpp
                    let copy_len = slice.len().min(mip_w);
                    ch_buf[k][..copy_len].copy_from_slice(&slice[..copy_len]);
                }
            }

            // Assemble pixels only for mip 0
            if img_idx == 0 {
                let has_alpha = channels.iter().any(|(_, k)| *k == CH_ALPHA);
                let has_mask = channels.iter().any(|(_, k)| *k == CH_MASK);
                for (x, base) in (0..w).map(|x| (x, (y * w + x) * 4)).collect::<Vec<_>>() {
                    mip0[base] = ch_buf[CH_RED as usize][x];
                    mip0[base + 1] = ch_buf[CH_GREEN as usize][x];
                    mip0[base + 2] = ch_buf[CH_BLUE as usize][x];
                    mip0[base + 3] = if has_alpha {
                        ch_buf[CH_ALPHA as usize][x]
                    } else if has_mask {
                        ch_buf[CH_MASK as usize][x]
                    } else {
                        0xFF
                    };
                }
            }
        }
    }

    let has_mask_channel = channels.iter().any(|(_, k)| *k == CH_MASK)
        && !channels.iter().any(|(_, k)| *k == CH_ALPHA);

    let alpha_bits = if channels.iter().any(|(_, k)| *k == CH_ALPHA) {
        8
    } else if channels.iter().any(|(_, k)| *k == CH_MASK) {
        1
    } else {
        0
    };

    Ok(AceFile {
        width,
        height,
        format: AceFormat::Rgba8,
        mips_count: image_count as u8,
        mip0,
        has_mask_channel,
        alpha_bits,
    })
}

// ── Synthetic test-fixture body parser ───────────────────────────────────────

/// Parse the project-internal `@ACE` synthetic format used by test fixtures:
/// ```text
/// u32 width  u32 height  u32 format_id  u8 mip_count  u8 channels  u16 reserved  pixels…
/// ```
fn parse_synthetic_body(body: &[u8]) -> Result<AceFile, AceError> {
    if body.len() < 16 {
        return Err(AceError::Truncated(body.len()));
    }
    let width = read_u32_le(body, 0);
    let height = read_u32_le(body, 4);
    let format_id = read_u32_le(body, 8);
    let mip_count = body[12];

    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(AceError::InvalidDimensions { width, height });
    }

    let format = match format_id {
        0 => AceFormat::Rgba8,
        1 => AceFormat::Dxt1,
        2 => AceFormat::Dxt3,
        3 => AceFormat::Dxt5,
        other => return Err(AceError::UnsupportedFormat(other)),
    };

    let pixel_data = &body[16..];
    let mip0 = decode_dxt_or_rgba(format, width, height, pixel_data, body.len())?;

    Ok(AceFile {
        width,
        height,
        format,
        mips_count: mip_count.max(1),
        mip0,
        // Synthetic test format has no mask channel metadata.
        has_mask_channel: false,
        alpha_bits: 0,
    })
}

// ── Pixel decoders ────────────────────────────────────────────────────────────

fn decode_dxt_or_rgba(
    format: AceFormat,
    width: u32,
    height: u32,
    data: &[u8],
    total_len: usize,
) -> Result<Vec<u8>, AceError> {
    let w = width as usize;
    let h = height as usize;
    match format {
        AceFormat::Rgba8 => {
            let needed = w * h * 4;
            if data.len() < needed {
                return Err(AceError::Truncated(total_len));
            }
            Ok(data[..needed].to_vec())
        }
        _ => decode_dxt(format, width, height, data),
    }
}

fn decode_dxt(
    format: AceFormat,
    width: u32,
    height: u32,
    data: &[u8],
) -> Result<Vec<u8>, AceError> {
    use texpresso::Format as TF;
    let tf = match format {
        AceFormat::Dxt1 => TF::Bc1,
        AceFormat::Dxt3 => TF::Bc2,
        AceFormat::Dxt5 => TF::Bc3,
        AceFormat::Rgba8 => unreachable!(),
    };
    let w = width as usize;
    let h = height as usize;
    let needed = tf.compressed_size(w, h);
    if data.len() < needed {
        return Err(AceError::Truncated(data.len()));
    }
    let mut out = vec![0u8; w * h * 4];
    tf.decompress(&data[..needed], w, h, &mut out);
    Ok(out)
}

// ── ZLIB decompression ────────────────────────────────────────────────────────

fn zlib_decompress(bytes: &[u8]) -> Result<Vec<u8>, AceError> {
    let mut decoder = flate2::read::ZlibDecoder::new(bytes);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| AceError::Decompress(e.to_string()))?;
    Ok(out)
}

// ── Little-endian helpers ─────────────────────────────────────────────────────

fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64_le(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

// ── Test fixtures helper ──────────────────────────────────────────────────────

/// Write a minimal `@ACE` synthetic file for tests.
///
/// `pixels` is RGBA8 data for all width × height pixels.
#[cfg(test)]
pub fn write_synthetic_ace(path: &std::path::Path, pixels: &[u8]) {
    let side = (pixels.len() / 4).isqrt() as u32;
    let mut bytes = ACE_MAGIC.to_vec();
    bytes.extend_from_slice(&side.to_le_bytes()); // width
    bytes.extend_from_slice(&side.to_le_bytes()); // height
    bytes.extend_from_slice(&0u32.to_le_bytes()); // format = RGBA8
    bytes.push(1); // mip count
    bytes.push(4); // channels
    bytes.extend_from_slice(&[0, 0]); // reserved
    bytes.extend_from_slice(pixels);
    std::fs::write(path, &bytes).unwrap();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba8_roundtrips_known_pixels() {
        let mut bytes = ACE_MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes()); // width
        bytes.extend_from_slice(&1u32.to_le_bytes()); // height
        bytes.extend_from_slice(&0u32.to_le_bytes()); // format = RGBA8
        bytes.push(1); // mip count
        bytes.push(4); // channels
        bytes.extend_from_slice(&[0, 0]); // reserved
        bytes.extend_from_slice(&[0xFF, 0x00, 0x00, 0xFF]); // RGBA red

        let ace = AceFile::read_bytes(&bytes).expect("decode rgba8");
        assert_eq!(ace.width, 1);
        assert_eq!(ace.height, 1);
        assert_eq!(ace.format, AceFormat::Rgba8);
        assert_eq!(ace.mip0, vec![0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn unknown_magic_fails() {
        let bytes = b"NOPENOPE\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(matches!(
            AceFile::read_bytes(bytes),
            Err(AceError::UnknownMagic { .. })
        ));
    }

    #[test]
    fn unsupported_format_fails() {
        let mut bytes = ACE_MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&99u32.to_le_bytes()); // bogus format
        bytes.push(1);
        bytes.push(4);
        bytes.extend_from_slice(&[0, 0]);
        assert!(matches!(
            AceFile::read_bytes(&bytes),
            Err(AceError::UnsupportedFormat(99))
        ));
    }

    /// Verify that a real Chiltern ACE (SIMISA@F compressed, structured channels)
    /// decodes to the expected dimensions without panicking.
    #[test]
    fn decode_chiltern_simisa_f_ace() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/chiltern/trains/RF_Blue_Pullman/TEXTURES/bp01.ace");
        if !path.exists() {
            return; // Skip in CI
        }
        let ace = read_ace(&path).expect("decode bp01.ace");
        assert_eq!(ace.width, 1024);
        assert_eq!(ace.height, 1024);
        assert_eq!(ace.format, AceFormat::Rgba8);
        assert_eq!(ace.mip0.len(), 1024 * 1024 * 4);
        // The first pixel should be valid (not all zeroes for a real texture)
        assert!(
            ace.mip0.iter().any(|&b| b != 0),
            "mip0 should have non-zero pixels"
        );
    }
}

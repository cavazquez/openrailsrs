//! Lossless KTX2 derivative cache. Author files are never replaced.
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType, TextureError, TranscodeFormat},
    prelude::*,
    render::render_resource::TextureFormat,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: usize = 384 * 1024 * 1024;
const VERSION: u32 = 1;
static HITS: AtomicU64 = AtomicU64::new(0);
static MISSES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TextureAlpha {
    pub bits: u8,
    pub mask: bool,
}
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    source_hash: String,
    texture_hash: String,
    alpha: TextureAlpha,
}
pub struct CachedTexture {
    pub image: Image,
    pub alpha: TextureAlpha,
    pub hit: bool,
}

pub fn cache_dir() -> PathBuf {
    openrailsrs_content::data_dir().join("cache/textures-v1")
}
pub fn telemetry() -> (u64, u64) {
    (HITS.load(Ordering::Relaxed), MISSES.load(Ordering::Relaxed))
}
fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
        return Err("Texture exceeds the memory budget".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Texture exceeds the memory budget".into());
    }
    Ok(bytes)
}

/// Validate size and mip lengths before allowing Bevy to decompress an authored file.
pub fn decode_ktx2(
    bytes: &[u8],
    formats: CompressedImageFormats,
    addr: Option<i32>,
    bias: Option<f32>,
) -> Result<Image, String> {
    if bytes.len() > MAX_BYTES {
        return Err("KTX2 exceeds the memory budget".into());
    }
    let reader = ktx2::Reader::new(bytes).map_err(|e| format!("Invalid KTX2: {e:?}"))?;
    let h = reader.header();
    if h.pixel_width == 0
        || h.pixel_width > 8192
        || h.pixel_height == 0
        || h.pixel_height > 8192
        || h.pixel_depth > 1
        || h.layer_count > 1
        || h.face_count != 1
        || h.level_count > 14
        || h.level_count == 0
        || h.level_count > 32 - h.pixel_width.max(h.pixel_height).leading_zeros()
    {
        return Err("KTX2 requires a bounded single 2D texture with a valid mip chain".into());
    }
    if h.supercompression_scheme == Some(ktx2::SupercompressionScheme::BasisLZ) {
        return Err(
            "KTX2 ETC1S/BasisLZ is not supported by Bevy 0.19; use UASTC, BC or RGBA".into(),
        );
    }
    // Bevy's DFD-only path assumes valid sample descriptors. Limit that path
    // to the UASTC layout we can validate; other author formats must declare
    // their Vulkan format explicitly.
    if h.format.is_none() {
        let dfd = reader
            .basic_dfd()
            .ok_or("KTX2 has no basic format descriptor")?;
        if dfd.color_model != Some(ktx2::ColorModel::UASTC)
            || dfd.texel_block_dimensions.map(|v| v.get()) != [4, 4, 1, 1]
            || dfd.bytes_planes != [16, 0, 0, 0, 0, 0, 0, 0]
            || dfd.sample_information.len() != 1
            || !matches!(dfd.sample_information[0].channel_type, 0 | 3 | 4 | 5 | 6)
        {
            return Err("Unsupported or invalid KTX2 format descriptor".into());
        }
    }
    let (bw, bh, block) = match bevy::image::ktx2_get_texture_format(&reader, true) {
        Ok(format) => {
            let (bw, bh) = format.block_dimensions();
            (
                bw,
                bh,
                format
                    .block_copy_size(None)
                    .ok_or("Unsupported KTX2 format")?,
            )
        }
        Err(TextureError::FormatRequiresTranscodingError(TranscodeFormat::Rgb8)) => (1, 1, 3),
        Err(TextureError::FormatRequiresTranscodingError(TranscodeFormat::R8UnormSrgb)) => {
            (1, 1, 1)
        }
        Err(TextureError::FormatRequiresTranscodingError(TranscodeFormat::Rg8UnormSrgb)) => {
            (1, 1, 2)
        }
        Err(TextureError::FormatRequiresTranscodingError(TranscodeFormat::Uastc(_))) => (4, 4, 16),
        Err(error) => return Err(error.to_string()),
    };
    let mut total = 0usize;
    for (i, level) in reader.levels().enumerate() {
        let w = (h.pixel_width >> i).max(1);
        let height = (h.pixel_height >> i).max(1);
        let expected = w.div_ceil(bw) as usize * height.div_ceil(bh) as usize * block as usize;
        total = total.checked_add(expected).ok_or("KTX2 size overflow")?;
        if total > MAX_BYTES || level.uncompressed_byte_length != expected as u64 {
            return Err("KTX2 mip size differs from its dimensions".into());
        }
        match h.supercompression_scheme {
            None if level.data.len() != expected => return Err("Truncated KTX2 mip".into()),
            Some(ktx2::SupercompressionScheme::Zstandard) => {
                // Cap inflation even when a malicious frame lies about its size.
                let data =
                    zstd::bulk::decompress(level.data, expected).map_err(|e| e.to_string())?;
                if data.len() != expected {
                    return Err("Invalid KTX2 Zstd mip".into());
                }
            }
            Some(_) => return Err("Unsupported KTX2 supercompression".into()),
            _ => {}
        }
    }
    let decode = |support| {
        Image::from_buffer(
            bytes,
            ImageType::Extension("ktx2"),
            support,
            true,
            ImageSampler::Default,
            RenderAssetUsages::default(),
        )
        .map_err(|e| e.to_string())
    };
    let raw = decode(formats).or_else(|_| decode(CompressedImageFormats::all()))?;
    let needs_rgba = raw.texture_descriptor.format.is_compressed()
        && (!formats.supports(raw.texture_descriptor.format)
            || !h.pixel_width.is_multiple_of(bw)
            || !h.pixel_height.is_multiple_of(bh));
    let mut image = if needs_rgba { decompress_bc(raw)? } else { raw };
    // Height-one authored images are sampled through texture_2d in OR materials.
    image.texture_descriptor.dimension = bevy::render::render_resource::TextureDimension::D2;
    image.texture_descriptor.mip_level_count = h.level_count.max(1);
    crate::textures::apply_msts_texture_sampler(&mut image, addr, bias);
    Ok(image)
}

fn decompress_bc(mut image: Image) -> Result<Image, String> {
    let block = match image.texture_descriptor.format {
        TextureFormat::Bc1RgbaUnorm | TextureFormat::Bc1RgbaUnormSrgb => texpresso::Format::Bc1,
        TextureFormat::Bc2RgbaUnorm | TextureFormat::Bc2RgbaUnormSrgb => texpresso::Format::Bc2,
        TextureFormat::Bc3RgbaUnorm | TextureFormat::Bc3RgbaUnormSrgb => texpresso::Format::Bc3,
        other => return Err(format!("CPU fallback cannot decompress {other:?}")),
    };
    let data = image.data.as_deref().ok_or("KTX2 has no pixels")?;
    let mut rgba = Vec::new();
    let mut offset = 0;
    for mip in 0..image.texture_descriptor.mip_level_count {
        let w = (image.width() >> mip).max(1) as usize;
        let h = (image.height() >> mip).max(1) as usize;
        let end = offset + block.compressed_size(w, h);
        let blocks = data.get(offset..end).ok_or("Invalid BC mip")?;
        let start = rgba.len();
        rgba.resize(start + w * h * 4, 0);
        block.decompress(blocks, w, h, &mut rgba[start..]);
        offset = end;
    }
    image.texture_descriptor.format = TextureFormat::Rgba8UnormSrgb;
    image.data = Some(rgba);
    Ok(image)
}

/// KTX2 writer uses the Khronos DFD generator; retains native BC or exact RGBA mips.
pub fn encode_ktx2(image: &Image) -> Result<Vec<u8>, String> {
    let format = match image.texture_descriptor.format {
        TextureFormat::Rgba8UnormSrgb => ktx2::Format::R8G8B8A8_SRGB,
        TextureFormat::Rgba8Unorm => ktx2::Format::R8G8B8A8_UNORM,
        TextureFormat::Bc1RgbaUnormSrgb => ktx2::Format::BC1_RGBA_SRGB_BLOCK,
        TextureFormat::Bc2RgbaUnormSrgb => ktx2::Format::BC2_SRGB_BLOCK,
        TextureFormat::Bc3RgbaUnormSrgb => ktx2::Format::BC3_SRGB_BLOCK,
        other => return Err(format!("Cache writer does not support {other:?}")),
    };
    let (basic, type_size) =
        ktx2::dfd::Basic::from_format(format).map_err(|e| format!("DFD: {e:?}"))?;
    let dfd = ktx2::dfd::Block::Basic(basic).to_vec();
    let count = image.texture_descriptor.mip_level_count.max(1) as usize;
    let dfd_offset = 80 + 24 * count;
    let dfd_len = dfd.len() + 4;
    let mut result = vec![0; dfd_offset];
    result.extend((dfd_len as u32).to_le_bytes());
    result.extend(dfd);
    let mut h = ktx2::Header {
        format: Some(format),
        type_size,
        pixel_width: image.width(),
        pixel_height: image.height(),
        pixel_depth: 0,
        layer_count: 0,
        face_count: 1,
        level_count: count as u32,
        supercompression_scheme: Some(ktx2::SupercompressionScheme::Zstandard),
        index: ktx2::Index {
            dfd_byte_offset: dfd_offset as u32,
            dfd_byte_length: dfd_len as u32,
            kvd_byte_offset: 0,
            kvd_byte_length: 0,
            sgd_byte_offset: 0,
            sgd_byte_length: 0,
        },
    };
    let pixels = image.data.as_deref().ok_or("Image has no pixels")?;
    let (bw, bh) = image.texture_descriptor.format.block_dimensions();
    let block = image
        .texture_descriptor
        .format
        .block_copy_size(None)
        .ok_or("Invalid image format")?;
    let mut source_offset = 0;
    let mut mips = Vec::new();
    for mip in 0..count {
        let w = (image.width() >> mip).max(1);
        let height = (image.height() >> mip).max(1);
        let len = w.div_ceil(bw) as usize * height.div_ceil(bh) as usize * block as usize;
        let data = pixels
            .get(source_offset..source_offset + len)
            .ok_or("Image mip chain is truncated")?;
        mips.push((
            zstd::bulk::compress(data, 1).map_err(|e| e.to_string())?,
            len,
        ));
        source_offset += len;
    }
    // KTX2 stores the smallest mip first; the index stays largest to smallest.
    for (mip, (bytes, len)) in mips.into_iter().enumerate().rev() {
        let index = ktx2::LevelIndex {
            byte_offset: result.len() as u64,
            byte_length: bytes.len() as u64,
            uncompressed_byte_length: len as u64,
        };
        result[80 + mip * 24..80 + (mip + 1) * 24].copy_from_slice(&index.as_bytes());
        result.extend(bytes);
    }
    h.level_count = count as u32;
    result[..80].copy_from_slice(&h.as_bytes());
    Ok(result)
}

fn cache_paths(root: &Path, source: &Path) -> (PathBuf, PathBuf) {
    let key = blake3::hash(source.as_os_str().as_encoded_bytes()).to_hex();
    (
        root.join(format!("{key}.ktx2")),
        root.join(format!("{key}.json")),
    )
}
fn alpha(image: &Image) -> TextureAlpha {
    let bits = if matches!(
        image.texture_descriptor.format,
        TextureFormat::Bc1RgbaUnorm | TextureFormat::Bc1RgbaUnormSrgb
    ) {
        1
    } else {
        8
    };
    TextureAlpha {
        bits,
        mask: bits == 1,
    }
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid cache directory")?)
        .map_err(|e| e.to_string())?;
    temp.write_all(bytes).map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn load(
    path: &Path,
    formats: CompressedImageFormats,
    addr: Option<i32>,
    bias: Option<f32>,
) -> Result<CachedTexture, String> {
    let root = if std::env::var("OPENRAILSRS_TEXTURE_CACHE").is_ok_and(|v| v == "off") {
        None
    } else {
        Some(cache_dir())
    };
    load_in(path, root.as_deref(), formats, addr, bias)
}
pub fn load_in(
    path: &Path,
    root: Option<&Path>,
    formats: CompressedImageFormats,
    addr: Option<i32>,
    bias: Option<f32>,
) -> Result<CachedTexture, String> {
    let source = path.canonicalize().map_err(|e| e.to_string())?;
    let bytes = bounded_read(&source, MAX_BYTES)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let ext = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "ktx2" {
        let image = decode_ktx2(&bytes, formats, addr, bias)?;
        return Ok(CachedTexture {
            alpha: alpha(&image),
            image,
            hit: false,
        });
    }
    if let Some(root) = root {
        let (ktx, metadata) = cache_paths(root, &source);
        if let Ok(record) = bounded_read(&metadata, 4096)
            .and_then(|b| serde_json::from_slice::<Record>(&b).map_err(|e| e.to_string()))
            && record.version == VERSION
            && record.source_hash == hash
            && let Ok(cached) = bounded_read(&ktx, MAX_BYTES)
            && blake3::hash(&cached).to_hex().as_str() == record.texture_hash
            && let Ok(image) = decode_ktx2(&cached, formats, addr, bias)
        {
            HITS.fetch_add(1, Ordering::Relaxed);
            return Ok(CachedTexture {
                image,
                alpha: record.alpha,
                hit: true,
            });
        }
    }
    MISSES.fetch_add(1, Ordering::Relaxed);
    let (mut image, alpha) = match ext.as_str() {
        "ace" => {
            let ace = openrailsrs_ace::AceFile::read_bytes(&bytes).map_err(|e| e.to_string())?;
            let image = crate::gpu_textures::ace_image_for_formats(
                &ace,
                CompressedImageFormats::BC,
                addr,
                bias,
            );
            (
                image,
                TextureAlpha {
                    bits: ace.alpha_bits,
                    mask: ace.has_mask_channel,
                },
            )
        }
        "dds" => {
            let image = crate::gpu_textures::decode_dds_for_formats(
                &bytes,
                CompressedImageFormats::BC,
                addr,
                bias,
            )?;
            let bits = if matches!(
                crate::textures::dds_alpha_type(&source),
                Some(crate::textures::DdsAlpha::NoneOr1Bit)
            ) {
                0
            } else {
                8
            };
            (image, TextureAlpha { bits, mask: false })
        }
        _ => return Err("Expected ACE, DDS or KTX2".into()),
    };
    if root.is_some()
        && let Ok(ktx) = encode_ktx2(&image)
    {
        if let Some(root) = root
            && std::fs::create_dir_all(root).is_ok()
        {
            let (target, metadata) = cache_paths(root, &source);
            let record = Record {
                version: VERSION,
                source_hash: hash,
                texture_hash: blake3::hash(&ktx).to_hex().to_string(),
                alpha,
            };
            if atomic_write(&target, &ktx).is_ok()
                && let Ok(record) = serde_json::to_vec(&record)
            {
                let _ = atomic_write(&metadata, &record);
            }
        }
        image = decode_ktx2(&ktx, formats, addr, bias)?;
    } else if image.texture_descriptor.format.is_compressed()
        && !formats.supports(image.texture_descriptor.format)
    {
        image = decompress_bc(image)?;
    }
    Ok(CachedTexture {
        image,
        alpha,
        hit: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-ace/tests/fixtures")
            .join(name)
    }
    #[test]
    fn cache_preserves_authored_blocks_and_cpu_pixels_and_recovers_from_corruption() {
        let root = tempfile::tempdir().unwrap();
        let path = fixture("dxt1_4x4.ace");
        let before = std::fs::read(&path).unwrap();
        let first = load_in(
            &path,
            Some(root.path()),
            CompressedImageFormats::BC,
            Some(1),
            None,
        )
        .unwrap();
        assert!(!first.hit);
        let second = load_in(
            &path,
            Some(root.path()),
            CompressedImageFormats::BC,
            Some(1),
            None,
        )
        .unwrap();
        assert!(second.hit);
        assert_eq!(first.image.data, second.image.data);
        let cpu = load_in(
            &path,
            Some(root.path()),
            CompressedImageFormats::NONE,
            None,
            None,
        )
        .unwrap();
        assert!(cpu.hit);
        assert_eq!(
            cpu.image.data.unwrap(),
            openrailsrs_ace::read_ace(&path).unwrap().mip0
        );
        let (ktx, _) = cache_paths(root.path(), &path.canonicalize().unwrap());
        std::fs::write(&ktx, b"broken").unwrap();
        assert!(
            !load_in(
                &path,
                Some(root.path()),
                CompressedImageFormats::BC,
                None,
                None
            )
            .unwrap()
            .hit
        );
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn source_updates_invalidate_cached_textures_and_partial_alpha_is_exact() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.ace");
        let cache = root.path().join("cache");
        let mut bytes = b"@ACE".to_vec();
        for v in [4u32, 4, 0] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend([1, 4, 0, 0]);
        for _ in 0..16 {
            bytes.extend([29, 61, 121, 17]);
        }
        std::fs::write(&path, &bytes).unwrap();
        let first = load_in(
            &path,
            Some(&cache),
            CompressedImageFormats::NONE,
            None,
            None,
        )
        .unwrap();
        assert_eq!(first.image.data.unwrap(), bytes[20..]);
        bytes[20] = 30;
        std::fs::write(&path, &bytes).unwrap();
        let update = load_in(
            &path,
            Some(&cache),
            CompressedImageFormats::NONE,
            None,
            None,
        )
        .unwrap();
        assert!(!update.hit);
        assert_eq!(update.image.data.unwrap()[0], 30);
    }
    #[test]
    fn malformed_ktx2_is_rejected_before_allocation() {
        let ace = openrailsrs_ace::read_ace(fixture("dxt1_4x4.ace")).unwrap();
        let image = crate::gpu_textures::ace_image_for_formats(
            &ace,
            CompressedImageFormats::BC,
            None,
            None,
        );
        let mut bytes = encode_ktx2(&image).unwrap();
        bytes[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_ktx2(&bytes, CompressedImageFormats::BC, None, None).is_err());
    }

    fn rgb8_ktx2(payload_len: usize) -> Vec<u8> {
        let header = ktx2::Header {
            format: Some(ktx2::Format::R8G8B8_SRGB),
            type_size: 1,
            pixel_width: 4,
            pixel_height: 4,
            pixel_depth: 0,
            layer_count: 0,
            face_count: 1,
            level_count: 1,
            supercompression_scheme: None,
            index: ktx2::Index {
                dfd_byte_offset: 104,
                dfd_byte_length: 4,
                kvd_byte_offset: 0,
                kvd_byte_length: 0,
                sgd_byte_offset: 0,
                sgd_byte_length: 0,
            },
        };
        let mut bytes = header.as_bytes().to_vec();
        bytes.extend(
            ktx2::LevelIndex {
                byte_offset: 108,
                byte_length: payload_len as u64,
                uncompressed_byte_length: payload_len as u64,
            }
            .as_bytes(),
        );
        bytes.extend(4u32.to_le_bytes());
        bytes.resize(108 + payload_len, 53);
        bytes
    }

    #[test]
    fn rgb8_transcoding_validates_pixel_lengths_before_bevy() {
        // A 16-byte 4x4 payload is a BC/UASTC block, not sixteen RGB pixels.
        assert!(decode_ktx2(&rgb8_ktx2(16), CompressedImageFormats::BC, None, None).is_err());
        let image = decode_ktx2(&rgb8_ktx2(48), CompressedImageFormats::NONE, None, None).unwrap();
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(image.data.unwrap(), [53, 53, 53, 255].repeat(16));
    }

    #[test]
    fn undefined_format_without_a_valid_uastc_descriptor_is_rejected() {
        let mut bytes = rgb8_ktx2(48);
        bytes[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode_ktx2(&bytes, CompressedImageFormats::BC, None, None).is_err());
    }

    #[test]
    fn authored_ktx2_and_dds_keep_mips_alpha_and_cpu_fallback() {
        let root = tempfile::tempdir().unwrap();
        let ace = openrailsrs_ace::read_ace(fixture("dxt1_4x4.ace")).unwrap();
        let authored = crate::gpu_textures::ace_image_for_formats(
            &ace,
            CompressedImageFormats::BC,
            None,
            None,
        );
        let ktx = root.path().join("native.ktx2");
        let dds = root.path().join("native.dds");
        std::fs::write(&ktx, encode_ktx2(&authored).unwrap()).unwrap();
        std::fs::write(&dds, ace.to_dds().unwrap()).unwrap();
        for source in [&ktx, &dds] {
            for support in [CompressedImageFormats::BC, CompressedImageFormats::NONE] {
                let loaded = load_in(
                    source,
                    Some(&root.path().join("cache")),
                    support,
                    Some(3),
                    None,
                )
                .unwrap();
                let expected =
                    crate::gpu_textures::ace_image_for_formats(&ace, support, Some(3), None);
                assert_eq!(loaded.image.data, expected.data);
                assert_eq!(
                    loaded.image.texture_descriptor.mip_level_count,
                    expected.texture_descriptor.mip_level_count
                );
                assert_eq!(
                    loaded.image.texture_descriptor.format,
                    expected.texture_descriptor.format
                );
                assert_eq!(
                    loaded.image.texture_descriptor.dimension,
                    bevy::render::render_resource::TextureDimension::D2
                );
            }
        }
        // Native KTX2 is an author resource, not a generated cache entry.
        assert!(
            !load_in(&ktx, None, CompressedImageFormats::BC, None, None)
                .unwrap()
                .hit
        );
    }

    #[test]
    fn zstd_inflation_cannot_exceed_declared_mip_dimensions() {
        let ace = openrailsrs_ace::read_ace(fixture("dxt1_4x4.ace")).unwrap();
        let image = crate::gpu_textures::ace_image_for_formats(
            &ace,
            CompressedImageFormats::BC,
            None,
            None,
        );
        let mut bytes = encode_ktx2(&image).unwrap();
        let payload = zstd::bulk::compress(&vec![0; 1024 * 1024], 1).unwrap();
        let offset = bytes.len();
        bytes.extend(&payload);
        bytes[80..104].copy_from_slice(
            &ktx2::LevelIndex {
                byte_offset: offset as u64,
                byte_length: payload.len() as u64,
                uncompressed_byte_length: 8,
            }
            .as_bytes(),
        );
        assert!(decode_ktx2(&bytes, CompressedImageFormats::BC, None, None).is_err());
    }
}

//! Upload authored ACE/DDS block compression only when the render device supports it.
//! CPU fallback keeps authored mips and alpha, including tiny tail levels.
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
    render::render_resource::{TextureDimension, TextureFormat},
};
use openrailsrs_ace::AceFile;
use std::sync::atomic::{AtomicU32, Ordering};

static DEVICE_FORMATS: AtomicU32 = AtomicU32::new(0);

pub fn set_device_texture_formats(formats: CompressedImageFormats) {
    DEVICE_FORMATS.store(formats.bits(), Ordering::Relaxed);
}

pub fn device_texture_formats() -> CompressedImageFormats {
    if std::env::var("OPENRAILSRS_TEXTURE_UPLOAD").is_ok_and(|mode| mode == "rgba") {
        return CompressedImageFormats::NONE;
    }
    CompressedImageFormats::from_bits_truncate(DEVICE_FORMATS.load(Ordering::Relaxed))
}

pub fn ace_to_gpu_image_with_sampler(ace: &AceFile, addr: Option<i32>, bias: Option<f32>) -> Image {
    ace_image_for_formats(ace, device_texture_formats(), addr, bias)
}

pub fn ace_image_for_formats(
    ace: &AceFile,
    formats: CompressedImageFormats,
    addr: Option<i32>,
    bias: Option<f32>,
) -> Image {
    if formats.contains(CompressedImageFormats::BC)
        && !ace.compressed_mips.is_empty()
        && ace.width.is_multiple_of(4)
        && ace.height.is_multiple_of(4)
        && let Ok(bytes) = ace.to_dds()
        && let Ok(image) = decode_dds_for_formats(&bytes, formats, addr, bias)
    {
        return image;
    }
    crate::textures::ace_to_image_with_sampler(ace, addr, bias)
}

pub fn decode_dds_for_formats(
    bytes: &[u8],
    formats: CompressedImageFormats,
    addr: Option<i32>,
    bias: Option<f32>,
) -> Result<Image, String> {
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        return Err("invalid DDS header".into());
    }
    // Bevy rounds BC base extents up to full blocks. A source with a partial
    // block must use RGBA, otherwise the authored UV range includes padding.
    let width = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("invalid DDS dimensions".into());
    }
    let decode = |support| {
        Image::from_buffer(
            bytes,
            ImageType::Extension("dds"),
            support,
            true,
            ImageSampler::Default,
            RenderAssetUsages::default(),
        )
    };
    let mut image = match decode(formats) {
        Ok(image)
            if !image.texture_descriptor.format.is_compressed()
                || (width.is_multiple_of(4) && height.is_multiple_of(4)) =>
        {
            image
        }
        _ => {
            // This image is read on the CPU only. It never reaches RenderAssets
            // until converted to a universally supported RGBA texture.
            let raw = decode(CompressedImageFormats::all()).map_err(|e| e.to_string())?;
            let block = match raw.texture_descriptor.format {
                TextureFormat::Bc1RgbaUnorm | TextureFormat::Bc1RgbaUnormSrgb => {
                    texpresso::Format::Bc1
                }
                TextureFormat::Bc2RgbaUnorm | TextureFormat::Bc2RgbaUnormSrgb => {
                    texpresso::Format::Bc2
                }
                TextureFormat::Bc3RgbaUnorm | TextureFormat::Bc3RgbaUnormSrgb => {
                    texpresso::Format::Bc3
                }
                other => return Err(format!("DDS CPU fallback does not support {other:?}")),
            };
            if raw.texture_descriptor.dimension != TextureDimension::D2
                || raw.texture_descriptor.size.depth_or_array_layers != 1
            {
                return Err("DDS CPU fallback requires a single 2D texture".into());
            }
            let data = raw.data.as_deref().ok_or("DDS has no pixel data")?;
            let mut rgba = Vec::new();
            let mut offset = 0;
            for mip in 0..raw.texture_descriptor.mip_level_count {
                let w = (width >> mip).max(1) as usize;
                let h = (height >> mip).max(1) as usize;
                let end = offset + block.compressed_size(w, h);
                let blocks = data.get(offset..end).ok_or("truncated DDS mip chain")?;
                let start = rgba.len();
                rgba.resize(start + w * h * 4, 0);
                block.decompress(blocks, w, h, &mut rgba[start..]);
                offset = end;
            }
            let mut size = raw.texture_descriptor.size;
            size.width = width;
            size.height = height;
            let mut image = Image::new_uninit(
                size,
                TextureDimension::D2,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            image.texture_descriptor.mip_level_count = raw.texture_descriptor.mip_level_count;
            image.data = Some(rgba);
            image
        }
    };
    crate::textures::apply_msts_texture_sampler(&mut image, addr, bias);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_ace::AceFormat;
    #[test]
    fn partial_block_dds_preserves_logical_dimensions_and_uv_range() {
        for (width, height) in [(2u32, 2u32), (5, 7)] {
            let mut source = b"@ACE".to_vec();
            for v in [width, height, 3] {
                source.extend(v.to_le_bytes());
            }
            source.extend([1, 4, 0, 0]);
            source.extend(vec![
                0;
                texpresso::Format::Bc3
                    .compressed_size(width as usize, height as usize)
            ]);
            let ace = AceFile::read_bytes(&source).unwrap();
            let dds = ace.to_dds().unwrap();
            for support in [CompressedImageFormats::NONE, CompressedImageFormats::BC] {
                let image = decode_dds_for_formats(&dds, support, None, None).unwrap();
                assert_eq!((image.width(), image.height()), (width, height));
                assert_eq!(image.data.as_deref(), Some(ace.mip0.as_slice()));
                assert!(!image.texture_descriptor.format.is_compressed());
            }
        }
    }
    #[test]
    fn lossless_rgba_dds_keeps_colors_and_partial_alpha_on_every_backend() {
        let mut source = b"@ACE".to_vec();
        for v in [4u32, 4, 0] {
            source.extend(v.to_le_bytes());
        }
        source.extend([1, 4, 0, 0]);
        for alpha in [0, 17, 128, 255] {
            for _ in 0..4 {
                source.extend([23, 97, 184, alpha]);
            }
        }
        let ace = AceFile::read_bytes(&source).unwrap();
        let dds = ace.to_dds().unwrap();
        for support in [CompressedImageFormats::NONE, CompressedImageFormats::BC] {
            let image = decode_dds_for_formats(&dds, support, None, None).unwrap();
            assert_eq!(
                image.texture_descriptor.format,
                TextureFormat::Rgba8UnormSrgb
            );
            assert_eq!(image.data.as_deref(), Some(ace.mip0.as_slice()));
        }
    }
    #[test]
    fn dxt_gpu_and_cpu_uploads_preserve_blocks_mips_alpha_and_srgb() {
        for (format, code, bytes) in [
            (AceFormat::Dxt1, 1u32, 8usize),
            (AceFormat::Dxt3, 2, 16),
            (AceFormat::Dxt5, 3, 16),
        ] {
            let mut source = b"@ACE".to_vec();
            for v in [4u32, 4, code] {
                source.extend(v.to_le_bytes());
            }
            source.extend([1, 4, 0, 0]);
            source.extend(vec![0; bytes]);
            let mut ace = AceFile::read_bytes(&source).unwrap();
            // Authored tail mips reuse the last DXT block, as OR does.
            for size in [2, 1] {
                let mut mip = ace.mips[0].clone();
                mip.width = size;
                mip.height = size;
                mip.rgba.truncate((size * size * 4) as usize);
                ace.mips.push(mip);
                ace.compressed_mips.push(ace.compressed_mips[0].clone());
            }
            ace.mips_count = 3;
            let gpu = ace_image_for_formats(&ace, CompressedImageFormats::BC, Some(1), Some(2.));
            let cpu = ace_image_for_formats(&ace, CompressedImageFormats::NONE, Some(1), Some(2.));
            assert_eq!(ace.format, format);
            assert!(gpu.texture_descriptor.format.is_compressed());
            assert!(gpu.texture_descriptor.format.is_srgb());
            assert_eq!(gpu.texture_descriptor.mip_level_count, 3);
            assert_eq!(gpu.data.as_ref().unwrap().len(), bytes * 3);
            assert_eq!(cpu.data.as_ref().unwrap().len(), (16 + 4 + 1) * 4);
            let fallback = decode_dds_for_formats(
                &ace.to_dds().unwrap(),
                CompressedImageFormats::NONE,
                Some(1),
                Some(2.),
            )
            .unwrap();
            assert_eq!(fallback.data, cpu.data);
            assert_eq!(fallback.texture_descriptor.mip_level_count, 3);
        }
    }
}

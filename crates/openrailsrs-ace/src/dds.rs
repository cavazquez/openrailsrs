//! Lossless DDS export. Reuse authored DXT blocks; never recompress cab text,
//! alpha or albedo. Structured ACE stays RGBA8 rather than adding artifacts.
use crate::{AceError, AceFile, AceFormat, dxt_format};
use std::path::Path;

impl AceFile {
    pub fn to_dds(&self) -> Result<Vec<u8>, AceError> {
        if self.width == 0 || self.height == 0 || self.width > 8192 || self.height > 8192 {
            return Err(AceError::InvalidDimensions {
                width: self.width,
                height: self.height,
            });
        }
        let count = self.mips.len().max(1);
        let max_mips = 32 - self.width.max(self.height).leading_zeros();
        if count as u32 > max_mips {
            return Err(AceError::InvalidMipChain);
        }
        let compressed = self.format != AceFormat::Rgba8 && !self.compressed_mips.is_empty();
        if compressed && self.compressed_mips.len() != count {
            return Err(AceError::InvalidMipChain);
        }
        let mut payload = Vec::new();
        for level in 0..count {
            let (w, h) = ((self.width >> level).max(1), (self.height >> level).max(1));
            let rgba = if let Some(mip) = self.mips.get(level) {
                if mip.width != w || mip.height != h {
                    return Err(AceError::InvalidMipChain);
                }
                &mip.rgba
            } else {
                &self.mip0
            };
            if rgba.len() != (w * h * 4) as usize {
                return Err(AceError::InvalidMipChain);
            }
            if compressed {
                let blocks = &self.compressed_mips[level];
                if blocks.len() != dxt_format(self.format).compressed_size(w as usize, h as usize) {
                    return Err(AceError::InvalidMipChain);
                }
                payload.extend_from_slice(blocks);
            } else {
                payload.extend_from_slice(rgba);
            }
        }
        let mut dds = vec![0u8; 128];
        dds[..4].copy_from_slice(b"DDS ");
        put(&mut dds, 4, 124);
        put(
            &mut dds,
            8,
            0x1007 | if compressed { 0x80000 } else { 0x8 } | if count > 1 { 0x20000 } else { 0 },
        );
        put(&mut dds, 12, self.height);
        put(&mut dds, 16, self.width);
        put(
            &mut dds,
            20,
            if compressed {
                self.compressed_mips[0].len() as u32
            } else {
                self.width * 4
            },
        );
        put(&mut dds, 28, count as u32);
        put(&mut dds, 76, 32);
        if compressed {
            put(&mut dds, 80, 4);
            dds[84..88].copy_from_slice(match self.format {
                AceFormat::Dxt1 => b"DXT1",
                AceFormat::Dxt3 => b"DXT3",
                AceFormat::Dxt5 => b"DXT5",
                AceFormat::Rgba8 => unreachable!(),
            });
        } else {
            put(&mut dds, 80, 0x41); // RGB | alpha pixels
            put(&mut dds, 88, 32);
            for (offset, mask) in [(92, 0xff), (96, 0xff00), (100, 0xff0000), (104, 0xff000000)] {
                put(&mut dds, offset, mask);
            }
        }
        put(&mut dds, 108, 0x1000 | if count > 1 { 0x400008 } else { 0 });
        dds.extend(payload);
        Ok(dds)
    }
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Write a new DDS, refusing to overwrite an existing file or the source ACE.
pub fn write_dds(ace: &AceFile, path: impl AsRef<Path>) -> Result<(), AceError> {
    use std::io::Write;
    let data = ace.to_dds()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&data)?;
    Ok(())
}

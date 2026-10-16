//! Validate single 2D DDS inputs before handing them to a GPU/third-party decoder.
use crate::AceError;

#[derive(Clone, Copy, Debug)]
pub struct DdsInfo {
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub payload_bytes: usize,
}

pub fn validate_dds(bytes: &[u8], limit: usize) -> Result<DdsInfo, AceError> {
    if bytes.len() > limit {
        return Err(AceError::LimitExceeded { limit });
    }
    if bytes.len() < 128 {
        return Err(AceError::Truncated(bytes.len()));
    }
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("validated header"));
    if &bytes[..4] != b"DDS " || u32_at(4) != 124 || u32_at(76) != 32 {
        return Err(AceError::UnknownMagic { offset: 0 });
    }
    let (width, height) = (u32_at(16), u32_at(12));
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err(AceError::InvalidDimensions { width, height });
    }
    let mip_count = u32_at(28).max(1);
    if mip_count > 32 - width.max(height).leading_zeros()
        || u32_at(24) > 1
        || u32_at(112) & 0x20fe00 != 0
    {
        return Err(AceError::InvalidMipChain);
    }
    let mut offset = 128usize;
    let (block, bits) = if u32_at(80) & 4 != 0 {
        match &bytes[84..88] {
            b"DXT1" | b"ATI1" | b"BC4U" | b"BC4S" => (8, 0),
            b"DXT3" | b"DXT5" | b"ATI2" | b"BC5U" | b"BC5S" => (16, 0),
            b"DX10" => {
                if bytes.len() < 148 {
                    return Err(AceError::Truncated(bytes.len()));
                }
                if u32_at(132) != 3 || u32_at(140) != 1 || u32_at(136) & 4 != 0 {
                    return Err(AceError::InvalidMipChain);
                }
                offset = 148;
                match u32_at(128) {
                    71 | 72 | 80 | 81 => (8, 0),
                    74 | 75 | 77 | 78 | 83 | 84 | 95 | 96 | 98 | 99 => (16, 0),
                    28 | 29 | 87 | 91 => (0, 32),
                    61 => (0, 8),
                    n => return Err(AceError::UnsupportedFormat(n)),
                }
            }
            _ => return Err(AceError::UnsupportedFormat(u32_at(84))),
        }
    } else {
        let bits = u32_at(88);
        if ![8, 16, 24, 32].contains(&bits) {
            return Err(AceError::UnsupportedFormat(bits));
        }
        (0, bits)
    };
    let mut payload_bytes = 0usize;
    let mut decoded_bytes = 0usize;
    for level in 0..mip_count {
        let (w, h) = (
            (width >> level).max(1) as usize,
            (height >> level).max(1) as usize,
        );
        let n = if block > 0 {
            w.div_ceil(4) * h.div_ceil(4) * block
        } else {
            (w * bits as usize).div_ceil(8) * h
        };
        payload_bytes = payload_bytes
            .checked_add(n)
            .ok_or(AceError::InvalidMipChain)?;
        decoded_bytes = decoded_bytes
            .checked_add(w * h * 4)
            .ok_or(AceError::InvalidMipChain)?;
        if decoded_bytes > limit {
            return Err(AceError::LimitExceeded { limit });
        }
    }
    if offset + payload_bytes > bytes.len() {
        return Err(AceError::Truncated(bytes.len()));
    }
    Ok(DdsInfo {
        width,
        height,
        mip_count,
        payload_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = b"@ACE".to_vec();
        for n in [4u32, 4, 0] {
            b.extend(n.to_le_bytes());
        }
        b.extend([1, 4, 0, 0]);
        b.extend([0u8; 64]);
        crate::AceFile::read_bytes(&b).unwrap().to_dds().unwrap()
    }
    #[test]
    fn validates_real_payload_and_rejects_mips_before_decode() {
        let b = fixture();
        assert_eq!(validate_dds(&b, 1024).unwrap().mip_count, 1);
        assert!(validate_dds(&b[..b.len() - 1], 1024).is_err());
        let mut huge = b.clone();
        huge[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(validate_dds(&huge, 1024).is_err());
        let mut cube = b.clone();
        cube[112..116].copy_from_slice(&0x200u32.to_le_bytes());
        assert!(validate_dds(&cube, 1024).is_err());
        assert!(validate_dds(&b, 10).is_err());
    }
}

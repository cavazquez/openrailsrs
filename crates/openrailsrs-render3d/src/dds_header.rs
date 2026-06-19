use std::fs::File;
use std::io::Read;

pub enum DdsAlpha {
    NoneOr1Bit,
    Full,
}

pub fn dds_alpha_type(path: &std::path::Path) -> Option<DdsAlpha> {
    let mut f = File::open(path).ok()?;
    let mut header = [0u8; 128];
    f.read_exact(&mut header).ok()?;
    
    if &header[0..4] != b"DDS " {
        return None;
    }
    
    // Check if dwFlags in dwPixelFormat includes DDPF_FOURCC (0x4)
    // dwPixelFormat starts at offset 76.
    let pf_flags = u32::from_le_bytes(header[80..84].try_into().unwrap());
    if (pf_flags & 0x4) != 0 {
        let fourcc = &header[84..88];
        match fourcc {
            b"DXT1" => Some(DdsAlpha::NoneOr1Bit),
            b"DXT3" | b"DXT5" => Some(DdsAlpha::Full),
            _ => Some(DdsAlpha::Full), // BC7 etc
        }
    } else {
        // Uncompressed, maybe RGBA? Check DDPF_ALPHAPIXELS (0x1)
        if (pf_flags & 0x1) != 0 {
            Some(DdsAlpha::Full)
        } else {
            Some(DdsAlpha::NoneOr1Bit)
        }
    }
}

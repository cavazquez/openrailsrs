//! Shared terrtex CPU helpers (sampler wrap + base alpha sanitize).

use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::Image;

/// Repeat UVs like Open Rails terrain samplers.
pub fn set_terrain_repeat_sampler(image: &mut Image) {
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..Default::default()
    });
}

/// Fill transparent / chroma-key pixels in base TERRTEX so holes are mesh-driven.
pub fn sanitize_terrain_base_rgba(data: Option<&mut Vec<u8>>) {
    let Some(data) = data else {
        return;
    };
    if data.chunks_exact(4).all(|rgba| rgba[3] >= 250) {
        return;
    }

    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for rgba in data.chunks_exact(4) {
        if rgba[3] >= 250 && !looks_like_terrain_chroma_key(rgba) {
            sum[0] += rgba[0] as u64;
            sum[1] += rgba[1] as u64;
            sum[2] += rgba[2] as u64;
            count += 1;
        }
    }
    let fill = count
        .checked_sub(1)
        .map(|_| {
            [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
            ]
        })
        .unwrap_or([72, 107, 56]);

    for rgba in data.chunks_exact_mut(4) {
        if rgba[3] < 16 || looks_like_terrain_chroma_key(rgba) {
            rgba[0] = fill[0];
            rgba[1] = fill[1];
            rgba[2] = fill[2];
        }
        rgba[3] = 255;
    }
}

fn looks_like_terrain_chroma_key(rgba: &[u8]) -> bool {
    let [r, g, b, _] = [rgba[0], rgba[1], rgba[2], rgba[3]];
    b > 135 && g > 115 && r < 170 && b.saturating_sub(r) > 25 && b >= g
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::image::{ImageAddressMode, ImageSampler};

    #[test]
    fn sanitizer_fills_transparent_pixels() {
        let mut rgba = vec![
            10, 20, 30, 255, //
            200, 210, 220, 0,
        ];
        sanitize_terrain_base_rgba(Some(&mut rgba));
        assert_eq!(&rgba[0..4], &[10, 20, 30, 255]);
        assert_eq!(&rgba[4..8], &[10, 20, 30, 255]);
    }

    #[test]
    fn sampler_is_repeat() {
        let mut image = Image::default();
        set_terrain_repeat_sampler(&mut image);
        let ImageSampler::Descriptor(desc) = image.sampler else {
            panic!("expected explicit sampler");
        };
        assert_eq!(desc.address_mode_u, ImageAddressMode::Repeat);
        assert_eq!(desc.address_mode_v, ImageAddressMode::Repeat);
    }
}

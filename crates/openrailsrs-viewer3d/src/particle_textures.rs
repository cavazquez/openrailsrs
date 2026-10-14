//! Three bounded, reusable textures shared by CPU particles and Hanabi.
//!
//! Hanabi 0.19 caches material bind groups by Image ID without retiring that
//! cache when an image is removed. Keep these small images alive across scenes
//! instead of creating a new material key on every route transfer.
use bevy::{
    asset::{AssetId, RenderAssetUsages},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub const SHELTER_GRID: usize = 64;
const SHELTER_CPU: Handle<Image> =
    bevy::asset::uuid_handle!("657c23a8-ea46-4ae2-8ab6-4c5197fc8798");
const SHELTER_GPU: Handle<Image> =
    bevy::asset::uuid_handle!("f6f315f9-73b9-4aa2-8e35-de2770929985");
const PLUME: Handle<Image> = bevy::asset::uuid_handle!("2ce2f312-00b8-4a40-9934-bbeab4f1c249");

pub fn preserved_ids() -> [AssetId<Image>; 3] {
    [SHELTER_CPU.id(), SHELTER_GPU.id(), PLUME.id()]
}

pub fn shelter(images: &mut Assets<Image>) -> (Handle<Image>, Handle<Image>) {
    let size = Extent3d {
        width: SHELTER_GRID as u32,
        height: SHELTER_GRID as u32,
        depth_or_array_layers: 1,
    };
    if !images.contains(SHELTER_CPU.id()) {
        images
            .insert(
                SHELTER_CPU.id(),
                Image::new_fill(
                    size,
                    TextureDimension::D2,
                    &(-10000.0_f32).to_le_bytes(),
                    TextureFormat::R32Float,
                    RenderAssetUsages::default(),
                ),
            )
            .expect("The fixed shelter image ID is valid");
    }
    if !images.contains(SHELTER_GPU.id()) {
        images
            .insert(
                SHELTER_GPU.id(),
                Image::new_fill(
                    size,
                    TextureDimension::D2,
                    &[0, 0, 0, 255],
                    TextureFormat::Rgba8Unorm,
                    RenderAssetUsages::default(),
                ),
            )
            .expect("The fixed packed shelter image ID is valid");
    }
    (SHELTER_CPU.clone(), SHELTER_GPU.clone())
}

pub fn plume(images: &mut Assets<Image>) -> Handle<Image> {
    if !images.contains(PLUME.id()) {
        let mut pixels = vec![255u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let p = Vec2::new(x as f32 - 31.5, y as f32 - 31.5) / 31.5;
                let noise = (p.x * 15.0 + (p.y * 9.0).sin()).sin()
                    * (p.y * 13.0 + (p.x * 11.0).cos()).cos();
                let edge = (1.0 - p.length() + noise * 0.13).clamp(0.0, 1.0);
                pixels[(y * 64 + x) * 4 + 3] =
                    (edge.powf(1.3) * (0.65 + noise * 0.25) * 230.0) as u8;
            }
        }
        images
            .insert(
                PLUME.id(),
                Image::new(
                    Extent3d {
                        width: 64,
                        height: 64,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    pixels,
                    TextureFormat::Rgba8UnormSrgb,
                    // Retain the 16 KiB CPU copy so repeated scenes and device
                    // recovery reuse the same image without reconstructing it.
                    RenderAssetUsages::default(),
                ),
            )
            .expect("The fixed plume image ID is valid");
    }
    PLUME.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_transfers_keep_particle_texture_ids_and_refresh_shelter_bytes() {
        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        // Capture global state before the first scene creates these textures.
        crate::scene_cleanup::begin_loading(&mut world);
        let (cpu, gpu) = shelter(&mut world.resource_mut::<Assets<Image>>());
        let plume = plume(&mut world.resource_mut::<Assets<Image>>());
        let ids = [cpu.id(), gpu.id(), plume.id()];
        let initial_descriptor = world
            .resource::<Assets<Image>>()
            .get(&gpu)
            .unwrap()
            .texture_descriptor
            .clone();
        let original_plume = world
            .resource::<Assets<Image>>()
            .get(&plume)
            .unwrap()
            .data
            .clone();
        for cycle in 0..12 {
            let transient = world.resource_mut::<Assets<Image>>().add(Image::default());
            crate::scene_cleanup::begin_loading(&mut world);
            let images = &mut world.resource_mut::<Assets<Image>>();
            assert!(!images.contains(transient.id()));
            assert_eq!(images.len(), 3, "Only the bounded shared textures survive");
            let (again_cpu, again_gpu) = shelter(images);
            let again_plume = super::plume(images);
            assert_eq!([again_cpu.id(), again_gpu.id(), again_plume.id()], ids);
            images.get_mut(&cpu).unwrap().data.as_mut().unwrap()[..4]
                .copy_from_slice(&(cycle as f32).to_le_bytes());
            images.get_mut(&gpu).unwrap().data.as_mut().unwrap()[0] = cycle;
            assert_eq!(
                images.get(&gpu).unwrap().texture_descriptor,
                initial_descriptor
            );
            assert_eq!(
                images.get(&cpu).unwrap().data.as_ref().unwrap()[..4],
                (cycle as f32).to_le_bytes()
            );
            assert_eq!(images.get(&gpu).unwrap().data.as_ref().unwrap()[0], cycle);
            assert_eq!(images.get(&plume).unwrap().data, original_plume);
        }
    }
}

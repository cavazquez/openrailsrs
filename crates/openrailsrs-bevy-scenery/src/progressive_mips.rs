//! Stage authored mip tails, then refine visible textures within an upload budget.
//! No recompression, new mips or changes to author files. Oversized/unsupported
//! layouts keep the ordinary upload path rather than silently losing detail.
use bevy::{
    asset::{RenderAssetUsages, UntypedAssetId},
    camera::primitives::Aabb,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{TextureDimension, TextureUsages},
};

const UPLOAD_BUDGET: usize = 8 * 1024 * 1024;
const PENDING_BUDGET: usize = 64 * 1024 * 1024;
pub const AUTHORED_TEXTURE_LABEL: &str = "openrailsrs-authored-texture";

struct Pending {
    id: AssetId<Image>,
    full: Image,
    base: usize,
    age: u32,
}
#[derive(Resource, Default)]
struct PendingMips(Vec<Pending>);
#[derive(Resource, Default, Debug)]
pub struct MipStreamingStats {
    pub staged: u64,
    pub refined: u64,
    pub pending_bytes: usize,
    pub submitted_asset_bytes: usize,
    bindings: std::collections::HashSet<UntypedAssetId>,
}

pub struct ProgressiveMipsPlugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MipRefinement;

/// Replacing a GPU image view does not change its handle. Reprepare only the
/// materials that reference that image, including custom overlay textures.
pub fn refresh_material_bindings<M: Material>(
    changes: Res<MipStreamingStats>,
    materials: Option<ResMut<Assets<M>>>,
) {
    if changes.bindings.is_empty() {
        return;
    }
    let Some(mut materials) = materials else {
        return;
    };
    let affected = affected_materials(&materials, &changes.bindings);
    for id in affected {
        if let Some(material) = materials.get_mut(id) {
            // Merely obtaining AssetMut doesn't emit Modified in Bevy 0.19.
            let _ = material.into_inner();
        }
    }
}

fn affected_materials<M: Material>(
    materials: &Assets<M>,
    images: &std::collections::HashSet<UntypedAssetId>,
) -> Vec<AssetId<M>> {
    materials
        .iter()
        .filter_map(|(id, material)| {
            let mut affected = false;
            material.visit_dependencies(&mut |image| affected |= images.contains(&image));
            affected.then_some(id)
        })
        .collect()
}

impl Plugin for ProgressiveMipsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingMips>()
            .init_resource::<MipStreamingStats>()
            .add_systems(PostUpdate, stage_and_refine.in_set(MipRefinement))
            .add_systems(
                PostUpdate,
                (
                    refresh_material_bindings::<StandardMaterial>,
                    refresh_material_bindings::<crate::TerrainMaterial>,
                    refresh_material_bindings::<crate::OrTerrainMaterial>,
                    refresh_material_bindings::<crate::OrSceneryMaterial>,
                    refresh_material_bindings::<crate::OrCabMaterial>,
                    refresh_material_bindings::<crate::OrForestMaterial>,
                )
                    .after(MipRefinement),
            );
    }
}

/// Return a complete mip tail with the same authored bytes and texel encoding.
pub fn mip_tail(image: &Image, base: usize) -> Option<Image> {
    let d = &image.texture_descriptor;
    if d.dimension != TextureDimension::D2
        || d.size.width == 0
        || d.size.height == 0
        || d.size.depth_or_array_layers != 1
        || image.texture_view_descriptor.is_some()
        || base >= d.mip_level_count as usize
        || d.mip_level_count > 14
        || d.mip_level_count > 32 - d.size.width.max(d.size.height).leading_zeros()
    {
        return None;
    }
    let (bw, bh) = d.format.block_dimensions();
    let block = d.format.block_copy_size(None)? as usize;
    let data = image.data.as_deref()?;
    let mut offsets = vec![0usize];
    for mip in 0..d.mip_level_count {
        let w = (d.size.width >> mip).max(1);
        let h = (d.size.height >> mip).max(1);
        let n = w.div_ceil(bw) as usize * h.div_ceil(bh) as usize * block;
        offsets.push(offsets.last()?.checked_add(n)?);
    }
    if *offsets.last()? != data.len() {
        return None;
    }
    let mut size = d.size;
    size.width = (size.width >> base).max(1);
    size.height = (size.height >> base).max(1);
    if d.format.is_compressed()
        && (!size.width.is_multiple_of(bw) || !size.height.is_multiple_of(bh))
    {
        return None;
    }
    let mut result = Image::new_uninit(size, TextureDimension::D2, d.format, image.asset_usage);
    result.texture_descriptor = d.clone();
    result.texture_descriptor.size = size;
    result.texture_descriptor.mip_level_count -= base as u32;
    result.data_order = image.data_order;
    result.data = Some(data[offsets[base]..].to_vec());
    result.sampler = image.sampler.clone();
    if let ImageSampler::Descriptor(sampler) = &mut result.sampler {
        sampler.lod_min_clamp = (sampler.lod_min_clamp - base as f32).max(0.);
        sampler.lod_max_clamp = (sampler.lod_max_clamp - base as f32).max(sampler.lod_min_clamp);
    }
    Some(result)
}

type TextureUsers<'w, 's> = Query<
    'w,
    's,
    (
        &'static MeshMaterial3d<StandardMaterial>,
        &'static GlobalTransform,
        Option<&'static Aabb>,
        Option<&'static ViewVisibility>,
    ),
>;

fn stage_and_refine(
    mut events: MessageReader<AssetEvent<Image>>,
    mut images: ResMut<Assets<Image>>,
    mut pending: ResMut<PendingMips>,
    mut stats: ResMut<MipStreamingStats>,
    materials: Option<Res<Assets<StandardMaterial>>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    users: TextureUsers,
) {
    stats.bindings.clear();
    let added = events
        .read()
        .filter_map(|e| {
            if let AssetEvent::Added { id } = e {
                Some(*id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    stats.submitted_asset_bytes = 0;
    if std::env::var("OPENRAILSRS_PROGRESSIVE_MIPS")
        .is_ok_and(|v| matches!(v.as_str(), "0" | "false" | "off"))
    {
        return;
    }
    pending.0.retain(|p| images.contains(p.id));
    let mut retained = pending
        .0
        .iter()
        .map(|p| p.full.data.as_ref().map_or(0, Vec::len))
        .sum::<usize>();
    for id in added {
        if pending.0.len() >= 32 {
            break;
        }
        let Some(full) = images.get(id) else {
            continue;
        };
        let size = full.data.as_ref().map_or(0, Vec::len);
        if full.texture_descriptor.label != Some(AUTHORED_TEXTURE_LABEL)
            || !full.asset_usage.contains(RenderAssetUsages::MAIN_WORLD)
            || full
                .texture_descriptor
                .usage
                .contains(TextureUsages::RENDER_ATTACHMENT)
            || !(256 * 1024..=UPLOAD_BUDGET).contains(&size)
            || full.texture_descriptor.mip_level_count < 3
            || retained + size > PENDING_BUDGET
        {
            continue;
        }
        let mut base = 0;
        while (full.width().max(full.height()) >> base) > 256
            && base + 1 < full.texture_descriptor.mip_level_count as usize
        {
            base += 1;
        }
        let Some(tail) = mip_tail(full, base) else {
            continue;
        };
        if base == 0 {
            continue;
        }
        let full = std::mem::replace(&mut *images.get_mut(id).expect("existing image"), tail);
        stats.bindings.insert(id.untyped());
        pending.0.push(Pending {
            id,
            full,
            base,
            age: 0,
        });
        retained += size;
        stats.staged += 1;
    }
    let eye = cameras
        .iter()
        .filter(|(c, _)| c.is_active)
        .min_by_key(|(c, _)| c.order)
        .map(|(_, t)| t.translation());
    let mut priority = std::collections::HashMap::<AssetId<Image>, f32>::new();
    if let (Some(materials), Some(eye)) = (materials, eye) {
        for (handle, transform, bounds, visibility) in &users {
            if visibility.is_some_and(|v| !v.get()) {
                continue;
            }
            let Some(image) = materials
                .get(handle)
                .and_then(|m| m.base_color_texture.as_ref())
            else {
                continue;
            };
            let center = bounds.map_or(transform.translation(), |b| {
                transform.transform_point(Vec3::from(b.center))
            });
            let radius = bounds.map_or(8., |b| {
                Vec3::from(b.half_extents).length()
                    * transform
                        .to_scale_rotation_translation()
                        .0
                        .abs()
                        .max_element()
            });
            let pixels = (1200. * radius / eye.distance(center).max(0.1)).clamp(1., 8192.);
            priority
                .entry(image.id())
                .and_modify(|p| *p = p.max(pixels))
                .or_insert(pixels);
        }
    }
    for p in &mut pending.0 {
        p.age = p.age.saturating_add(1);
    }
    pending.0.sort_by(|a, b| {
        priority
            .get(&b.id)
            .copied()
            .unwrap_or(0.)
            .total_cmp(&priority.get(&a.id).copied().unwrap_or(0.))
            .then(b.age.cmp(&a.age))
    });
    let mut budget = UPLOAD_BUDGET;
    for p in &mut pending.0 {
        // Give the coarse image at least one extraction before refining it.
        if p.age <= 1 {
            continue;
        }
        let pixels = priority.get(&p.id).copied().unwrap_or(0.);
        let wanted = if p.age > 120 || eye.is_none() {
            0
        } else if pixels > 0. {
            (p.full.width().max(p.full.height()) as f32 / pixels)
                .max(1.)
                .log2()
                .floor() as usize
        } else {
            p.base
        };
        if p.base <= wanted {
            continue;
        }
        let next = p.base - 1;
        let Some(tail) = mip_tail(&p.full, next) else {
            p.base = 0;
            continue;
        };
        let bytes = tail.data.as_ref().map_or(0, Vec::len);
        if bytes > budget {
            continue;
        }
        if let Some(mut image) = images.get_mut(p.id) {
            *image = tail;
            stats.bindings.insert(p.id.untyped());
            p.base = next;
            budget -= bytes;
            stats.submitted_asset_bytes += bytes;
            stats.refined += 1;
        }
    }
    pending.0.retain(|p| p.base > 0);
    stats.pending_bytes = pending
        .0
        .iter()
        .map(|p| p.full.data.as_ref().map_or(0, Vec::len))
        .sum();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureFormat};
    #[test]
    fn refinement_invalidates_only_materials_using_the_replaced_image_view() {
        let mut images = Assets::<Image>::default();
        let changing = images.add(Image::default());
        let stable = images.add(Image::default());
        let mut standard = Assets::<StandardMaterial>::default();
        let used = standard.add(StandardMaterial {
            base_color_texture: Some(changing.clone()),
            ..default()
        });
        standard.add(StandardMaterial {
            base_color_texture: Some(stable.clone()),
            ..default()
        });
        let changed = std::collections::HashSet::from([changing.id().untyped()]);
        assert_eq!(affected_materials(&standard, &changed), vec![used.id()]);
        let mut terrain = Assets::<crate::TerrainMaterial>::default();
        let used = terrain.add(crate::TerrainMaterial {
            base_texture: stable.clone(),
            overlay_texture: changing,
            overlay_scale: 32.,
            surface_weather: Vec2::ZERO,
            enhancement: Vec4::ZERO,
            snow_tracks: default(),
        });
        terrain.add(crate::TerrainMaterial {
            base_texture: stable.clone(),
            overlay_texture: stable,
            overlay_scale: 32.,
            surface_weather: Vec2::ZERO,
            enhancement: Vec4::ZERO,
            snow_tracks: default(),
        });
        assert_eq!(affected_materials(&terrain, &changed), vec![used.id()]);
    }
    #[test]
    fn rgba_and_bc_mip_tails_preserve_authored_bytes_and_encoding() {
        for format in [
            TextureFormat::Rgba8UnormSrgb,
            TextureFormat::Bc1RgbaUnormSrgb,
            TextureFormat::Bc3RgbaUnormSrgb,
        ] {
            let mut full = Image::new_uninit(
                Extent3d {
                    width: 16,
                    height: 16,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                format,
                RenderAssetUsages::default(),
            );
            full.texture_descriptor.mip_level_count = 5;
            let (bw, bh) = format.block_dimensions();
            let block = format.block_copy_size(None).unwrap() as usize;
            let mut data = vec![];
            let mut offset = 0;
            for mip in 0..5 {
                if mip == 1 {
                    offset = data.len();
                }
                let side = (16u32 >> mip).max(1);
                data.extend(vec![
                    mip as u8;
                    side.div_ceil(bw) as usize
                        * side.div_ceil(bh) as usize
                        * block
                ]);
            }
            full.data = Some(data.clone());
            let tail = mip_tail(&full, 1).unwrap();
            assert_eq!(tail.texture_descriptor.format, format);
            assert_eq!(tail.width(), 8);
            assert_eq!(tail.texture_descriptor.mip_level_count, 4);
            assert_eq!(tail.data.unwrap(), data[offset..]);
        }
    }
}

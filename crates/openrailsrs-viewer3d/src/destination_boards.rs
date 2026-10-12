//! Dynamic texture on explicitly named destination meshes; no original file edits.
use bevy::{asset::RenderAssetUsages, image::ImageSampler, light::NotShadowCaster, prelude::*};
use openrailsrs_formats::ShapeFile;
#[derive(Component)]
pub struct DestinationBoard(pub usize);
#[derive(Default)]
struct Display {
    text: String,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
}
#[derive(Resource, Default)]
pub struct Boards(std::collections::HashMap<usize, Display>);
impl Boards {
    pub fn report(&self) -> Vec<String> {
        let mut values = self.0.iter().collect::<Vec<_>>();
        values.sort_by_key(|(id, _)| **id);
        values.into_iter().map(|(_, d)| d.text.clone()).collect()
    }
}
pub fn compatible(shape: &ShapeFile, prim: i32) -> bool {
    let matrix = crate::rolling_stock_anim::matrix_idx_for_prim_state(shape, prim);
    let name = crate::rolling_stock_anim::matrix_name(shape, matrix)
        .trim()
        .to_ascii_uppercase();
    [
        "DESTINATION",
        "DESTBOARD",
        "DESTINATIONBOARD",
        "HEADCODE",
        "DEST_DISPLAY",
    ]
    .iter()
    .any(|s| {
        name == *s
            || name.strip_prefix(s).is_some_and(|suffix| {
                suffix.starts_with('_') || suffix.chars().all(|c| c.is_ascii_digit())
            })
    })
}
pub fn mark(entity: &mut EntityCommands, shape: &ShapeFile, prim: i32, track: usize) {
    if compatible(shape, prim) {
        entity.insert((DestinationBoard(track), NotShadowCaster));
    }
}
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' | 'Á' | 'À' | 'Ä' | 'Â' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' | 'É' | 'È' | 'Ë' | 'Ê' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' | 'Í' | 'Ï' | 'Î' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' | 'Ñ' => [17, 25, 21, 19, 17, 17, 17],
        'O' | 'Ó' | 'Ö' | 'Ô' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' | 'Ú' | 'Ü' | 'Û' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '→' => [0, 4, 2, 31, 2, 4, 0],
        '←' => [0, 4, 8, 31, 8, 4, 0],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        ' ' => [0; 7],
        _ => [31, 17, 1, 6, 4, 0, 4],
    }
}
pub fn texture(text: &str) -> Image {
    let mut rgba = image::RgbaImage::from_pixel(512, 128, image::Rgba([5, 9, 6, 255]));
    for (line, text) in text.lines().take(2).enumerate() {
        let chars = text.to_uppercase().chars().take(40).collect::<Vec<_>>();
        let scale = 2u32;
        let start = (512 - (chars.len() as u32 * 6 * scale)) / 2;
        for (column, c) in chars.iter().enumerate() {
            for (row, bits) in glyph(*c).iter().enumerate() {
                for x in 0..5 {
                    if bits & (1 << (4 - x)) == 0 {
                        continue;
                    }
                    for dx in 0..scale {
                        for dy in 0..scale {
                            rgba.put_pixel(
                                start + column as u32 * 6 * scale + x * scale + dx,
                                20 + line as u32 * 55 + row as u32 * scale + dy,
                                image::Rgba([242, 193, 55, 255]),
                            );
                        }
                    }
                }
            }
        }
    }
    let mut image = Image::from_dynamic(
        image::DynamicImage::ImageRgba8(rgba),
        true,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}
pub fn update(
    mut commands: Commands,
    live: Option<Res<crate::live::LiveDrive>>,
    boards: Query<(
        Entity,
        &DestinationBoard,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
    mut cache: ResMut<Boards>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(live) = live else {
        return;
    };
    for (entity, board, current) in &boards {
        let session = if board.0 == 0 {
            Some(&live.session)
        } else {
            live.traffic.services.get(board.0 - 1).map(|s| &s.session)
        };
        let Some(session) = session else {
            continue;
        };
        let text = session.destination_display();
        let display = cache.0.entry(board.0).or_default();
        if display.text != text || display.image == Handle::default() {
            let image = texture(&text);
            if let Some(mut existing) = images.get_mut(&display.image) {
                *existing = image;
            } else {
                display.image = images.add(image);
                display.material = materials.add(StandardMaterial {
                    base_color_texture: Some(display.image.clone()),
                    unlit: true,
                    emissive: LinearRgba::new(0.15, 0.10, 0.01, 1.),
                    ..default()
                });
            }
            display.text = text;
        }
        if current.is_none_or(|m| m.0 != display.material) {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(display.material.clone()));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_service_changes_pixels_but_render_is_bounded() {
        let a = texture("MARYLEBONE\nSERVICE 1");
        let b = texture("GERRARDS CROSS\nSERVICE 2");
        assert_eq!(a.width(), 512);
        assert_eq!(a.height(), 128);
        assert_ne!(a.data, b.data);
        assert_eq!(a.data.as_ref().unwrap().len(), 512 * 128 * 4);
        assert_eq!(texture("x").data, texture("X").data);
    }
}

//! Native 3D cab instruments (#157): OR `ThreeDimCabGaugeNative` + `ThreeDimCabDigit`.
//!
//! Quads parented to `CabInteriorRoot` at the CVF matrix pivot. Gauge = solid colour bar;
//! Digit = ACE 4×4 atlas (`speed.ace` / custom).

use std::path::{Path, PathBuf};

use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use openrailsrs_ace::read_ace;
use openrailsrs_formats::{CabControl, CabDigitalParams, CabGaugeParams, ControlType};

use crate::cab_cvf::{
    CabCvfRuntime, CabCvfState, MatrixDriver, cvf_control_at_order, dial_control_value,
    digital_control_value, static_matrix_transform,
};
use crate::cab_view::{CabInteriorMarker, CabInteriorRoot};
use crate::camera::CameraFollowMode;
use crate::live::LiveDrive;
use crate::shapes::{ace_to_scenery_image, resolve_texture_path_in_dirs};
use crate::viewer_log;

const GAUGE_Z: f32 = 0.002;
const DIGIT_Z: f32 = 0.01;
const MAX_DIGITS_DEFAULT: usize = 6;

/// Solid gauge bar (OR `ThreeDimCabGaugeNative`).
#[derive(Component, Clone, Debug)]
pub struct CabNativeGauge {
    pub matrix_idx: usize,
    pub control: ControlType,
    pub order: u32,
    pub width_m: f32,
    pub max_len_m: f32,
    pub gauge: CabGaugeParams,
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

/// ACE font digits (OR `ThreeDimCabDigit`).
#[derive(Component, Clone, Debug)]
pub struct CabNativeDigit {
    pub matrix_idx: usize,
    pub control: ControlType,
    pub order: u32,
    pub size_m: f32,
    pub max_digits: usize,
    pub digital: CabDigitalParams,
    pub mesh: Handle<Mesh>,
}

/// Spawn Digit/GaugeNative quads under an existing cab interior root.
pub fn spawn_cab_native_instruments(
    root: &mut ChildSpawnerCommands<'_>,
    runtime: &CabCvfRuntime,
    cab_shape: &Path,
    route_dir: &Path,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let mut gauge_n = 0u32;
    let mut digit_n = 0u32;
    for (matrix_idx, driver) in &runtime.matrix_drivers {
        match driver {
            MatrixDriver::GaugeNative {
                control,
                order,
                width_mm,
                length_mm,
            } => {
                let Some(CabControl::Gauge { gauge, .. }) =
                    cvf_control_at_order(&runtime.cvf, control, *order)
                else {
                    continue;
                };
                if gauge.is_pointer() {
                    continue;
                }
                let width_m = (*width_mm / 1000.0).max(1e-4);
                let max_len_m = (*length_mm / 1000.0).max(1e-4);
                let mesh = meshes.add(gauge_mesh(width_m, max_len_m, 0.0, gauge));
                let rgba = gauge.positive_colour.unwrap_or([1.0, 1.0, 0.0, 1.0]);
                let material = materials.add(StandardMaterial {
                    base_color: Color::srgba(rgba[1], rgba[2], rgba[3], rgba[0]),
                    unlit: true,
                    alpha_mode: AlphaMode::Opaque,
                    ..default()
                });
                let transform = static_matrix_transform(&runtime.shape, *matrix_idx);
                root.spawn((
                    CabInteriorMarker,
                    CabNativeGauge {
                        matrix_idx: *matrix_idx,
                        control: control.clone(),
                        order: *order,
                        width_m,
                        max_len_m,
                        gauge: gauge.clone(),
                        mesh: mesh.clone(),
                        material: material.clone(),
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    transform,
                    Visibility::Visible,
                    NotShadowCaster,
                    NotShadowReceiver,
                    Name::new(format!("cab:native:gauge:{matrix_idx}")),
                ));
                gauge_n += 1;
            }
            MatrixDriver::Digit {
                control,
                order,
                height_mm,
                font_ace,
            } => {
                let digital = match cvf_control_at_order(&runtime.cvf, control, *order) {
                    Some(CabControl::Digital { digital, .. }) => digital.clone(),
                    _ => CabDigitalParams::default(),
                };
                let size_m = (*height_mm / 1000.0).max(1e-4);
                let max_digits =
                    if control.as_str().eq_ignore_ascii_case("CLOCK") && digital.accuracy > 0 {
                        8
                    } else {
                        MAX_DIGITS_DEFAULT
                    };
                let Some(image) =
                    load_digit_font_image(cab_shape, route_dir, control, font_ace.as_deref())
                else {
                    viewer_log!(
                        "openrailsrs-viewer3d: cab Digit matrix {matrix_idx} — font ACE missing"
                    );
                    continue;
                };
                let texture = images.add(image);
                let material = materials.add(StandardMaterial {
                    base_color_texture: Some(texture),
                    base_color: Color::WHITE,
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                });
                let mesh = meshes.add(digit_mesh(size_m, max_digits, &"0".repeat(max_digits)));
                let transform = static_matrix_transform(&runtime.shape, *matrix_idx);
                root.spawn((
                    CabInteriorMarker,
                    CabNativeDigit {
                        matrix_idx: *matrix_idx,
                        control: control.clone(),
                        order: *order,
                        size_m,
                        max_digits,
                        digital,
                        mesh: mesh.clone(),
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    transform,
                    Visibility::Visible,
                    NotShadowCaster,
                    NotShadowReceiver,
                    Name::new(format!("cab:native:digit:{matrix_idx}")),
                ));
                digit_n += 1;
            }
            _ => {}
        }
    }
    if gauge_n + digit_n > 0 {
        viewer_log!(
            "openrailsrs-viewer3d: cab native instruments — {gauge_n} gauge(s), {digit_n} digit(s)"
        );
    }
}

/// Update gauge / digit geometry from live telemetry.
pub fn update_cab_native_instruments(
    follow: Res<CameraFollowMode>,
    live: Option<Res<LiveDrive>>,
    cvf_state: Option<Res<CabCvfState>>,
    interior: Query<Entity, With<CabInteriorRoot>>,
    mut gauges: Query<(
        &CabNativeGauge,
        &mut Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    digits: Query<&CabNativeDigit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if *follow != CameraFollowMode::DriverCam {
        return;
    }
    let Some(live) = live else {
        return;
    };
    let Some(cvf_state) = cvf_state else {
        return;
    };
    let Some(runtime) = cvf_state.runtime.as_ref() else {
        return;
    };
    if interior.is_empty() {
        return;
    }
    let tel = live.session.cab_telemetry();

    for (gauge, mesh3d, mat3d) in &mut gauges {
        let reading = gauge_control_value(&gauge.control, &gauge.gauge, &tel);
        let fraction = gauge.gauge.range_fraction(reading, true) as f32;
        if let Some(mut mesh) = meshes.get_mut(&mesh3d.0) {
            *mesh = gauge_mesh(gauge.width_m, gauge.max_len_m, fraction, &gauge.gauge);
        }
        let rgba = if fraction < 0.0 {
            gauge
                .gauge
                .negative_colour
                .or(gauge.gauge.positive_colour)
                .unwrap_or([1.0, 1.0, 0.0, 1.0])
        } else {
            gauge.gauge.positive_colour.unwrap_or([1.0, 1.0, 0.0, 1.0])
        };
        if let Some(mut mat) = materials.get_mut(&mat3d.0) {
            mat.base_color = Color::srgba(rgba[1], rgba[2], rgba[3], rgba[0]);
        }
        let _ = runtime; // keep matrix drivers authoritative at spawn
    }

    for digit in &digits {
        let reading = digital_control_value(&digit.control, &digit.digital, &tel);
        let text = format_3d_digits(&digit.digital, reading, digit.max_digits);
        if let Some(mut mesh) = meshes.get_mut(&digit.mesh) {
            *mesh = digit_mesh(digit.size_m, digit.max_digits, &text);
        }
    }
}

fn gauge_control_value(
    control: &ControlType,
    gauge: &CabGaugeParams,
    tel: &openrailsrs_sim::CabTelemetry,
) -> f64 {
    let dial = openrailsrs_formats::CabDialParams {
        scale_min: gauge.scale_min,
        scale_max: gauge.scale_max,
        units: gauge.units.clone(),
        ..Default::default()
    };
    dial_control_value(control, &dial, tel)
}

/// OR gauge bar corners in matrix-local space (Z = [`GAUGE_Z`]).
pub fn gauge_bar_corners(
    width_m: f32,
    max_len_m: f32,
    fraction: f32,
    orientation: i32,
    direction: i32,
) -> [Vec3; 4] {
    let len = max_len_m * fraction;
    let abs_len = len.abs();
    let z = GAUGE_Z;
    if orientation == 0 {
        if (direction == 0) ^ (len < 0.0) {
            // grow +X
            [
                Vec3::new(0.0, 0.0, z),
                Vec3::new(0.0, width_m, z),
                Vec3::new(abs_len, width_m, z),
                Vec3::new(abs_len, 0.0, z),
            ]
        } else {
            // grow −X — OR order v1,v2,v3,v4
            [
                Vec3::new(0.0, 0.0, z),
                Vec3::new(-abs_len, 0.0, z),
                Vec3::new(-abs_len, width_m, z),
                Vec3::new(0.0, width_m, z),
            ]
        }
    } else if (direction == 1) ^ (len < 0.0) {
        // grow +Y
        [
            Vec3::new(0.0, 0.0, z),
            Vec3::new(0.0, abs_len, z),
            Vec3::new(width_m, abs_len, z),
            Vec3::new(width_m, 0.0, z),
        ]
    } else {
        // grow −Y
        [
            Vec3::new(0.0, 0.0, z),
            Vec3::new(width_m, 0.0, z),
            Vec3::new(width_m, -abs_len, z),
            Vec3::new(0.0, -abs_len, z),
        ]
    }
}

fn gauge_mesh(width_m: f32, max_len_m: f32, fraction: f32, gauge: &CabGaugeParams) -> Mesh {
    let corners = gauge_bar_corners(
        width_m,
        max_len_m,
        fraction,
        gauge.orientation,
        gauge.direction,
    );
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    let positions: Vec<[f32; 3]> = corners.iter().map(|v| [v.x, v.y, v.z]).collect();
    let normals = vec![[0.0, 0.0, -1.0]; 4];
    let uvs = vec![[0.0, 0.0]; 4];
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    // OR ctor: 0,1,2 / 0,2,3
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// Atlas UV bottom-left of a 4×4 cell (OR `GetTextureCoordX/Y`).
pub fn digit_texture_coord(c: char) -> (f32, f32) {
    let x: f32 = match c {
        '.' => 0.0,
        ':' => 0.5,
        ' ' => 0.75,
        '-' => 0.25,
        'a' | 'A' => 0.5,
        'p' | 'P' => 0.75,
        d if d.is_ascii_digit() => ((d as u8 - b'0') as f32 % 4.0) * 0.25,
        _ => 0.0,
    };
    let y: f32 = match c {
        '0' | '1' | '2' | '3' => 0.25,
        '4' | '5' | '6' | '7' => 0.5,
        '8' | '9' | ':' | ' ' => 0.75,
        _ => 1.0,
    };
    (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
}

/// Format + Cab3D justification padding (OR `Get3DDigits` subset, no clock/alert).
pub fn format_3d_digits(digital: &CabDigitalParams, value: f64, max_digits: usize) -> String {
    let mut text = digital.format_value(value);
    if text.len() > max_digits {
        text = text[text.len() - max_digits..].to_string();
    }
    let leading = match digital.justification {
        // Cab3D: 4=center, 5=left, 6=right; MSTS 1–3 → left in 3D
        4 => max_digits.saturating_sub(text.len()).div_ceil(2),
        6 => max_digits.saturating_sub(text.len()),
        _ => 0,
    };
    format!("{}{text}", " ".repeat(leading))
}

fn digit_mesh(size_m: f32, max_digits: usize, text: &str) -> Mesh {
    let mut positions = Vec::with_capacity(max_digits * 4);
    let mut normals = Vec::with_capacity(max_digits * 4);
    let mut uvs = Vec::with_capacity(max_digits * 4);
    let mut indices = Vec::with_capacity(max_digits * 6);
    let mut offset_x = 0.0f32;
    let offset_y = -size_m;
    let chars: Vec<char> = text.chars().take(max_digits).collect();
    for ch in chars {
        let (tx, ty) = digit_texture_coord(ch);
        let base = positions.len() as u32;
        // left-bottom, right-bottom, right-top, left-top (OR)
        positions.push([offset_x, offset_y, DIGIT_Z]);
        positions.push([offset_x + size_m, offset_y, DIGIT_Z]);
        positions.push([offset_x + size_m, offset_y + size_m, DIGIT_Z]);
        positions.push([offset_x, offset_y + size_m, DIGIT_Z]);
        for _ in 0..4 {
            normals.push([0.0, 0.0, -1.0]);
        }
        uvs.push([tx, ty]);
        uvs.push([tx + 0.25, ty]);
        uvs.push([tx + 0.25, ty - 0.25]);
        uvs.push([tx, ty - 0.25]);
        // OR: 0,2,1 / 0,3,2
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        offset_x += size_m * 0.8;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn load_digit_font_image(
    cab_shape: &Path,
    route_dir: &Path,
    control: &ControlType,
    font_ace: Option<&str>,
) -> Option<Image> {
    let image_name = digit_font_file_name(control, font_ace);
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = cab_shape.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.push(route_dir.join("GLOBAL/TEXTURES"));
    dirs.push(route_dir.join("../GLOBAL/TEXTURES"));
    if let Some(content) = std::env::var_os("OPENRAILSRS_MSTS_CONTENT") {
        let c = PathBuf::from(content);
        dirs.push(c.join("GLOBAL/TEXTURES"));
        dirs.push(c.join("Chiltern/GLOBAL/TEXTURES"));
    }
    // Repo fixture + OR Addons (dev).
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/fixtures/cab"));
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../openrails/Addons"));
    if let Some(addons) = std::env::var_os("OPENRAILSRS_OR_ADDONS") {
        dirs.push(PathBuf::from(addons));
    }
    let refs: Vec<&Path> = dirs.iter().map(|p| p.as_path()).collect();
    let path = resolve_texture_path_in_dirs(&refs, &image_name)?;
    let ace = read_ace(&path).ok()?;
    Some(ace_to_scenery_image(&ace).0)
}

fn digit_font_file_name(control: &ControlType, font_ace: Option<&str>) -> String {
    if let Some(name) = font_ace {
        let upper = name.to_ascii_uppercase();
        if upper.ends_with(".ACE") {
            return upper;
        }
        return format!("{upper}.ACE");
    }
    let name = match control.as_str().to_ascii_uppercase().as_str() {
        "CLOCK" => "clock.ace",
        "SPEEDLIMIT" | "SPEEDLIM_DISPLAY" => "speedlim.ace",
        _ => "speed.ace",
    };
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::ShapeFile;

    #[test]
    fn gauge_horizontal_grows_positive_x() {
        let c = gauge_bar_corners(0.01, 0.1, 0.5, 0, 0);
        assert!((c[3].x - 0.05).abs() < 1e-5);
        assert!(c[3].x > 0.0);
    }

    #[test]
    fn gauge_vertical_grows_positive_y() {
        let c = gauge_bar_corners(0.01, 0.1, 1.0, 1, 1);
        assert!((c[1].y - 0.1).abs() < 1e-5);
    }

    #[test]
    fn digit_uv_map_matches_or_atlas() {
        assert_eq!(digit_texture_coord('0'), (0.0, 0.25));
        assert_eq!(digit_texture_coord('5'), (0.25, 0.5));
        assert_eq!(digit_texture_coord('9'), (0.25, 0.75));
        assert_eq!(digit_texture_coord(':'), (0.5, 0.75));
        assert_eq!(digit_texture_coord('.'), (0.0, 1.0));
        assert_eq!(digit_texture_coord('-'), (0.25, 1.0));
    }

    #[test]
    fn format_3d_digits_pads_right_justified() {
        let digital = CabDigitalParams {
            scale_min: 0.0,
            scale_max: 999.0,
            accuracy: 0,
            leading_zeros: 0,
            justification: 6,
            units: None,
        };
        let s = format_3d_digits(&digital, 42.0, 6);
        assert_eq!(s, "    42");
    }

    #[test]
    fn digit_font_defaults_to_speed_ace() {
        assert_eq!(
            digit_font_file_name(&ControlType::Speedometer, None),
            "speed.ace"
        );
        assert_eq!(
            digit_font_file_name(&ControlType::Speedometer, Some("CLOCKS")),
            "CLOCKS.ACE"
        );
    }

    #[test]
    fn load_fixture_speed_ace() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/fixtures/cab");
        let img = load_digit_font_image(
            &fixture.join("dummy.s"),
            &fixture,
            &ControlType::Speedometer,
            None,
        );
        assert!(img.is_some(), "docs/fixtures/cab/speed.ace must decode");
    }

    #[test]
    fn matrix_gauge_pointer_stays_multistate() {
        let shape = ShapeFile::default();
        let cvf = openrailsrs_formats::CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![CabControl::Gauge {
                control_type: ControlType::BrakePipe,
                position: openrailsrs_formats::ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                graphic: String::new(),
                gauge: CabGaugeParams {
                    style: Some("POINTER".into()),
                    ..Default::default()
                },
            }],
        };
        let driver = crate::cab_cvf::matrix_driver_from_name("BRAKE_PIPE:0:0", &shape, 0, &cvf);
        assert!(matches!(driver, Some(MatrixDriver::MultiState { .. })));
    }

    #[test]
    fn matrix_gauge_solid_becomes_gauge_native() {
        let shape = ShapeFile::default();
        let cvf = openrailsrs_formats::CabViewFile {
            cab_view_type: None,
            views: vec![],
            controls: vec![CabControl::Gauge {
                control_type: ControlType::Ammeter,
                position: openrailsrs_formats::ScreenRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                graphic: String::new(),
                gauge: CabGaugeParams {
                    style: Some("SOLID".into()),
                    ..Default::default()
                },
            }],
        };
        let driver = crate::cab_cvf::matrix_driver_from_name("AMMETER:0:10:100", &shape, 2, &cvf);
        assert_eq!(
            driver,
            Some(MatrixDriver::GaugeNative {
                control: ControlType::Ammeter,
                order: 0,
                width_mm: 10.0,
                length_mm: 100.0,
            })
        );
    }
}

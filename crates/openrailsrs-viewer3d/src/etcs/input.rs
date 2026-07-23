//! DMI soft-key hit-test and interactive UI state (#161).

use bevy::prelude::*;

use super::paint::{DMI_H, DMI_W};
use super::status::EtcsStatus;

/// Interactive DMI controls (scroll / scale / menu), independent of TCS.
#[derive(Resource, Clone, Debug)]
pub struct EtcsUiState {
    pub message_page: usize,
    /// Planning zoom (OR `MaxViewingDistanceM`), metres.
    pub planning_max_m: i32,
    pub pressed: Option<DmiHit>,
    pub pressed_until_s: f64,
    /// Echo of last soft-key action (shown in message area briefly).
    pub last_action: Option<String>,
    pub last_action_until_s: f64,
}

impl Default for EtcsUiState {
    fn default() -> Self {
        Self {
            message_page: 0,
            planning_max_m: 4000,
            pressed: None,
            pressed_until_s: 0.0,
            last_action: None,
            last_action_until_s: 0.0,
        }
    }
}

impl EtcsUiState {
    pub const PLAN_MIN_M: i32 = 1000;
    pub const PLAN_MAX_M: i32 = 32_000;

    pub fn tick(&mut self, now_s: f64) {
        if self.pressed.is_some() && now_s >= self.pressed_until_s {
            self.pressed = None;
        }
        if self.last_action.is_some() && now_s >= self.last_action_until_s {
            self.last_action = None;
        }
    }

    pub fn apply_to_status(&self, status: &mut EtcsStatus, now_s: f64) {
        status.planning_max_m = f64::from(self.planning_max_m);
        status.message_page = self.message_page;
        status.pressed_hit = self.pressed.filter(|_| now_s < self.pressed_until_s);
        if let Some(msg) = self.last_action.as_ref() {
            if now_s < self.last_action_until_s {
                status.messages.push(msg.clone());
            }
        }
    }

    pub fn handle_hit(&mut self, hit: DmiHit, now_s: f64, message_count: usize) {
        self.pressed = Some(hit);
        self.pressed_until_s = now_s + 0.2;
        match hit {
            DmiHit::ScrollUp => {
                let pages = message_pages(message_count);
                if self.message_page + 1 < pages {
                    self.message_page += 1;
                    self.flash_action("Scroll up", now_s);
                }
            }
            DmiHit::ScrollDown => {
                if self.message_page > 0 {
                    self.message_page -= 1;
                    self.flash_action("Scroll down", now_s);
                }
            }
            DmiHit::ScaleUp => {
                if self.planning_max_m > Self::PLAN_MIN_M {
                    self.planning_max_m = (self.planning_max_m / 2).max(Self::PLAN_MIN_M);
                    self.flash_action(&format!("Scale {}", self.planning_max_m), now_s);
                }
            }
            DmiHit::ScaleDown => {
                if self.planning_max_m < Self::PLAN_MAX_M {
                    self.planning_max_m = (self.planning_max_m * 2).min(Self::PLAN_MAX_M);
                    self.flash_action(&format!("Scale {}", self.planning_max_m), now_s);
                }
            }
            DmiHit::SoftKey(i) => {
                self.flash_action(&format!("Key {}", i + 1), now_s);
            }
        }
    }

    fn flash_action(&mut self, msg: &str, now_s: f64) {
        self.last_action = Some(msg.to_string());
        self.last_action_until_s = now_s + 1.5;
    }
}

fn message_pages(message_count: usize) -> usize {
    ((message_count.max(1) + 4) / 5).max(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmiHit {
    ScrollUp,
    ScrollDown,
    ScaleUp,
    ScaleDown,
    SoftKey(u8),
}

/// Map DMI pixel coords (origin top-left, 640×480) to a soft key.
pub fn hit_test_dmi(x: i32, y: i32) -> Option<DmiHit> {
    if !(0..DMI_W as i32).contains(&x) || !(0..DMI_H as i32).contains(&y) {
        return None;
    }
    // Message scroll
    if rect_contains(288, 365, 46, 50, x, y) {
        return Some(DmiHit::ScrollUp);
    }
    if rect_contains(288, 415, 46, 50, x, y) {
        return Some(DmiHit::ScrollDown);
    }
    // Planning scale
    if rect_contains(334, 15, 40, 15, x, y) {
        return Some(DmiHit::ScaleUp);
    }
    if rect_contains(334, 300, 40, 15, x, y) {
        return Some(DmiHit::ScaleDown);
    }
    // Right soft keys
    for i in 0..6i32 {
        let by = 15 + 50 * i;
        if rect_contains(580, by, 60, 48, x, y) {
            return Some(DmiHit::SoftKey(i as u8));
        }
    }
    None
}

/// UV (0–1, Bevy/top-left) → DMI pixel.
pub fn uv_to_dmi(uv: Vec2) -> (i32, i32) {
    let x = (uv.x.clamp(0.0, 1.0) * (DMI_W as f32 - 1.0)).round() as i32;
    let y = (uv.y.clamp(0.0, 1.0) * (DMI_H as f32 - 1.0)).round() as i32;
    (x, y)
}

fn rect_contains(rx: i32, ry: i32, rw: i32, rh: i32, x: i32, y: i32) -> bool {
    x >= rx && y >= ry && x < rx + rw && y < ry + rh
}

/// Ray ↔ triangle mesh; returns closest hit distance and interpolated UV0.
pub fn raycast_mesh_uv(
    mesh: &Mesh,
    world_from_local: Mat4,
    origin: Vec3,
    dir: Vec3,
) -> Option<(f32, Vec2)> {
    let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?.as_float3()?;
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0)? {
        bevy::render::mesh::VertexAttributeValues::Float32x2(v) => v.as_slice(),
        _ => return None,
    };
    if positions.len() != uvs.len() {
        return None;
    }
    let Some(indices) = mesh.indices() else {
        return None;
    };
    let mut best: Option<(f32, Vec2)> = None;
    let mut tri = [0u32; 3];
    let mut tix = 0usize;
    for idx in indices.iter() {
        tri[tix] = idx as u32;
        tix += 1;
        if tix < 3 {
            continue;
        }
        tix = 0;
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        let p0 = world_from_local.transform_point3(Vec3::from(positions[i0]));
        let p1 = world_from_local.transform_point3(Vec3::from(positions[i1]));
        let p2 = world_from_local.transform_point3(Vec3::from(positions[i2]));
        if let Some((t, b, c)) = ray_triangle(origin, dir, p0, p1, p2) {
            if t <= 0.0 {
                continue;
            }
            if best.is_some_and(|(bt, _)| t >= bt) {
                continue;
            }
            let a = 1.0 - b - c;
            let uv0 = Vec2::from(uvs[i0]);
            let uv1 = Vec2::from(uvs[i1]);
            let uv2 = Vec2::from(uvs[i2]);
            let uv = uv0 * a + uv1 * b + uv2 * c;
            best = Some((t, uv));
        }
    }
    best
}

/// Möller–Trumbore; returns (t, u, v) barycentric with hit = (1-u-v)*p0 + u*p1 + v*p2.
fn ray_triangle(
    origin: Vec3,
    dir: Vec3,
    p0: Vec3,
    p1: Vec3,
    p2: Vec3,
) -> Option<(f32, f32, f32)> {
    const EPS: f32 = 1e-6;
    let e1 = p1 - p0;
    let e2 = p2 - p0;
    let pvec = dir.cross(e2);
    let det = e1.dot(pvec);
    if det.abs() < EPS {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = origin - p0;
    let u = tvec.dot(pvec) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(e1);
    let v = dir.dot(qvec) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(qvec) * inv;
    Some((t, u, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_scroll_and_scale() {
        assert_eq!(hit_test_dmi(300, 380), Some(DmiHit::ScrollUp));
        assert_eq!(hit_test_dmi(300, 430), Some(DmiHit::ScrollDown));
        assert_eq!(hit_test_dmi(340, 20), Some(DmiHit::ScaleUp));
        assert_eq!(hit_test_dmi(340, 305), Some(DmiHit::ScaleDown));
        assert_eq!(hit_test_dmi(600, 40), Some(DmiHit::SoftKey(0)));
        assert_eq!(hit_test_dmi(100, 100), None);
    }

    #[test]
    fn scale_zoom_halves_and_doubles() {
        let mut ui = EtcsUiState::default();
        ui.handle_hit(DmiHit::ScaleUp, 0.0, 3);
        assert_eq!(ui.planning_max_m, 2000);
        ui.handle_hit(DmiHit::ScaleDown, 0.1, 3);
        assert_eq!(ui.planning_max_m, 4000);
    }

    #[test]
    fn message_scroll_pages() {
        let mut ui = EtcsUiState::default();
        // 12 messages → 3 pages
        ui.handle_hit(DmiHit::ScrollUp, 0.0, 12);
        assert_eq!(ui.message_page, 1);
        ui.handle_hit(DmiHit::ScrollUp, 0.1, 12);
        assert_eq!(ui.message_page, 2);
        ui.handle_hit(DmiHit::ScrollUp, 0.2, 12);
        assert_eq!(ui.message_page, 2);
        ui.handle_hit(DmiHit::ScrollDown, 0.3, 12);
        assert_eq!(ui.message_page, 1);
    }

    #[test]
    fn raycast_unit_quad_uv() {
        let mut mesh = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        );
        mesh.insert_indices(bevy::render::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
        let hit = raycast_mesh_uv(
            &mesh,
            Mat4::IDENTITY,
            Vec3::new(0.5, 0.5, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        );
        let (t, uv) = hit.expect("hit");
        assert!(t > 0.0);
        assert!((uv.x - 0.5).abs() < 0.05);
        assert!((uv.y - 0.5).abs() < 0.05);
    }
}

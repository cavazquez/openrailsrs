//! DMI soft-key hit-test and interactive UI state (#161/#162).

use bevy::prelude::*;

use super::mode::DmiMode;
use super::paint::{DMI_H, DMI_W};
use super::status::EtcsStatus;
use super::subwindow::{self, DmiOverlay, SubHit};

/// Interactive DMI controls (scroll / scale / menu / overlays).
#[derive(Resource, Clone, Debug)]
pub struct EtcsUiState {
    pub message_page: usize,
    pub planning_max_m: i32,
    pub pressed: Option<DmiHit>,
    pub pressed_until_s: f64,
    pub last_action: Option<String>,
    pub last_action_until_s: f64,
    pub overlay: DmiOverlay,
    pub sub_pressed: Option<SubHit>,
    pub sub_pressed_until_s: f64,
    /// Acknowledged message texts this session.
    pub acked: Vec<String>,
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
            overlay: DmiOverlay::None,
            sub_pressed: None,
            sub_pressed_until_s: 0.0,
            acked: Vec::new(),
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
        if self.sub_pressed.is_some() && now_s >= self.sub_pressed_until_s {
            self.sub_pressed = None;
        }
        if self.last_action.is_some() && now_s >= self.last_action_until_s {
            self.last_action = None;
        }
    }

    pub fn apply_to_status(&self, status: &mut EtcsStatus, now_s: f64) {
        status.planning_max_m = f64::from(self.planning_max_m);
        status.message_page = self.message_page;
        status.pressed_hit = self.pressed.filter(|_| now_s < self.pressed_until_s);
        status.blink_on = (now_s * 4.0).fract() < 0.5;
        for m in &mut status.messages {
            if m.acknowledgeable && self.acked.iter().any(|a| a == &m.text) {
                m.acknowledged = true;
                m.acknowledgeable = false;
            }
        }
        status.needs_ack = status
            .messages
            .iter()
            .any(|m| m.acknowledgeable && !m.acknowledged);
        if let Some(msg) = self.last_action.as_ref() {
            if now_s < self.last_action_until_s {
                status.messages.push(super::status::TextMessage {
                    text: msg.clone(),
                    acknowledgeable: false,
                    acknowledged: true,
                });
            }
        }
    }

    pub fn handle_dmi_click(&mut self, x: i32, y: i32, now_s: f64, status: &EtcsStatus) {
        if self.overlay.is_open() {
            if let Some(hit) = subwindow::hit_test_overlay(&self.overlay, x, y) {
                self.handle_sub_hit(hit, now_s);
            }
            return;
        }
        // Message area ack (OR MessageArea press).
        if status.needs_ack && rect_contains(54, 365, 234, 100, x, y) {
            if let Some(m) = status
                .messages
                .iter()
                .rev()
                .find(|m| m.acknowledgeable && !m.acknowledged)
            {
                self.acked.push(m.text.clone());
                self.flash_action(&format!("Ack {}", m.text), now_s);
            }
            return;
        }
        if let Some(hit) = hit_test_dmi(x, y, status.dmi_mode) {
            self.handle_hit(hit, now_s, status.messages.len());
        }
    }

    fn handle_sub_hit(&mut self, hit: SubHit, now_s: f64) {
        self.sub_pressed = Some(hit);
        self.sub_pressed_until_s = now_s + 0.2;
        match hit {
            SubHit::Close => {
                self.overlay = DmiOverlay::None;
                self.flash_action("Close", now_s);
            }
            SubHit::MenuItem(i) => match &self.overlay {
                DmiOverlay::MainMenu => match i {
                    0 => self.flash_action("Start", now_s),
                    1 => self.flash_action("Override", now_s),
                    2 => {
                        self.overlay = DmiOverlay::DataEntry {
                            value: String::new(),
                        };
                    }
                    3 => self.flash_action("Special", now_s),
                    4 => self.overlay = DmiOverlay::Settings,
                    5 => {
                        self.overlay = DmiOverlay::None;
                        self.flash_action("Quit", now_s);
                    }
                    _ => {}
                },
                DmiOverlay::Settings => {
                    if i == 4 {
                        self.overlay = DmiOverlay::MainMenu;
                    } else {
                        self.flash_action("Settings", now_s);
                    }
                }
                _ => {}
            },
            SubHit::KeyDigit(d) => {
                if let DmiOverlay::DataEntry { value } = &mut self.overlay {
                    if value.len() < 8 {
                        value.push(char::from(b'0' + d));
                    }
                }
            }
            SubHit::KeyDot => {
                if let DmiOverlay::DataEntry { value } = &mut self.overlay {
                    if !value.contains('.') && value.len() < 8 {
                        value.push('.');
                    }
                }
            }
            SubHit::KeyDel => {
                if let DmiOverlay::DataEntry { value } = &mut self.overlay {
                    value.pop();
                }
            }
            SubHit::KeyYes => {
                if let DmiOverlay::DataEntry { value } = &self.overlay {
                    let v = value.clone();
                    self.overlay = DmiOverlay::None;
                    self.flash_action(&format!("Entered {v}"), now_s);
                }
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
            DmiHit::SoftKey(0) => {
                self.overlay = DmiOverlay::MainMenu;
                self.flash_action("Main", now_s);
            }
            DmiHit::SoftKey(1) => self.flash_action("Override", now_s),
            DmiHit::SoftKey(2) => {
                self.overlay = DmiOverlay::DataEntry {
                    value: String::new(),
                };
            }
            DmiHit::SoftKey(3) => self.flash_action("Special", now_s),
            DmiHit::SoftKey(4) => {
                self.overlay = DmiOverlay::Settings;
            }
            DmiHit::SoftKey(_) => {}
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

/// Map DMI pixel coords to a soft key (layout depends on mode).
pub fn hit_test_dmi(x: i32, y: i32, mode: DmiMode) -> Option<DmiHit> {
    let (dw, dh) = mode.size();
    if !(0..dw as i32).contains(&x) || !(0..dh as i32).contains(&y) {
        return None;
    }
    match mode {
        DmiMode::GaugeOnly => None,
        DmiMode::PlanningArea => {
            if rect_contains(0, 0, 40, 15, x, y) {
                return Some(DmiHit::ScaleUp);
            }
            if rect_contains(0, 285, 40, 15, x, y) {
                return Some(DmiHit::ScaleDown);
            }
            for i in 0..6i32 {
                let by = 15 + 50 * i;
                if rect_contains(274, by, 60, 48, x, y) {
                    return Some(DmiHit::SoftKey(i as u8));
                }
            }
            None
        }
        DmiMode::SpeedArea => {
            if rect_contains(288 - 54, 365, 46, 50, x, y) || rect_contains(234, 365, 46, 50, x, y) {
                return Some(DmiHit::ScrollUp);
            }
            if rect_contains(234, 415, 46, 50, x, y) {
                return Some(DmiHit::ScrollDown);
            }
            None
        }
        DmiMode::FullSize => {
            if rect_contains(288, 365, 46, 50, x, y) {
                return Some(DmiHit::ScrollUp);
            }
            if rect_contains(288, 415, 46, 50, x, y) {
                return Some(DmiHit::ScrollDown);
            }
            if rect_contains(334, 15, 40, 15, x, y) {
                return Some(DmiHit::ScaleUp);
            }
            if rect_contains(334, 300, 40, 15, x, y) {
                return Some(DmiHit::ScaleDown);
            }
            for i in 0..6i32 {
                let by = 15 + 50 * i;
                if rect_contains(580, by, 60, 48, x, y) {
                    return Some(DmiHit::SoftKey(i as u8));
                }
            }
            None
        }
    }
}

/// UV (0–1) → DMI pixel for given mode size.
pub fn uv_to_dmi(uv: Vec2, mode: DmiMode) -> (i32, i32) {
    let (dw, dh) = mode.size();
    let x = (uv.x.clamp(0.0, 1.0) * (dw as f32 - 1.0)).round() as i32;
    let y = (uv.y.clamp(0.0, 1.0) * (dh as f32 - 1.0)).round() as i32;
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
        assert_eq!(
            hit_test_dmi(300, 380, DmiMode::FullSize),
            Some(DmiHit::ScrollUp)
        );
        assert_eq!(
            hit_test_dmi(340, 20, DmiMode::FullSize),
            Some(DmiHit::ScaleUp)
        );
        assert_eq!(
            hit_test_dmi(600, 40, DmiMode::FullSize),
            Some(DmiHit::SoftKey(0))
        );
    }

    #[test]
    fn soft_key_opens_menu() {
        let mut ui = EtcsUiState::default();
        ui.handle_hit(DmiHit::SoftKey(0), 0.0, 3);
        assert_eq!(ui.overlay, DmiOverlay::MainMenu);
    }

    #[test]
    fn data_entry_digits() {
        let mut ui = EtcsUiState::default();
        ui.overlay = DmiOverlay::DataEntry {
            value: String::new(),
        };
        ui.handle_sub_hit(SubHit::KeyDigit(1), 0.0);
        ui.handle_sub_hit(SubHit::KeyDigit(2), 0.1);
        match &ui.overlay {
            DmiOverlay::DataEntry { value } => assert_eq!(value, "12"),
            _ => panic!("expected data entry"),
        }
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
        ui.handle_hit(DmiHit::ScrollUp, 0.0, 12);
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

    #[test]
    #[allow(unused_imports)]
    fn dmi_wh_consts() {
        assert_eq!(DMI_W, 640);
        assert_eq!(DMI_H, 480);
    }
}

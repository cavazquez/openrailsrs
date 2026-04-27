//! 2D animated viewer: track topology + signals + multi-train replay.
//!
//! Usage:
//!   openrailsrs-viewer <route_dir>              -- static topology only
//!   openrailsrs-viewer <scenario.toml>          -- topology + signal markers + animated trains
//!
//! Keyboard controls:
//!   Space   — pause / resume
//!   R       — reset to t=0
//!   +       — double playback speed
//!   -       — halve  playback speed
//!   Esc     — quit

use std::path::{Path, PathBuf};
use std::time::Instant;

use minifb::{Key, Window, WindowOptions};
use openrailsrs_route::load_track_graph_from_route_dir;
use openrailsrs_track::{SignalAspect, TrackGraph};

const W: usize = 1024;
const H: usize = 768;
const HUD_H: usize = 60; // pixels reserved at bottom for HUD
const PAD: f64 = 55.0;
const TARGET_FPS: u64 = 60;

// ── Colours (ARGB 0x00RRGGBB) ──────────────────────────────────────────────
const COL_BG: u32 = 0x00_0d_1b_2a;
const COL_EDGE: u32 = 0x00_ff_aa_33;
const COL_EDGE_THICK: u32 = 0x00_c8_80_20;
const COL_NODE: u32 = 0x00_ff_ff_ff;
const COL_SWITCH: u32 = 0x00_00_ff_ff;
const COL_STATION: u32 = 0x00_ff_ff_00;
const COL_SIG_STOP: u32 = 0x00_ff_22_22;
const COL_SIG_CAUTION: u32 = 0x00_ff_cc_00;
const COL_SIG_CLEAR: u32 = 0x00_22_ff_55;
const COL_HUD_BG: u32 = 0x00_05_10_1a;
const COL_HUD_TEXT: u32 = 0x00_cc_cc_cc;
const COL_PAUSED: u32 = 0x00_ff_66_00;
const COL_GRID: u32 = 0x00_14_28_3c;

/// Palette for trains (up to 8).
const TRAIN_COLORS: [u32; 8] = [
    0x00_ff_40_ff, // magenta — primary
    0x00_40_ff_ff, // cyan    — express
    0x00_80_ff_40, // lime
    0x00_ff_80_40, // orange
    0x00_ff_40_80, // rose
    0x00_80_40_ff, // violet
    0x00_40_80_ff, // sky
    0x00_ff_ff_40, // yellow
];

// ── Train CSV row ──────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct CsvRow {
    time_s: f64,
    velocity_mps: f64,
    #[serde(default)]
    edge_id: String,
    #[serde(default)]
    pos_on_edge_m: f64,
}

fn load_csv(path: &Path) -> Vec<CsvRow> {
    let Ok(mut rdr) = csv::Reader::from_path(path) else {
        return vec![];
    };
    rdr.deserialize::<CsvRow>().filter_map(|r| r.ok()).collect()
}

// ── Coordinate mapping ────────────────────────────────────────────────────

struct Viewport {
    minx: f64,
    miny: f64,
    s: f64,
}

impl Viewport {
    fn from_graph(graph: &TrackGraph) -> Self {
        let mut minx = f64::MAX;
        let mut miny = f64::MAX;
        let mut maxx = f64::MIN;
        let mut maxy = f64::MIN;
        for (_, n) in graph.nodes_iter() {
            minx = minx.min(n.x_m);
            miny = miny.min(n.y_m);
            maxx = maxx.max(n.x_m);
            maxy = maxy.max(n.y_m);
        }
        let rw = (maxx - minx).max(1.0);
        let rh = (maxy - miny).max(1.0);
        let draw_h = (H - HUD_H) as f64;
        let sx = (W as f64 - 2.0 * PAD) / rw;
        let sy = (draw_h - 2.0 * PAD) / rh;
        Self {
            minx,
            miny,
            s: sx.min(sy),
        }
    }

    fn world_to_px(&self, x_m: f64, y_m: f64) -> (isize, isize) {
        let draw_h = (H - HUD_H) as f64;
        let px = ((x_m - self.minx) * self.s + PAD) as isize;
        let py = (draw_h - ((y_m - self.miny) * self.s + PAD)) as isize;
        (px, py)
    }
}

// ── Drawing primitives ────────────────────────────────────────────────────

fn set_pixel(buf: &mut [u32], x: isize, y: isize, c: u32) {
    if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
        buf[y as usize * W + x as usize] = c;
    }
}

/// Bresenham line.
fn draw_line(buf: &mut [u32], mut p0: (isize, isize), p1: (isize, isize), c: u32) {
    let (mut x0, mut y0) = p0;
    let (x1, y1) = p1;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        set_pixel(buf, x0, y0, c);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }
    p0 = (x0, y0);
    let _ = p0;
}

/// Thick line: draw the main line plus 1-pixel offsets above/below.
fn draw_thick_line(buf: &mut [u32], p0: (isize, isize), p1: (isize, isize), c: u32, shadow: u32) {
    draw_line(buf, (p0.0 + 1, p0.1), (p1.0 + 1, p1.1), shadow);
    draw_line(buf, (p0.0, p0.1 + 1), (p1.0, p1.1 + 1), shadow);
    draw_line(buf, p0, p1, c);
}

/// Filled circle (Bresenham mid-point).
fn draw_circle(buf: &mut [u32], cx: isize, cy: isize, r: isize, c: u32) {
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                set_pixel(buf, cx + dx, cy + dy, c);
            }
        }
    }
}

/// Diamond (rotated square) — used for signals.
fn draw_diamond(buf: &mut [u32], cx: isize, cy: isize, r: isize, c: u32) {
    for dy in -r..=r {
        let half = r - dy.abs();
        for dx in -half..=half {
            set_pixel(buf, cx + dx, cy + dy, c);
        }
    }
}

/// Filled rectangle.
fn fill_rect(buf: &mut [u32], x: isize, y: isize, w: isize, h: isize, c: u32) {
    for dy in 0..h {
        for dx in 0..w {
            set_pixel(buf, x + dx, y + dy, c);
        }
    }
}

// ── Minimal 8×8 pixel font ─────────────────────────────────────────────────
//
// Each character is 8 rows × 8 bits.  Bit 7 is the leftmost pixel.
// Only printable ASCII 32–126 is included.
const FONT: [[u8; 8]; 127] = {
    let mut f = [[0u8; 8]; 127];
    // Space (32)
    // '0' (48)
    f[48] = [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00];
    f[49] = [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00];
    f[50] = [0x3C, 0x66, 0x06, 0x1C, 0x30, 0x62, 0x7E, 0x00];
    f[51] = [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00];
    f[52] = [0x0E, 0x1E, 0x36, 0x66, 0x7F, 0x06, 0x06, 0x00];
    f[53] = [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00];
    f[54] = [0x3C, 0x60, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00];
    f[55] = [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x00];
    f[56] = [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00];
    f[57] = [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x3C, 0x00];
    // '.' (46) ':' (58) '/' (47) '-' (45) '+' (43) '%' (37) 's' (115) 'k' (107) 'm' (109) 'h' (104)
    f[46] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00]; // .
    f[58] = [0x00, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00]; // :
    f[47] = [0x00, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x00, 0x00]; // /
    f[45] = [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00]; // -
    f[43] = [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00]; // +
    f[37] = [0x62, 0x66, 0x0C, 0x18, 0x30, 0x66, 0x46, 0x00]; // %
    f[61] = [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00]; // =
    f[120] = [0x00, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x00, 0x00]; // x
    // uppercase A-Z (65-90)
    f[65] = [0x18, 0x3C, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00]; // A
    f[66] = [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00]; // B
    f[67] = [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00]; // C
    f[68] = [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00]; // D
    f[69] = [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00]; // E
    f[70] = [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00]; // F
    f[71] = [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3C, 0x00]; // G
    f[72] = [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00]; // H
    f[73] = [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00]; // I
    f[74] = [0x06, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3C, 0x00]; // J
    f[75] = [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00]; // K
    f[76] = [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00]; // L
    f[77] = [0x63, 0x77, 0x7F, 0x6B, 0x63, 0x63, 0x63, 0x00]; // M
    f[78] = [0x66, 0x76, 0x7E, 0x7E, 0x6E, 0x66, 0x66, 0x00]; // N
    f[79] = [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00]; // O
    f[80] = [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00]; // P
    f[81] = [0x3C, 0x66, 0x66, 0x66, 0x6E, 0x3C, 0x06, 0x00]; // Q
    f[82] = [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00]; // R
    f[83] = [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00]; // S
    f[84] = [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00]; // T
    f[85] = [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00]; // U
    f[86] = [0x66, 0x66, 0x66, 0x66, 0x3C, 0x3C, 0x18, 0x00]; // V
    f[87] = [0x63, 0x63, 0x6B, 0x7F, 0x77, 0x63, 0x63, 0x00]; // W
    f[88] = [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00]; // X
    f[89] = [0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x00]; // Y
    f[90] = [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00]; // Z
    // lowercase
    f[97] = [0x00, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00]; // a
    f[98] = [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x00]; // b
    f[99] = [0x00, 0x00, 0x3C, 0x60, 0x60, 0x60, 0x3C, 0x00]; // c
    f[100] = [0x06, 0x06, 0x3E, 0x66, 0x66, 0x66, 0x3E, 0x00]; // d
    f[101] = [0x00, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00]; // e
    f[102] = [0x0E, 0x18, 0x18, 0x3E, 0x18, 0x18, 0x18, 0x00]; // f
    f[103] = [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x3C]; // g
    f[104] = [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00]; // h
    f[105] = [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00]; // i
    f[106] = [0x06, 0x00, 0x06, 0x06, 0x06, 0x06, 0x66, 0x3C]; // j
    f[107] = [0x60, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0x00]; // k
    f[108] = [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00]; // l
    f[109] = [0x00, 0x00, 0x66, 0x7F, 0x7F, 0x6B, 0x63, 0x00]; // m
    f[110] = [0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00]; // n
    f[111] = [0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00]; // o
    f[112] = [0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60]; // p
    f[113] = [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x06]; // q
    f[114] = [0x00, 0x00, 0x6C, 0x76, 0x60, 0x60, 0x60, 0x00]; // r
    f[115] = [0x00, 0x00, 0x3C, 0x60, 0x3C, 0x06, 0x7C, 0x00]; // s
    f[116] = [0x18, 0x18, 0x7E, 0x18, 0x18, 0x18, 0x0E, 0x00]; // t
    f[117] = [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00]; // u
    f[118] = [0x00, 0x00, 0x66, 0x66, 0x3C, 0x3C, 0x18, 0x00]; // v
    f[119] = [0x00, 0x00, 0x63, 0x6B, 0x7F, 0x3E, 0x36, 0x00]; // w
    f[121] = [0x00, 0x00, 0x66, 0x3C, 0x18, 0x30, 0x60, 0x00]; // y
    f[122] = [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00]; // z
    f
};

fn draw_char(buf: &mut [u32], ch: char, x: isize, y: isize, fg: u32) {
    let idx = ch as usize;
    if idx >= FONT.len() {
        return;
    }
    let glyph = FONT[idx];
    for (row, &byte) in glyph.iter().enumerate() {
        for col in 0..8_isize {
            if byte & (0x80 >> col) != 0 {
                set_pixel(buf, x + col, y + row as isize, fg);
            }
        }
    }
}

fn draw_str(buf: &mut [u32], s: &str, x: isize, y: isize, fg: u32) {
    for (i, ch) in s.chars().enumerate() {
        draw_char(buf, ch, x + i as isize * 9, y, fg);
    }
}

// ── Train data ────────────────────────────────────────────────────────────

struct TrainTrack {
    label: String,
    color: u32,
    rows: Vec<CsvRow>,
}

impl TrainTrack {
    /// Interpolate pixel position at simulation time `t`.
    fn position_at(
        &self,
        t: f64,
        graph: &TrackGraph,
        vp: &Viewport,
    ) -> Option<(isize, isize, f64)> {
        if self.rows.is_empty() {
            return None;
        }
        // Binary search for the straddling rows.
        let idx = self
            .rows
            .partition_point(|r| r.time_s <= t)
            .saturating_sub(1)
            .min(self.rows.len() - 1);
        let row = &self.rows[idx];
        let vel = row.velocity_mps;

        let edge_id = row.edge_id.trim();
        let edge = graph.edge(edge_id)?;
        let from_node = graph.node(&edge.from.0)?;
        let to_node = graph.node(&edge.to.0)?;

        let frac = if edge.length_m > 0.0 {
            (row.pos_on_edge_m / edge.length_m).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let x_m = from_node.x_m + frac * (to_node.x_m - from_node.x_m);
        let y_m = from_node.y_m + frac * (to_node.y_m - from_node.y_m);
        let (px, py) = vp.world_to_px(x_m, y_m);
        Some((px, py, vel))
    }
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: openrailsrs-viewer <route_dir | scenario.toml>")?;

    // Determine route dir and optional CSV tracks.
    let (route_dir, train_tracks, scenario_name): (PathBuf, Vec<TrainTrack>, String) = {
        if arg.extension().and_then(|e| e.to_str()) == Some("toml") {
            let scenario_dir = arg
                .parent()
                .ok_or("scenario path has no parent")?
                .to_path_buf();
            let scenario = openrailsrs_scenarios::load_scenario(&arg)?;
            let mut tracks = Vec::new();
            let primary_csv = scenario_dir.join(&scenario.output.csv);
            let rows = load_csv(&primary_csv);
            if !rows.is_empty() {
                tracks.push(TrainTrack {
                    label: "primary".into(),
                    color: TRAIN_COLORS[0],
                    rows,
                });
            }
            for (i, extra) in scenario.extra_trains.iter().enumerate() {
                let path = scenario_dir.join(&extra.output_csv);
                let rows = load_csv(&path);
                if !rows.is_empty() {
                    tracks.push(TrainTrack {
                        label: extra.id.clone(),
                        color: TRAIN_COLORS[(i + 1) % TRAIN_COLORS.len()],
                        rows,
                    });
                }
            }
            let route = scenario_dir.join(&scenario.route.path);
            let name = scenario.scenario.name.clone();
            (route, tracks, name)
        } else {
            (arg.clone(), vec![], "Track view".into())
        }
    };

    let graph = load_track_graph_from_route_dir(&route_dir)?;
    let vp = Viewport::from_graph(&graph);

    // Find max sim time across all tracks.
    let max_t = train_tracks
        .iter()
        .filter_map(|t| t.rows.last().map(|r| r.time_s))
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut window = Window::new("openrailsrs-viewer 2D", W, H, WindowOptions::default())?;
    window.set_target_fps(TARGET_FPS as usize);

    let mut buffer: Vec<u32> = vec![0; W * H];
    let mut t_sim: f64 = 0.0;
    let mut speed: f64 = 1.0; // playback speed multiplier
    let mut paused = false;
    let mut last_frame = Instant::now();

    // Key-repeat state.
    let mut space_prev = false;
    let mut plus_prev = false;
    let mut minus_prev = false;
    let mut r_prev = false;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = Instant::now();
        let dt_wall = now.duration_since(last_frame).as_secs_f64();
        last_frame = now;

        // ── Key handling ────────────────────────────────────────────────
        let space = window.is_key_down(Key::Space);
        let plus = window.is_key_down(Key::Equal); // '=' key (no shift needed)
        let minus = window.is_key_down(Key::Minus);
        let r_key = window.is_key_down(Key::R);

        if space && !space_prev {
            paused = !paused;
        }
        if plus && !plus_prev {
            speed = (speed * 2.0).min(64.0);
        }
        if minus && !minus_prev {
            speed = (speed / 2.0).max(0.125);
        }
        if r_key && !r_prev {
            t_sim = 0.0;
        }

        space_prev = space;
        plus_prev = plus;
        minus_prev = minus;
        r_prev = r_key;

        // Advance simulation time.
        if !paused && !train_tracks.is_empty() {
            t_sim += dt_wall * speed;
            if t_sim > max_t {
                t_sim = max_t;
                paused = true;
            }
        }

        // ── Clear ────────────────────────────────────────────────────────
        for p in buffer.iter_mut() {
            *p = COL_BG;
        }

        // ── Background grid ──────────────────────────────────────────────
        for gx in (0..W).step_by(80) {
            for gy in 0..H - HUD_H {
                set_pixel(&mut buffer, gx as isize, gy as isize, COL_GRID);
            }
        }
        for gy in (0..H - HUD_H).step_by(80) {
            for gx in 0..W {
                set_pixel(&mut buffer, gx as isize, gy as isize, COL_GRID);
            }
        }

        // ── Draw edges ───────────────────────────────────────────────────
        for (_, e) in graph.edges_iter() {
            if let (Some(a), Some(b)) = (graph.node(&e.from.0), graph.node(&e.to.0)) {
                let p0 = vp.world_to_px(a.x_m, a.y_m);
                let p1 = vp.world_to_px(b.x_m, b.y_m);
                draw_thick_line(&mut buffer, p0, p1, COL_EDGE, COL_EDGE_THICK);

                // Edge label at midpoint.
                let mx = (p0.0 + p1.0) / 2 + 3;
                let my = (p0.1 + p1.1) / 2 - 10;
                draw_str(&mut buffer, &e.id.0, mx, my, 0x00_88_88_88);
            }
        }

        // ── Draw signals ─────────────────────────────────────────────────
        for sig in graph.signals() {
            if let Some(edge) = graph.edge(&sig.edge_id) {
                if let (Some(a), Some(b)) = (graph.node(&edge.from.0), graph.node(&edge.to.0)) {
                    let frac = if edge.length_m > 0.0 {
                        (sig.position_m / edge.length_m).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let x_m = a.x_m + frac * (b.x_m - a.x_m);
                    let y_m = a.y_m + frac * (b.y_m - a.y_m);
                    let (px, py) = vp.world_to_px(x_m, y_m);
                    let col = match sig.aspect {
                        SignalAspect::Stop => COL_SIG_STOP,
                        SignalAspect::Caution => COL_SIG_CAUTION,
                        SignalAspect::Clear => COL_SIG_CLEAR,
                    };
                    // Outer ring (black) + inner diamond.
                    draw_diamond(&mut buffer, px, py, 7, 0x00_00_00_00);
                    draw_diamond(&mut buffer, px, py, 5, col);
                    // Signal pole (vertical line down).
                    for dy in 1..=10 {
                        set_pixel(&mut buffer, px, py + dy, 0x00_88_88_88);
                    }
                    // Label above.
                    draw_str(&mut buffer, &sig.id, px + 7, py - 12, col);
                }
            }
        }

        // ── Draw nodes ───────────────────────────────────────────────────
        for (_, n) in graph.nodes_iter() {
            let (px, py) = vp.world_to_px(n.x_m, n.y_m);
            let col = match &n.kind {
                openrailsrs_track::NodeKind::Switch { .. } => COL_SWITCH,
                openrailsrs_track::NodeKind::Station { .. } => COL_STATION,
                openrailsrs_track::NodeKind::Plain => COL_NODE,
            };
            draw_circle(&mut buffer, px, py, 5, 0x00_00_00_00); // shadow
            draw_circle(&mut buffer, px, py, 4, col);
            // Node label below.
            draw_str(&mut buffer, &n.id.0, px - 4, py + 8, col);
        }

        // ── Draw trains ──────────────────────────────────────────────────
        for track in &train_tracks {
            if let Some((px, py, vel)) = track.position_at(t_sim, &graph, &vp) {
                let col = track.color;
                // Glow ring based on speed.
                let glow_r = 3 + (vel * 0.15) as isize;
                draw_circle(&mut buffer, px, py, glow_r.min(10), blend_alpha(col, 0x40));
                // Solid core.
                draw_circle(&mut buffer, px, py, 5, col);
                // Label.
                draw_str(&mut buffer, &track.label, px + 8, py - 12, col);
                // Speed indicator line (direction of travel).
                let speed_len = (vel * 0.5) as isize;
                draw_line(
                    &mut buffer,
                    (px, py - 5),
                    (px, py - 5 - speed_len.min(20)),
                    col,
                );
            }
        }

        // ── HUD strip ────────────────────────────────────────────────────
        {
            let hy = (H - HUD_H) as isize;
            fill_rect(&mut buffer, 0, hy, W as isize, HUD_H as isize, COL_HUD_BG);
            // Top separator line.
            for gx in 0..W {
                set_pixel(&mut buffer, gx as isize, hy, 0x00_ff_aa_33);
            }

            // Row 1: scenario name + time
            let (t_col, status) = if paused {
                (COL_PAUSED, "PAUSED")
            } else {
                (0x00_88_ff_88, "PLAY")
            };
            let hud_y = hy + 6;
            draw_str(&mut buffer, &scenario_name, 8, hud_y, COL_HUD_TEXT);
            let status_x = W as isize - 9 * 8;
            draw_str(&mut buffer, status, status_x, hud_y, t_col);

            // Row 2: t=XXX.Xs  spd=Xx  trains=N
            let row2 = hy + 22;
            let t_int = t_sim as u64;
            let t_frac = ((t_sim - t_int as f64) * 10.0) as u64;
            let t_str = format!("t={}.{}s", t_int, t_frac);
            draw_str(&mut buffer, &t_str, 8, row2, 0x00_66_dd_ff);

            let spd_str = format!("spd={}x", speed as u32);
            draw_str(&mut buffer, &spd_str, 160, row2, 0x00_ff_cc_44);

            // Progress bar.
            let bar_x = 300_isize;
            let bar_w = (W as isize - bar_x - 16).max(10);
            let bar_h = 8_isize;
            let bar_y = row2 + 1;
            fill_rect(&mut buffer, bar_x, bar_y, bar_w, bar_h, 0x00_22_33_44);
            let fill = if max_t > 0.0 {
                ((t_sim / max_t) * bar_w as f64) as isize
            } else {
                0
            };
            fill_rect(
                &mut buffer,
                bar_x,
                bar_y,
                fill.clamp(0, bar_w),
                bar_h,
                0x00_44_bb_ff,
            );
            fill_rect(&mut buffer, bar_x, bar_y, 1, bar_h, 0x00_88_cc_ff);

            // Row 3: train legend
            let row3 = hy + 38;
            let mut lx = 8_isize;
            for track in &train_tracks {
                if let Some((_, _, vel)) = track.position_at(t_sim, &graph, &vp) {
                    fill_rect(&mut buffer, lx, row3, 8, 8, track.color);
                    lx += 12;
                    let vel_kmh = (vel * 3.6) as u32;
                    let label = format!("{} {}km/h", track.label, vel_kmh);
                    draw_str(&mut buffer, &label, lx, row3, track.color);
                    lx += 9 * (label.len() + 1) as isize;
                }
            }

            // Controls hint.
            draw_str(
                &mut buffer,
                "SPC:pause  R:reset  +/-:spd  ESC:quit",
                8,
                hy + HUD_H as isize - 14,
                0x00_44_55_66,
            );
        }

        window.update_with_buffer(&buffer, W, H)?;
    }
    Ok(())
}

/// Create a slightly transparent version of a color (for glow effects).
fn blend_alpha(col: u32, alpha: u32) -> u32 {
    let r = ((col >> 16) & 0xFF) * alpha / 255;
    let g = ((col >> 8) & 0xFF) * alpha / 255;
    let b = (col & 0xFF) * alpha / 255;
    (r << 16) | (g << 8) | b
}

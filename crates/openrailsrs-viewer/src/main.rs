//! Minimal 2D framebuffer view of `track.toml` topology (not coupled to `openrailsrs-sim`).

use std::path::PathBuf;

use minifb::{Key, Window, WindowOptions};
use openrailsrs_route::load_track_graph_from_route_dir;

const W: usize = 800;
const H: usize = 600;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let route = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: openrailsrs-viewer <route_dir>")?;
    let graph = load_track_graph_from_route_dir(&route)?;

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
    let pad = 40.0;
    let sx = (W as f64 - 2.0 * pad) / rw;
    let sy = (H as f64 - 2.0 * pad) / rh;
    let s = sx.min(sy);

    let mut buffer: Vec<u32> = vec![0; W * H];
    let mut window = Window::new("openrailsrs-viewer (2D)", W, H, WindowOptions::default())?;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        for p in buffer.iter_mut() {
            *p = 0x00102030;
        }
        for (_, e) in graph.edges_iter() {
            if let (Some(a), Some(b)) = (graph.node(&e.from.0), graph.node(&e.to.0)) {
                let x0 = ((a.x_m - minx) * s + pad) as isize;
                let y0 = (H as f64 - ((a.y_m - miny) * s + pad)) as isize;
                let x1 = ((b.x_m - minx) * s + pad) as isize;
                let y1 = (H as f64 - ((b.y_m - miny) * s + pad)) as isize;
                line(&mut buffer, W, H, (x0, y0), (x1, y1), 0x00_ff_aa_33);
            }
        }
        for (_, n) in graph.nodes_iter() {
            let x = ((n.x_m - minx) * s + pad) as usize;
            let y = (H as f64 - ((n.y_m - miny) * s + pad)) as usize;
            if x < W && y < H {
                for dy in 0..4 {
                    for dx in 0..4 {
                        let px = x + dx;
                        let py = y + dy;
                        if px < W && py < H {
                            buffer[py * W + px] = 0x00_ff_ff_ff;
                        }
                    }
                }
            }
        }
        window.update_with_buffer(&buffer, W, H)?;
    }
    Ok(())
}

fn line(buf: &mut [u32], w: usize, h: usize, p0: (isize, isize), p1: (isize, isize), c: u32) {
    let (mut x0, mut y0) = p0;
    let (x1, y1) = p1;
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        if x0 >= 0 && y0 >= 0 && (x0 as usize) < w && (y0 as usize) < h {
            buf[y0 as usize * w + x0 as usize] = c;
        }
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
}

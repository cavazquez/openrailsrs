use openrailsrs_track::TrackGraph;

/// Very small ASCII sketch: bbox of nodes scaled into a character grid.
pub fn track_graph_to_ascii(graph: &TrackGraph, width: usize, height: usize) -> String {
    if graph.nodes_iter().next().is_none() {
        return "(empty graph)\n".into();
    }
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
    let w = (maxx - minx).max(1.0);
    let h = (maxy - miny).max(1.0);
    let mut grid = vec![vec![b'.'; width]; height];
    for (_, e) in graph.edges_iter() {
        if let (Some(a), Some(b)) = (graph.node(&e.from.0), graph.node(&e.to.0)) {
            let x0 = (((a.x_m - minx) / w) * (width.saturating_sub(1)) as f64).round() as usize;
            let y0 = (((a.y_m - miny) / h) * (height.saturating_sub(1)) as f64).round() as usize;
            let x1 = (((b.x_m - minx) / w) * (width.saturating_sub(1)) as f64).round() as usize;
            let y1 = (((b.y_m - miny) / h) * (height.saturating_sub(1)) as f64).round() as usize;
            bresenham(&mut grid, x0, y0, x1, y1, b'#');
        }
    }
    for (_, n) in graph.nodes_iter() {
        let x = (((n.x_m - minx) / w) * (width.saturating_sub(1)) as f64).round() as usize;
        let y = (((n.y_m - miny) / h) * (height.saturating_sub(1)) as f64).round() as usize;
        if y < height && x < width {
            grid[y][x] = b'O';
        }
    }
    let mut out = String::new();
    for row in grid {
        out.push_str(std::str::from_utf8(&row).unwrap());
        out.push('\n');
    }
    out
}

fn bresenham(grid: &mut [Vec<u8>], mut x0: usize, mut y0: usize, x1: usize, y1: usize, c: u8) {
    let h = grid.len();
    let w = grid.first().map(|r| r.len()).unwrap_or(0);
    if h == 0 || w == 0 {
        return;
    }
    let dx = (x1 as isize - x0 as isize).abs();
    let dy = -(y1 as isize - y0 as isize).abs();
    let sx = if x0 < x1 { 1_isize } else { -1 };
    let sy = if y0 < y1 { 1_isize } else { -1 };
    let mut err = dx + dy;
    loop {
        if y0 < h && x0 < w {
            grid[y0][x0] = c;
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 = (x0 as isize + sx) as usize;
        }
        if e2 <= dx {
            err += dx;
            y0 = (y0 as isize + sy) as usize;
        }
    }
}

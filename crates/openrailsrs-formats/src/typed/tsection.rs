//! Parser for MSTS / Open Rails `tsection.dat` (track sections + track shapes).
//!
//! World `TrackObj` entries reference a `TrackShape` index via `SectionIdx`; each shape
//! maps to a `.s` filename and one or more paths built from `TrackSection` sizes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::{Ast, Atom};
use crate::encoding::{decode_msts_bytes, resolve_path_case_insensitive};
use crate::error::FormatError;
use crate::parser::{parse_all_top_level_lenient, parse_first};

fn matches_head(items: &[Ast], expected: &str) -> bool {
    matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(expected))
}

fn atom_to_string(atom: &Atom) -> Option<String> {
    match atom {
        Atom::String(value) | Atom::Symbol(value) => Some(value.clone()),
        _ => None,
    }
}

fn atom_to_number(atom: &Atom) -> Option<f64> {
    match atom {
        Atom::Number(value) => Some(*value),
        Atom::Integer(value) => Some(*value as f64),
        _ => None,
    }
}

/// One straight or curved track section definition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackSectionDef {
    pub gauge_m: f64,
    pub length_m: f64,
    pub curve_radius_m: Option<f64>,
    pub curve_angle_deg: Option<f64>,
    /// Small heading change (degrees) for alignment at junctions / skewed straights.
    pub skew_deg: Option<f64>,
}

/// One path through a track shape (left → right in MSTS convention).
#[derive(Clone, Debug, PartialEq)]
pub struct TrackShapePath {
    pub num_sections: u32,
    pub offset: [f64; 3],
    pub angle_deg: f64,
    pub section_indices: Vec<u32>,
}

/// Track shape: `.s` mesh plus section path geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackShapeDef {
    pub file_name: String,
    /// Path index of the through route for junction shapes (`MainRoute` in `tsection.dat`).
    pub main_route: Option<u32>,
    /// Occupancy clearance in metres for diverging routes (`ClearanceDist` in junction shapes).
    pub clearance_dist_m: Option<f64>,
    pub paths: Vec<TrackShapePath>,
}

impl TrackShapeDef {
    /// Junction / turnout shape (multiple paths or explicit `MainRoute`).
    pub fn is_junction(&self) -> bool {
        self.paths.len() > 1 || self.main_route.is_some()
    }

    /// Path indices for procedural geometry: `MainRoute` when valid, otherwise all paths.
    pub fn procedural_path_indices(&self) -> Vec<usize> {
        if let Some(main) = self.main_route {
            let idx = main as usize;
            if idx < self.paths.len() {
                return vec![idx];
            }
        }
        (0..self.paths.len()).collect()
    }
}

/// Dimensions for procedural sleepers/rails derived from TSection data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackProceduralDims {
    pub length_m: f64,
    pub half_gauge_m: f64,
    pub curve_radius_m: Option<f64>,
    pub curve_angle_deg: Option<f64>,
}

/// One procedural segment along a track-shape path (shape-local frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackProceduralLink {
    pub shape_local_offset: [f64; 3],
    pub shape_local_yaw_deg: f64,
    pub dims: TrackProceduralDims,
}

impl TrackSectionDef {
    /// True when the section follows a circular arc (`SectionCurve` with non-zero radius and angle).
    pub fn is_curved(&self) -> bool {
        matches!(
            (self.curve_radius_m, self.curve_angle_deg),
            (Some(r), Some(a)) if r.abs() > 1e-9 && a.abs() > 1e-9
        )
    }

    /// True when the section is skew-only (rotation with no drawable length).
    pub fn is_skew_only(&self) -> bool {
        matches!(self.skew_deg, Some(skew) if skew.abs() > 1e-9)
            && !self.is_curved()
            && self.length_m.abs() <= 1e-9
    }

    /// Travel length along the section centreline (arc length for curves; excludes skew).
    pub fn effective_length_m(&self) -> f64 {
        if self.is_curved() {
            let r = self.curve_radius_m.unwrap().abs();
            let a = self.curve_angle_deg.unwrap().abs();
            r * a * std::f64::consts::PI / 180.0
        } else {
            self.length_m
        }
    }
}

/// Merged `tsection.dat` catalog (global + route overlays).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TSectionCatalog {
    pub sections: HashMap<u32, TrackSectionDef>,
    pub shapes: HashMap<u32, TrackShapeDef>,
}

impl TSectionCatalog {
    /// Load route overlay (`OpenRails/tsection.dat` or route root) with `include` recursion.
    pub fn load_for_route(route_dir: &Path) -> Result<Self, FormatError> {
        let mut catalog = Self::default();
        if let Some(path) = discover_route_tsection(route_dir) {
            Self::merge_file(&path, &mut catalog)?;
        } else if let Some(global) = discover_global_tsection(route_dir) {
            Self::merge_file(&global, &mut catalog)?;
        }
        Ok(catalog)
    }

    pub fn shape_file_name(&self, shape_idx: u32) -> Option<&str> {
        self.shapes.get(&shape_idx).map(|s| s.file_name.as_str())
    }

    pub fn clearance_dist_m(&self, shape_idx: u32) -> Option<f64> {
        self.shapes.get(&shape_idx)?.clearance_dist_m
    }

    pub fn is_junction_shape(&self, shape_idx: u32) -> bool {
        self.shapes.get(&shape_idx).is_some_and(|s| s.is_junction())
    }

    /// Length + half-gauge for procedural track from the shape's primary path (first section).
    pub fn procedural_dims(&self, shape_idx: u32) -> Option<TrackProceduralDims> {
        self.procedural_links(shape_idx)
            .into_iter()
            .next()
            .map(|l| l.dims)
    }

    /// Expand paths selected by [`TrackShapeDef::procedural_path_indices`] (respects `MainRoute`).
    pub fn procedural_links(&self, shape_idx: u32) -> Vec<TrackProceduralLink> {
        let Some(shape) = self.shapes.get(&shape_idx) else {
            return Vec::new();
        };
        self.links_for_paths(shape, &shape.procedural_path_indices())
    }

    /// Expand every path (ignores `MainRoute`).
    pub fn procedural_links_all_paths(&self, shape_idx: u32) -> Vec<TrackProceduralLink> {
        let Some(shape) = self.shapes.get(&shape_idx) else {
            return Vec::new();
        };
        let indices: Vec<usize> = (0..shape.paths.len()).collect();
        self.links_for_paths(shape, &indices)
    }

    /// Single path for debug overlays: `MainRoute` when set, otherwise path 0.
    pub fn procedural_links_primary_path(&self, shape_idx: u32) -> Vec<TrackProceduralLink> {
        let Some(shape) = self.shapes.get(&shape_idx) else {
            return Vec::new();
        };
        let path_index = shape
            .main_route
            .and_then(|main| {
                let idx = main as usize;
                (idx < shape.paths.len()).then_some(idx)
            })
            .unwrap_or(0);
        self.links_for_paths(shape, &[path_index])
    }

    fn links_for_paths(
        &self,
        shape: &TrackShapeDef,
        path_indices: &[usize],
    ) -> Vec<TrackProceduralLink> {
        let mut links = Vec::new();
        for &path_index in path_indices {
            let Some(path) = shape.paths.get(path_index) else {
                continue;
            };
            let ids = path_section_ids(path);
            if ids.is_empty() {
                continue;
            }
            let mut pos = path.offset;
            let mut yaw = path.angle_deg;
            for section_id in ids {
                let Some(section) = self.sections.get(&section_id) else {
                    continue;
                };
                if let Some(dims) = procedural_dims_for_section(section) {
                    links.push(TrackProceduralLink {
                        shape_local_offset: pos,
                        shape_local_yaw_deg: yaw,
                        dims,
                    });
                }
                let (dx, dy, dz, dyaw) = section_travel_delta(section);
                let yaw_rad = yaw.to_radians();
                let cos = yaw_rad.cos();
                let sin = yaw_rad.sin();
                pos[0] += dx * cos - dz * sin;
                pos[1] += dy;
                pos[2] += dx * sin + dz * cos;
                yaw += dyaw;
            }
        }
        links
    }

    fn merge_file(path: &Path, catalog: &mut Self) -> Result<(), FormatError> {
        let text = read_tsection_text(path)?;
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        for include_path in scan_includes(&text, base) {
            let resolved = resolve_path_case_insensitive(&include_path).unwrap_or(include_path);
            if resolved.is_file() {
                Self::merge_file(&resolved, catalog)?;
            }
        }
        for ast in parse_all_top_level_lenient(&text) {
            if let Ast::List(items) = ast {
                if matches_head(&items, "include") {
                    if let Some(include_path) = items.get(1).and_then(ast_to_include_path) {
                        let direct = base.join(&include_path);
                        let resolved = resolve_path_case_insensitive(&direct).unwrap_or(direct);
                        if resolved.is_file() {
                            Self::merge_file(&resolved, catalog)?;
                        }
                    }
                    continue;
                }
                collect_sections(&items, &mut catalog.sections);
                collect_shapes(&items, &mut catalog.shapes);
            }
        }
        scan_tsection_entries(&text, catalog);
        Ok(())
    }
}

fn path_section_ids(path: &TrackShapePath) -> Vec<u32> {
    if path.section_indices.is_empty() {
        return Vec::new();
    }
    let limit = path.num_sections.max(1) as usize;
    path.section_indices.iter().copied().take(limit).collect()
}

/// MSTS/Open Rails canonical radius when approximating `SectionSkew` as a micro-curve.
pub const SKEW_AS_CURVE_RADIUS_M: f64 = 0.001;
/// Minimum arc length for skew-only procedural meshes (pure 0.001 m arcs are invisible).
const SKEW_VISUAL_MIN_ARC_M: f64 = 0.25;

fn skew_visual_curve(section: &TrackSectionDef) -> Option<(f64, f64)> {
    let angle = section.skew_deg?;
    if angle.abs() <= 1e-9 {
        return None;
    }
    let angle_rad = angle.abs().to_radians();
    let radius = (SKEW_VISUAL_MIN_ARC_M / angle_rad).clamp(SKEW_AS_CURVE_RADIUS_M, 50.0);
    Some((radius, angle))
}

fn procedural_dims_for_section(section: &TrackSectionDef) -> Option<TrackProceduralDims> {
    if section.gauge_m <= 0.0 {
        return None;
    }
    if section.is_skew_only() {
        let (radius, angle) = skew_visual_curve(section)?;
        let length_m = radius * angle.abs().to_radians();
        return Some(TrackProceduralDims {
            length_m,
            half_gauge_m: section.gauge_m * 0.5,
            curve_radius_m: Some(radius),
            curve_angle_deg: Some(angle),
        });
    }
    if section.effective_length_m() <= 0.0 {
        return None;
    }
    Some(TrackProceduralDims {
        length_m: section.effective_length_m(),
        half_gauge_m: section.gauge_m * 0.5,
        curve_radius_m: section.curve_radius_m,
        curve_angle_deg: section.curve_angle_deg,
    })
}

/// End-of-section displacement in section-local space and yaw delta (degrees).
fn section_travel_delta(section: &TrackSectionDef) -> (f64, f64, f64, f64) {
    let skew = section.skew_deg.unwrap_or(0.0);
    if section.is_curved() {
        let (end, curve_dyaw) = arc_end_local(
            section.curve_radius_m.unwrap(),
            section.curve_angle_deg.unwrap(),
        );
        (end[0], end[1], end[2], curve_dyaw + skew)
    } else {
        (0.0, 0.0, section.length_m.max(0.0), skew)
    }
}

/// MSTS circular arc endpoint (start at origin, tangent +Z) — matches viewer `arc_local_frame`.
fn arc_end_local(radius_m: f64, total_angle_deg: f64) -> ([f64; 3], f64) {
    let theta_rad = total_angle_deg.to_radians();
    let r = radius_m.abs();
    let sign = if total_angle_deg >= 0.0 { 1.0 } else { -1.0 };
    let from_center_x = -sign * r;
    let cos = theta_rad.cos();
    let sin = theta_rad.sin();
    let rotated_x = from_center_x * cos;
    let rotated_z = -from_center_x * sin;
    let pos_x = sign * r + rotated_x;
    ([pos_x, 0.0, rotated_z], -total_angle_deg)
}

fn read_tsection_text(path: &Path) -> Result<String, FormatError> {
    let bytes = std::fs::read(path).map_err(|e| FormatError::UnexpectedToken {
        offset: 0,
        message: format!("failed to read {}: {e}", path.display()),
    })?;
    Ok(decode_msts_bytes(&bytes))
}

fn scan_includes(text: &str, base: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut pos = 0usize;
    while let Some(found) = text[pos..].find("include") {
        let abs = pos + found;
        if !tag_at_boundary(text, abs, "include".len()) {
            pos = abs + "include".len();
            continue;
        }
        let after_tag = abs + "include".len();
        let Some(rel_paren) = text[after_tag..].find('(') else {
            pos = abs + 1;
            continue;
        };
        let open = after_tag + rel_paren;
        if let Ok(Ast::List(items)) = parse_first(&text[open..]) {
            if let Some(rel) = items.first().and_then(ast_to_include_path) {
                paths.push(base.join(rel.trim()));
            }
        }
        pos = open + 1;
    }
    paths
}

fn insert_track_shape(out: &mut HashMap<u32, TrackShapeDef>, id: u32, incoming: TrackShapeDef) {
    out.entry(id)
        .and_modify(|existing| merge_track_shapes(existing, &incoming))
        .or_insert(incoming);
}

fn merge_track_shapes(existing: &mut TrackShapeDef, incoming: &TrackShapeDef) {
    let existing_score = shape_merge_score(existing);
    let incoming_score = shape_merge_score(incoming);
    if incoming_score > existing_score {
        *existing = incoming.clone();
    } else {
        if existing.main_route.is_none() {
            existing.main_route = incoming.main_route;
        }
        if existing.clearance_dist_m.is_none() {
            existing.clearance_dist_m = incoming.clearance_dist_m;
        }
    }
}

fn shape_merge_score(shape: &TrackShapeDef) -> u32 {
    let mut score = shape.paths.len() as u32;
    if shape.main_route.is_some() {
        score += 1000;
    }
    score
}

fn scan_tsection_entries(text: &str, catalog: &mut TSectionCatalog) {
    scan_tagged_entries(text, "TrackSection", |items| {
        if let Some((id, def)) = parse_track_section(items) {
            catalog.sections.insert(id, def);
        }
    });
    scan_tagged_entries(text, "TrackShape", |items| {
        if let Some((id, def)) = parse_track_shape(items) {
            insert_track_shape(&mut catalog.shapes, id, def);
        }
    });
}

fn scan_tagged_entries(text: &str, tag: &str, mut apply: impl FnMut(&[Ast])) {
    let mut pos = 0usize;
    while let Some(found) = text[pos..].find(tag) {
        let abs = pos + found;
        if !tag_at_boundary(text, abs, tag.len()) {
            pos = abs + tag.len();
            continue;
        }
        let after_tag = abs + tag.len();
        let Some(rel_paren) = text[after_tag..].find('(') else {
            pos = abs + 1;
            continue;
        };
        if !text[after_tag..after_tag + rel_paren]
            .chars()
            .all(|c| c.is_whitespace())
        {
            pos = abs + tag.len();
            continue;
        }
        let open = after_tag + rel_paren;
        if let Ok(Ast::List(items)) = parse_first(&text[open..]) {
            if matches_head(&items, tag) || items.first().and_then(ast_to_u32).is_some() {
                apply(&items);
            }
        }
        pos = open + 1;
    }
}

fn tag_at_boundary(text: &str, start: usize, len: usize) -> bool {
    let before_ok = start == 0
        || text.as_bytes()[start - 1].is_ascii_whitespace()
        || matches!(text.as_bytes()[start - 1], b'(' | b')');
    let end = start + len;
    let after_ok = end >= text.len()
        || text.as_bytes()[end].is_ascii_whitespace()
        || matches!(text.as_bytes()[end], b'(' | b')');
    before_ok && after_ok
}

fn discover_route_tsection(route_dir: &Path) -> Option<PathBuf> {
    [
        route_dir.join("OpenRails/tsection.dat"),
        route_dir.join("openrails/tsection.dat"),
        route_dir.join("tsection.dat"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

fn discover_global_tsection(route_dir: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(name) = route_dir.file_name() {
        if let Some(content) = route_dir.parent().and_then(|p| p.parent()) {
            candidates.push(content.join(name).join("GLOBAL/tsection.dat"));
            candidates.push(content.join(name).join("Global/tsection.dat"));
        }
    }
    candidates.push(route_dir.join("GLOBAL/tsection.dat"));
    candidates.push(route_dir.join("Global/tsection.dat"));
    candidates.push(route_dir.join("../GLOBAL/tsection.dat"));
    candidates.push(route_dir.join("../../GLOBAL/tsection.dat"));
    for candidate in candidates {
        if let Some(resolved) = resolve_path_case_insensitive(&candidate) {
            if resolved.is_file() {
                return Some(resolved);
            }
        }
    }
    None
}

fn ast_to_include_path(ast: &Ast) -> Option<String> {
    match ast {
        Ast::Atom(atom) => atom_to_string(atom),
        Ast::List(items) => items
            .get(1)
            .or_else(|| items.first())
            .and_then(|a| match a {
                Ast::Atom(atom) => atom_to_string(atom),
                _ => None,
            }),
    }
}

fn collect_sections(items: &[Ast], out: &mut HashMap<u32, TrackSectionDef>) {
    if matches_head(items, "TrackSections") {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "TrackSection") {
                    if let Some(def) = parse_track_section(sub) {
                        out.insert(def.0, def.1);
                    }
                }
            }
        }
        return;
    }
    for item in items {
        if let Ast::List(sub) = item {
            collect_sections(sub, out);
        }
    }
}

fn collect_shapes(items: &[Ast], out: &mut HashMap<u32, TrackShapeDef>) {
    if matches_head(items, "TrackShapes") {
        for item in items.iter().skip(1) {
            if let Ast::List(sub) = item {
                if matches_head(sub, "TrackShape") {
                    if let Some(def) = parse_track_shape(sub) {
                        insert_track_shape(out, def.0, def.1);
                    }
                }
            }
        }
        return;
    }
    for item in items {
        if let Ast::List(sub) = item {
            collect_shapes(sub, out);
        }
    }
}

fn parse_track_section(items: &[Ast]) -> Option<(u32, TrackSectionDef)> {
    let (id, body_start) = section_id_and_body(items)?;
    let mut gauge_m = 0.0;
    let mut length_m = 0.0;
    let mut curve_radius_m = None;
    let mut curve_angle_deg = None;
    let mut skew_deg = None;
    for item in items.iter().skip(body_start) {
        let Ast::List(sub) = item else { continue };
        if matches_head(sub, "SectionSize") {
            gauge_m = sub.get(1).and_then(ast_to_f64).unwrap_or(0.0);
            length_m = sub.get(2).and_then(ast_to_f64).unwrap_or(0.0);
        } else if matches_head(sub, "SectionCurve") {
            curve_radius_m = sub.get(1).and_then(ast_to_f64);
            curve_angle_deg = sub.get(2).and_then(ast_to_f64);
        } else if matches_head(sub, "SectionSkew") {
            skew_deg = sub.get(1).and_then(ast_to_f64);
        }
    }
    // MSTS flat form: `TrackSection ( ID SectionSize ( … ) SectionCurve ( … ) )`.
    let mut i = body_start;
    while i < items.len() {
        if let Ast::Atom(Atom::Symbol(tag)) = &items[i] {
            if tag.eq_ignore_ascii_case("SectionSize") && (gauge_m <= 0.0 || length_m <= 0.0) {
                i += 1;
                if let Some(Ast::List(sub)) = items.get(i) {
                    if gauge_m <= 0.0 {
                        gauge_m = sub.first().and_then(ast_to_f64).unwrap_or(0.0);
                    }
                    if length_m <= 0.0 {
                        length_m = sub.get(1).and_then(ast_to_f64).unwrap_or(0.0);
                    }
                }
            } else if tag.eq_ignore_ascii_case("SectionCurve") && curve_radius_m.is_none() {
                i += 1;
                if let Some(Ast::List(sub)) = items.get(i) {
                    curve_radius_m = sub.first().and_then(ast_to_f64);
                    curve_angle_deg = sub.get(1).and_then(ast_to_f64);
                }
            } else if tag.eq_ignore_ascii_case("SectionSkew") && skew_deg.is_none() {
                i += 1;
                if let Some(Ast::List(sub)) = items.get(i) {
                    skew_deg = sub.first().and_then(ast_to_f64);
                }
            }
        }
        i += 1;
    }
    Some((
        id,
        TrackSectionDef {
            gauge_m,
            length_m,
            curve_radius_m,
            curve_angle_deg,
            skew_deg,
        },
    ))
}

fn parse_track_shape(items: &[Ast]) -> Option<(u32, TrackShapeDef)> {
    let (id, body_start) = shape_id_and_body(items)?;
    let mut file_name = String::new();
    let mut main_route = None;
    let mut clearance_dist_m = None;
    let mut paths = Vec::new();
    let mut i = body_start;
    while i < items.len() {
        match &items[i] {
            Ast::List(sub) if matches_head(sub, "FileName") => {
                file_name = sub
                    .get(1)
                    .and_then(|a| match a {
                        Ast::Atom(atom) => atom_to_string(atom),
                        _ => None,
                    })?
                    .trim()
                    .to_string();
            }
            Ast::List(sub) if matches_head(sub, "MainRoute") => {
                main_route = sub.get(1).and_then(ast_to_u32);
            }
            Ast::List(sub) if matches_head(sub, "ClearanceDist") => {
                clearance_dist_m = sub.get(1).and_then(ast_to_f64);
            }
            Ast::List(sub) if matches_head(sub, "SectionIdx") => {
                if let Some(path) = parse_section_idx(sub) {
                    paths.push(path);
                }
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("FileName") => {
                i += 1;
                file_name = match items.get(i) {
                    Some(Ast::List(sub)) => sub.first().and_then(|a| match a {
                        Ast::Atom(atom) => atom_to_string(atom),
                        _ => None,
                    }),
                    Some(Ast::Atom(atom)) => atom_to_string(atom),
                    _ => None,
                }?
                .trim()
                .to_string();
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("MainRoute") => {
                i += 1;
                main_route = items.get(i).and_then(|a| match a {
                    Ast::List(sub) => sub.first().and_then(ast_to_u32),
                    other => ast_to_u32(other),
                });
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("ClearanceDist") => {
                i += 1;
                clearance_dist_m = items.get(i).and_then(|a| match a {
                    Ast::List(sub) => sub.first().and_then(ast_to_f64),
                    other => ast_to_f64(other),
                });
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("SectionIdx") => {
                i += 1;
                if let Some(Ast::List(sub)) = items.get(i) {
                    if let Some(path) = parse_section_idx_from_flat(sub) {
                        paths.push(path);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    if !is_shape_file_name(&file_name) {
        if let Some(found) = find_shape_file_name(items, body_start) {
            file_name = found;
        }
    }
    if !is_shape_file_name(&file_name) {
        return None;
    }
    Some((
        id,
        TrackShapeDef {
            file_name,
            main_route,
            clearance_dist_m,
            paths,
        },
    ))
}

fn is_shape_file_name(name: &str) -> bool {
    name.ends_with(".s") || name.ends_with(".S")
}

fn find_shape_file_name(items: &[Ast], body_start: usize) -> Option<String> {
    let mut i = body_start;
    while i < items.len() {
        match &items[i] {
            Ast::List(sub) if matches_head(sub, "FileName") => {
                if let Some(name) = sub.get(1).and_then(|a| match a {
                    Ast::Atom(atom) => atom_to_string(atom),
                    _ => None,
                }) {
                    let name = name.trim().to_string();
                    if is_shape_file_name(&name) {
                        return Some(name);
                    }
                }
            }
            Ast::Atom(Atom::Symbol(tag)) if tag.eq_ignore_ascii_case("FileName") => {
                i += 1;
                let name = match items.get(i) {
                    Some(Ast::List(sub)) => sub.first().and_then(|a| match a {
                        Ast::Atom(atom) => atom_to_string(atom),
                        _ => None,
                    }),
                    Some(Ast::Atom(atom)) => atom_to_string(atom),
                    _ => None,
                }?;
                let name = name.trim().to_string();
                if is_shape_file_name(&name) {
                    return Some(name);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn section_id_and_body(items: &[Ast]) -> Option<(u32, usize)> {
    if matches_head(items, "TrackSection") {
        let id = items.get(1).and_then(ast_to_u32)?;
        return Some((id, 2));
    }
    let id = items.first().and_then(ast_to_u32)?;
    Some((id, 1))
}

fn shape_id_and_body(items: &[Ast]) -> Option<(u32, usize)> {
    if matches_head(items, "TrackShape") {
        let id = items.get(1).and_then(ast_to_u32)?;
        return Some((id, 2));
    }
    let id = items.first().and_then(ast_to_u32)?;
    Some((id, 1))
}

fn parse_section_idx(items: &[Ast]) -> Option<TrackShapePath> {
    parse_section_idx_from_flat(&items[1..])
}

fn parse_section_idx_from_flat(values: &[Ast]) -> Option<TrackShapePath> {
    let nums: Vec<f64> = values.iter().filter_map(ast_to_f64).collect();
    if nums.is_empty() {
        return None;
    }
    let num_sections = nums[0].max(0.0) as u32;
    let offset = [
        nums.get(1).copied().unwrap_or(0.0),
        nums.get(2).copied().unwrap_or(0.0),
        nums.get(3).copied().unwrap_or(0.0),
    ];
    let angle_deg = nums.get(4).copied().unwrap_or(0.0);
    let section_indices: Vec<u32> = nums.iter().skip(5).map(|&n| n.max(0.0) as u32).collect();
    Some(TrackShapePath {
        num_sections,
        offset,
        angle_deg,
        section_indices,
    })
}

fn ast_to_u32(ast: &Ast) -> Option<u32> {
    ast_to_f64(ast).map(|n| n.max(0.0) as u32)
}

fn ast_to_f64(ast: &Ast) -> Option<f64> {
    match ast {
        Ast::Atom(atom) => atom_to_number(atom),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_msts_flat_track_shape_snippet() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/chiltern_tsection_snippet.dat");
        let text = std::fs::read_to_string(&path).expect("read");
        let shape_pos = text.find("TrackShape").expect("tag");
        let open = shape_pos + text[shape_pos..].find('(').expect("(");
        let parsed_ast = parse_first(&text[open..]).expect("parse shape list");
        let Ast::List(items) = parsed_ast else {
            panic!("expected list");
        };
        let parsed = parse_track_shape(&items).expect("shape fields");
        assert_eq!(parsed.0, 38508);
        assert_eq!(parsed.1.file_name, "ukfs_s_1x25m.s");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&path, &mut catalog).expect("merge");
        assert!(catalog.shapes.contains_key(&38508));
        assert!(catalog.sections.contains_key(&38508));
    }

    #[test]
    fn parse_skew_section_snippet() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/skew_tsection_snippet.dat");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&path, &mut catalog).expect("merge");
        let skew = catalog.sections.get(&9301).expect("skew section");
        assert!(skew.is_skew_only());
        assert!((skew.skew_deg.unwrap() + 0.55).abs() < 1e-3);
        let links = catalog.procedural_links(9301);
        assert_eq!(links.len(), 2, "skew micro-arc + following straight");
        assert!(links[0].dims.curve_radius_m.is_some());
        assert!((links[0].dims.curve_angle_deg.unwrap() + 0.55).abs() < 1e-3);
        assert!(
            links[0].dims.length_m >= SKEW_VISUAL_MIN_ARC_M - 0.01,
            "skew visual arc {}",
            links[0].dims.length_m
        );
        assert!((links[1].dims.length_m - 10.0).abs() < 1e-3);
        assert!(
            links[1].shape_local_yaw_deg.abs() > 0.5,
            "straight section should follow skew rotation"
        );
    }

    #[test]
    fn skew_only_msts_micro_radius_available() {
        let section = TrackSectionDef {
            gauge_m: 1.435,
            length_m: 0.0,
            curve_radius_m: None,
            curve_angle_deg: None,
            skew_deg: Some(-0.5529),
        };
        let (r, a) = skew_visual_curve(&section).expect("curve");
        assert!(r >= SKEW_AS_CURVE_RADIUS_M);
        assert!((a + 0.5529).abs() < 1e-4);
        let dims = procedural_dims_for_section(&section).expect("dims");
        assert!(dims.curve_radius_m.is_some());
        assert!((dims.length_m - SKEW_VISUAL_MIN_ARC_M).abs() < 0.02);
    }

    #[test]
    fn chiltern_skew_sections_when_content_present() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let catalog = TSectionCatalog::load_for_route(&route).expect("load");
        let left = catalog.sections.get(&192).expect("skew straight left");
        let right = catalog.sections.get(&193).expect("skew straight right");
        assert!((left.skew_deg.unwrap() + 3.44).abs() < 1e-2);
        assert!((right.skew_deg.unwrap() - 3.44).abs() < 1e-2);
        assert!((left.effective_length_m() - 43.62).abs() < 0.01);
    }

    #[test]
    fn parse_multipath_shape_snippet() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multipath_tsection_snippet.dat");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&path, &mut catalog).expect("merge");
        let shape = catalog.shapes.get(&9101).expect("multipath shape");
        assert_eq!(shape.paths.len(), 2);
        assert_eq!(shape.main_route, Some(0));
        assert!((shape.paths[0].offset[0] + 1.435).abs() < 1e-3);
        assert!((shape.paths[1].offset[0] - 1.435).abs() < 1e-3);
        let main_links = catalog.procedural_links(9101);
        assert_eq!(main_links.len(), 1);
        assert!((main_links[0].shape_local_offset[0] + 1.435).abs() < 1e-3);
        let all_links = catalog.procedural_links_all_paths(9101);
        assert_eq!(all_links.len(), 2);
        assert!((all_links[0].shape_local_offset[0] + 1.435).abs() < 1e-3);
        assert!((all_links[1].shape_local_offset[0] - 1.435).abs() < 1e-3);
    }

    #[test]
    fn parse_multiseg_shape_chains_sections() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/multiseg_tsection_snippet.dat");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&path, &mut catalog).expect("merge");
        let shape = catalog.shapes.get(&9201).expect("multiseg shape");
        assert_eq!(shape.paths[0].num_sections, 2);
        assert_eq!(shape.paths[0].section_indices, vec![9201, 9202]);
        let links = catalog.procedural_links(9201);
        assert_eq!(links.len(), 2);
        assert!((links[0].dims.length_m - 10.0).abs() < 1e-3);
        assert!(links[0].dims.curve_radius_m.is_none());
        assert!(
            (links[1].dims.length_m - (500.0 * 5.0 * std::f64::consts::PI / 180.0)).abs() < 0.05
        );
        assert!((links[1].shape_local_offset[2] - 10.0).abs() < 1e-2);
    }

    #[test]
    fn chiltern_multipath_shape19_when_content_present() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let catalog = TSectionCatalog::load_for_route(&route).expect("load");
        let shape = catalog.shapes.get(&19).expect("A4t500mStrt");
        assert!(
            shape.paths.len() >= 4,
            "expected quad paths, got {}",
            shape.paths.len()
        );
        let links = catalog.procedural_links(19);
        assert_eq!(links.len(), shape.paths.len());
    }

    #[test]
    fn chiltern_junction_shape1_main_route_when_content_present() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let catalog = TSectionCatalog::load_for_route(&route).expect("load");
        let shape = catalog.shapes.get(&1).expect("A1t45dYardRgt");
        assert_eq!(shape.main_route, Some(0));
        assert_eq!(shape.paths.len(), 2);
        assert_eq!(shape.file_name, "A1t45dYardRgt.s");
        assert!(shape.is_junction());
        assert_eq!(catalog.clearance_dist_m(1), Some(15.0));
        assert_eq!(catalog.procedural_links(1).len(), 1);
        assert_eq!(catalog.procedural_links_all_paths(1).len(), 3);
    }

    #[test]
    fn parse_clearance_dist_flat_and_nested() {
        let nested = r#"
TrackShape ( 42
 FileName ( turnout.s )
 MainRoute ( 1 )
 ClearanceDist ( 12.5 )
 SectionIdx ( 1 0 0 0 0 1 )
)
"#;
        let mut catalog = TSectionCatalog::default();
        scan_tsection_entries(nested, &mut catalog);
        let shape = catalog.shapes.get(&42).expect("nested");
        assert_eq!(shape.clearance_dist_m, Some(12.5));
        assert!(catalog.is_junction_shape(42));

        let flat = r#"
TrackShape ( 43
 FileName ( yard.s )
 ClearanceDist 18
 SectionIdx ( 1 0 0 0 0 1 )
 SectionIdx ( 1 1 0 0 0 1 )
)
"#;
        scan_tsection_entries(flat, &mut catalog);
        let yard = catalog.shapes.get(&43).expect("flat");
        assert_eq!(yard.clearance_dist_m, Some(18.0));
    }

    #[test]
    fn duplicate_shape_ids_prefer_main_route_entry() {
        let text = r#"
TrackShape ( 1
 FileName ( junction.s )
 NumPaths ( 2 )
 MainRoute ( 0 )
 ClearanceDist ( 15.0 )
 SectionIdx ( 1 0 0 0 0 10 )
 SectionIdx ( 1 1 0 0 0 10 )
)
TrackShape ( 1
 FileName ( straight.s )
 NumPaths ( 1 )
 SectionIdx ( 1 0 0 0 0 1 )
 SectionIdx ( 1 0 0 0 0 2 )
 SectionIdx ( 1 0 0 0 0 3 )
)
"#;
        let mut catalog = TSectionCatalog::default();
        scan_tsection_entries(text, &mut catalog);
        let shape = catalog.shapes.get(&1).expect("shape 1");
        assert_eq!(shape.main_route, Some(0));
        assert_eq!(shape.file_name, "junction.s");
        assert_eq!(shape.paths.len(), 2);
        assert_eq!(shape.clearance_dist_m, Some(15.0));
    }

    #[test]
    fn parse_curved_section_snippet() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/curved_tsection_snippet.dat");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&path, &mut catalog).expect("merge");
        let section = catalog.sections.get(&5005).expect("curved section");
        assert!((section.gauge_m - 1.435).abs() < 1e-3);
        assert!(section.is_curved());
        assert!((section.curve_radius_m.unwrap() - 500.0).abs() < 1e-3);
        assert!((section.curve_angle_deg.unwrap() + 5.0).abs() < 1e-3);
        let arc = section.effective_length_m();
        assert!((arc - (500.0 * 5.0 * std::f64::consts::PI / 180.0)).abs() < 1e-2);
        let dims = catalog.procedural_dims(5020).expect("curve shape dims");
        assert!((dims.length_m - arc).abs() < 1e-2);
        assert_eq!(dims.curve_radius_m, Some(500.0));
        assert_eq!(dims.curve_angle_deg, Some(-5.0));
    }

    #[test]
    fn chiltern_a1t500r5d_curve_dims_when_content_present() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let catalog = TSectionCatalog::load_for_route(&route).expect("load");
        let dims = catalog.procedural_dims(20).expect("A1t500r5d shape");
        assert!(dims.curve_radius_m.is_some());
        assert!(dims.curve_angle_deg.is_some());
        assert!(
            (dims.length_m - 43.633).abs() < 0.05,
            "arc length {}",
            dims.length_m
        );
    }

    #[test]
    fn parse_minimal_tsection_fixture() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal_tsection.dat");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&path, &mut catalog).expect("merge");
        let section = catalog.sections.get(&100).expect("section 100");
        assert!((section.gauge_m - 1.435).abs() < 1e-3);
        assert!((section.length_m - 25.0).abs() < 1e-3);
        let shape = catalog.shapes.get(&100).expect("shape 100");
        assert_eq!(shape.file_name, "test_straight.s");
        let dims = catalog.procedural_dims(100).expect("dims");
        assert!((dims.length_m - 25.0).abs() < 1e-3);
        assert!((dims.half_gauge_m - 0.7175).abs() < 1e-3);
    }

    #[test]
    fn chiltern_tsection_has_ukfs_shape_when_content_present() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let global = discover_global_tsection(&route).expect("GLOBAL tsection.dat should resolve");
        let mut catalog = TSectionCatalog::default();
        TSectionCatalog::merge_file(&global, &mut catalog).expect("global");
        assert!(
            catalog.shapes.contains_key(&38508),
            "UKFS 25m shape should be present ({} sections, {} shapes loaded)",
            catalog.sections.len(),
            catalog.shapes.len()
        );
        let dims = catalog.procedural_dims(38508).expect("dims");
        assert!((dims.length_m - 25.0).abs() < 1e-3);
    }

    #[test]
    fn chiltern_openrails_include_resolves_global_case() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let overlay = route.join("OpenRails/tsection.dat");
        if !overlay.is_file() {
            return;
        }
        let text = read_tsection_text(&overlay).expect("read overlay");
        let include_path = scan_includes(&text, overlay.parent().unwrap())
            .into_iter()
            .next()
            .expect("include path");
        let direct = include_path.clone();
        let resolved = resolve_path_case_insensitive(&direct).unwrap_or(direct.clone());
        assert!(
            resolved.is_file(),
            "include should resolve to GLOBAL tsection (direct={}, resolved={})",
            direct.display(),
            resolved.display()
        );
    }

    #[test]
    fn chiltern_route_tsection_includes_global_via_openrails() {
        let route =
            PathBuf::from("/home/cristian/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern");
        if !route.is_dir() {
            return;
        }
        let catalog = TSectionCatalog::load_for_route(&route).expect("load");
        assert!(
            catalog.shapes.contains_key(&38508),
            "route load: {} sections, {} shapes",
            catalog.sections.len(),
            catalog.shapes.len()
        );
    }
}

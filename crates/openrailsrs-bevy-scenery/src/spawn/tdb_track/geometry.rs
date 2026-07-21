//! `.tdb` vector-graph chord types and pure world-space helpers.
//!
//! Branch walking / chord collection lives in [`super::collect`] behind
//! injectable [`super::FocusQuery`] (apps adapt `RouteFocus` / tile focus).

use bevy::prelude::*;
use openrailsrs_formats::{
    TSectionCatalog, TrVectorSectionRecord, TrackDbFile, TrackNodeKind, TrackProceduralLink,
    TrackVectorGeometry, TrackVectorPoint,
};

pub use crate::spawn::dyntrack::{
    MSTS_DEFAULT_SECTION_LENGTH_M, MSTS_STANDARD_HALF_GAUGE_M, ProceduralTrackSegment,
    arc_local_frame,
};

/// Sentinel `section_index` for junction bridge chords (excluded from intra-node gap stats).
pub const TDB_JUNCTION_BRIDGE_SECTION: usize = usize::MAX;

#[derive(Clone, Copy, Debug)]
pub struct TdbChord {
    pub node_id: u32,
    pub section_index: usize,
    /// Sub-span within a section when TSection path has multiple links (0 = single span).
    pub span_index: u16,
    pub shape_idx: u32,
    pub start_world: Vec3,
    pub end_world: Vec3,
    pub curve_radius_m: Option<f32>,
    pub curve_angle_deg: Option<f32>,
}

/// One drawable piece along a `TrVectorSection` centreline (straight or arc).
#[derive(Clone, Copy, Debug)]
pub struct SectionPathSpan {
    pub start_world: Vec3,
    pub end_world: Vec3,
    pub world_yaw_deg: f64,
    pub half_gauge_m: Option<f32>,
    pub length_m: Option<f32>,
    pub curve_radius_m: Option<f32>,
    pub curve_angle_deg: Option<f32>,
}

impl SectionPathSpan {
    pub fn is_curved(&self) -> bool {
        matches!(
            (self.curve_radius_m, self.curve_angle_deg),
            (Some(r), Some(a)) if r.abs() > 1e-6 && a.abs() > 1e-6
        )
    }
}

/// True when a section should participate in `--track-dev` chord collection.
pub fn section_is_drawable(
    section: &TrVectorSectionRecord,
    tsection: Option<&TSectionCatalog>,
) -> bool {
    if section.shape_idx != 0 {
        if let Some(cat) = tsection {
            return !cat.is_road_shape(section.shape_idx);
        }
        return true;
    }
    let Some(cat) = tsection else {
        return false;
    };
    cat.procedural_dims(0).is_some() || cat.sections.contains_key(&0)
}

pub fn chord_heading_and_length(from: Vec3, to: Vec3) -> Option<(f64, f32)> {
    let dx = to.x - from.x;
    let dz = to.z - from.z;
    let len = (dx * dx + dz * dz).sqrt();
    if len < 0.5 {
        return None;
    }
    Some((f64::from(dx).atan2(f64::from(dz)).to_degrees(), len))
}

pub fn end_from_heading(start: Vec3, heading_deg: f64, length_m: f32) -> Vec3 {
    let yaw = heading_deg.to_radians() as f32;
    start + Vec3::new(yaw.sin() * length_m, 0.0, yaw.cos() * length_m)
}

pub fn single_section_length(node_length_m: f64, _shape_idx: u32) -> f32 {
    if node_length_m > 0.5 {
        return node_length_m as f32;
    }
    MSTS_DEFAULT_SECTION_LENGTH_M
}

pub fn section_shape_length_m(
    tsection: Option<&TSectionCatalog>,
    shape_idx: u32,
    node_length_m: f64,
    section_count: usize,
) -> f32 {
    if let Some(cat) = tsection {
        if let Some(dims) = cat.procedural_dims(shape_idx) {
            if dims.length_m > 0.5 {
                return dims.length_m as f32;
            }
        }
    }
    if section_count <= 1 {
        return single_section_length(node_length_m, shape_idx);
    }
    if node_length_m > 0.5 {
        return (node_length_m / section_count as f64) as f32;
    }
    MSTS_DEFAULT_SECTION_LENGTH_M
}

/// Map a flat local XZ offset into world XZ using MSTS/Bevy heading (tangent +Z at yaw 0).
pub fn local_flat_to_world(dx: f64, dz: f64, heading_deg: f64) -> (f32, f32) {
    let r = heading_deg.to_radians();
    let c = r.cos();
    let s = r.sin();
    let wx = dx * c + dz * s;
    let wz = -dx * s + dz * c;
    (wx as f32, wz as f32)
}

/// Open Rails `FindLocationInSection` — position at `distance_m` along the section centreline.
pub fn find_location_in_section_world(
    section: TrVectorSectionRecord,
    distance_m: f64,
    tsection: Option<&TSectionCatalog>,
    near_hint: Option<Vec3>,
    node_length_m: f64,
    section_count: usize,
) -> Option<Vec3> {
    if distance_m <= 1e-6 {
        return Some(section_world_vec3(section, near_hint));
    }
    let spans = section_path_spans(
        section,
        tsection,
        near_hint,
        node_length_m,
        section_count,
        None,
    );
    if spans.is_empty() {
        return None;
    }
    let last_end = spans.last().unwrap().end_world;
    let mut remaining = distance_m;
    for span in &spans {
        let span_len = span_length_m(*span);
        if remaining <= span_len + 1e-6 {
            return Some(point_along_span(*span, remaining));
        }
        remaining -= span_len;
    }
    Some(last_end)
}

fn span_length_m(span: SectionPathSpan) -> f64 {
    if span.is_curved() {
        let r = f64::from(span.curve_radius_m.unwrap().abs());
        let a = f64::from(span.curve_angle_deg.unwrap().abs());
        return r * a.to_radians();
    }
    span.length_m
        .map(f64::from)
        .unwrap_or_else(|| f64::from(distance_xz(span.start_world, span.end_world)))
}

fn distance_xz(a: Vec3, b: Vec3) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

fn point_along_span(span: SectionPathSpan, distance_m: f64) -> Vec3 {
    if span.is_curved() {
        let r = span.curve_radius_m.unwrap();
        let angle = span.curve_angle_deg.unwrap();
        let span_len = span_length_m(span);
        let fraction = (distance_m / span_len).clamp(0.0, 1.0) as f32;
        let (local, _) = arc_local_frame(r, angle, fraction);
        let (wx, wz) =
            local_flat_to_world(f64::from(local.x), f64::from(local.z), span.world_yaw_deg);
        return span.start_world + Vec3::new(wx, 0.0, wz);
    }
    let span_len = span_length_m(span).max(1e-6);
    let t = (distance_m / span_len).clamp(0.0, 1.0) as f32;
    span.start_world.lerp(span.end_world, t)
}

/// Walk the TSection primary path for one `TrVectorSection` in world space.
pub fn section_path_spans(
    section: TrVectorSectionRecord,
    tsection: Option<&TSectionCatalog>,
    near_hint: Option<Vec3>,
    node_length_m: f64,
    section_count: usize,
    next_section_anchor: Option<Vec3>,
) -> Vec<SectionPathSpan> {
    if !section_is_drawable(&section, tsection) {
        return Vec::new();
    }
    let anchor = section_world_vec3(section, near_hint);
    let base_yaw = section.heading_deg().unwrap_or(0.0);
    let Some(cat) = tsection else {
        if let Some(next) = next_section_anchor {
            if let Some(span) = straight_span_to(next, anchor, None) {
                return vec![span];
            }
            if section_count <= 1 {
                let len =
                    section_shape_length_m(None, section.shape_idx, node_length_m, section_count);
                if len >= 0.5 {
                    return vec![straight_span(anchor, base_yaw, len, None)];
                }
            }
            return Vec::new();
        }
        let len = section_shape_length_m(None, section.shape_idx, node_length_m, section_count);
        if len < 0.5 {
            return Vec::new();
        }
        return vec![straight_span(anchor, base_yaw, len, None)];
    };
    let links = cat.procedural_links_primary_path(section.shape_idx);
    if links.is_empty() {
        if let Some(next) = next_section_anchor {
            let half = cat
                .procedural_dims(section.shape_idx)
                .map(|d| d.half_gauge_m as f32);
            if let Some(span) = straight_span_to(next, anchor, half) {
                return vec![span];
            }
            if section_count <= 1 {
                let len = section_shape_length_m(
                    Some(cat),
                    section.shape_idx,
                    node_length_m,
                    section_count,
                );
                if len >= 0.5 {
                    return vec![straight_span(anchor, base_yaw, len, half)];
                }
            }
            return Vec::new();
        }
        let len =
            section_shape_length_m(Some(cat), section.shape_idx, node_length_m, section_count);
        if len < 0.5 {
            return Vec::new();
        }
        let half = cat
            .procedural_dims(section.shape_idx)
            .map(|d| d.half_gauge_m as f32);
        return vec![straight_span(anchor, base_yaw, len, half)];
    }
    let mut spans: Vec<SectionPathSpan> = links
        .iter()
        .map(|link| span_from_link(anchor, base_yaw, link))
        .collect();
    if let Some(next) = next_section_anchor {
        if let Some(last) = spans.last_mut() {
            if !last.is_curved() {
                if let Some(span) = straight_span_to(next, last.start_world, last.half_gauge_m) {
                    *last = span;
                }
            }
        }
    }
    spans
}

fn straight_span(
    start: Vec3,
    yaw_deg: f64,
    length_m: f32,
    half_gauge_m: Option<f32>,
) -> SectionPathSpan {
    let (wx, wz) = local_flat_to_world(0.0, f64::from(length_m), yaw_deg);
    SectionPathSpan {
        start_world: start,
        end_world: start + Vec3::new(wx, 0.0, wz),
        world_yaw_deg: yaw_deg,
        half_gauge_m,
        length_m: Some(length_m),
        curve_radius_m: None,
        curve_angle_deg: None,
    }
}

fn straight_span_to(end: Vec3, start: Vec3, half_gauge_m: Option<f32>) -> Option<SectionPathSpan> {
    let len = distance_xz(start, end);
    if len < 0.5 {
        return None;
    }
    let yaw_deg = f64::from((end.x - start.x).atan2(end.z - start.z)).to_degrees();
    Some(SectionPathSpan {
        start_world: start,
        end_world: end,
        world_yaw_deg: yaw_deg,
        half_gauge_m,
        length_m: Some(len),
        curve_radius_m: None,
        curve_angle_deg: None,
    })
}

fn span_from_link(anchor: Vec3, base_yaw: f64, link: &TrackProceduralLink) -> SectionPathSpan {
    let link_yaw = base_yaw + link.shape_local_yaw_deg;
    let (lx, _, lz) = (
        link.shape_local_offset[0],
        link.shape_local_offset[1],
        link.shape_local_offset[2],
    );
    let (ox, oz) = local_flat_to_world(lx, lz, base_yaw);
    let start = anchor + Vec3::new(ox, 0.0, oz);
    let half_gauge = Some(link.dims.half_gauge_m as f32);
    if let (Some(r_m), Some(a_deg)) = (link.dims.curve_radius_m, link.dims.curve_angle_deg) {
        let r = r_m as f32;
        let a = a_deg as f32;
        let (local_end, _) = arc_local_frame(r, a, 1.0);
        let (ex, ez) =
            local_flat_to_world(f64::from(local_end.x), f64::from(local_end.z), link_yaw);
        return SectionPathSpan {
            start_world: start,
            end_world: start + Vec3::new(ex, 0.0, ez),
            world_yaw_deg: link_yaw,
            half_gauge_m: half_gauge,
            length_m: None,
            curve_radius_m: Some(r),
            curve_angle_deg: Some(a),
        };
    }
    let len = link.dims.length_m as f32;
    let (ex, ez) = local_flat_to_world(0.0, f64::from(len), link_yaw);
    SectionPathSpan {
        start_world: start,
        end_world: start + Vec3::new(ex, 0.0, ez),
        world_yaw_deg: link_yaw,
        half_gauge_m: half_gauge,
        length_m: Some(len),
        curve_radius_m: None,
        curve_angle_deg: None,
    }
}

/// Curve metadata when a section is a single TSection arc (for audit chords anchor→anchor).
pub fn section_single_curve_metadata(
    section: TrVectorSectionRecord,
    tsection: Option<&TSectionCatalog>,
    near_hint: Option<Vec3>,
    node_length_m: f64,
    section_count: usize,
) -> (Option<f32>, Option<f32>) {
    let spans = section_path_spans(
        section,
        tsection,
        near_hint,
        node_length_m,
        section_count,
        None,
    );
    if spans.len() == 1 && spans[0].is_curved() {
        (spans[0].curve_radius_m, spans[0].curve_angle_deg)
    } else {
        (None, None)
    }
}

/// Chords for audit + one envelope per section; render expands via [`section_path_spans`].
#[allow(clippy::too_many_arguments)]
pub fn section_path_envelope_chords(
    node_id: u32,
    section_index: usize,
    section: TrVectorSectionRecord,
    tsection: Option<&TSectionCatalog>,
    near_hint: Option<Vec3>,
    node_length_m: f64,
    section_count: usize,
    next_section_anchor: Option<Vec3>,
) -> (Vec<TdbChord>, Vec3) {
    let spans = section_path_spans(
        section,
        tsection,
        near_hint,
        node_length_m,
        section_count,
        next_section_anchor,
    );
    if spans.is_empty() {
        return (Vec::new(), near_hint.unwrap_or(Vec3::ZERO));
    }
    let start = spans.first().unwrap().start_world;
    let end = spans.last().unwrap().end_world;
    let (curve_radius_m, curve_angle_deg) = if spans.len() == 1 && spans[0].is_curved() {
        (spans[0].curve_radius_m, spans[0].curve_angle_deg)
    } else {
        (None, None)
    };
    let chord = TdbChord {
        node_id,
        section_index,
        span_index: 0,
        shape_idx: section.shape_idx,
        start_world: start,
        end_world: end,
        curve_radius_m,
        curve_angle_deg,
    };
    (vec![chord], end)
}

#[allow(clippy::too_many_arguments)]
pub fn section_path_span_chords(
    node_id: u32,
    section_index: usize,
    section: TrVectorSectionRecord,
    tsection: Option<&TSectionCatalog>,
    near_hint: Option<Vec3>,
    node_length_m: f64,
    section_count: usize,
    next_section_anchor: Option<Vec3>,
) -> (Vec<TdbChord>, Vec3) {
    let spans = section_path_spans(
        section,
        tsection,
        near_hint,
        node_length_m,
        section_count,
        next_section_anchor,
    );
    if spans.is_empty() {
        return (Vec::new(), near_hint.unwrap_or(Vec3::ZERO));
    }
    let mut out = Vec::with_capacity(spans.len());
    for (span_index, span) in spans.iter().enumerate() {
        out.push(TdbChord {
            node_id,
            section_index,
            span_index: span_index as u16,
            shape_idx: section.shape_idx,
            start_world: span.start_world,
            end_world: span.end_world,
            curve_radius_m: span.curve_radius_m,
            curve_angle_deg: span.curve_angle_deg,
        });
    }
    let end = spans.last().unwrap().end_world;
    (out, end)
}

pub fn procedural_segment_from_span(span: SectionPathSpan) -> ProceduralTrackSegment {
    ProceduralTrackSegment {
        position: span.start_world,
        rotation: Quat::from_rotation_y(span.world_yaw_deg.to_radians() as f32),
        length_m: span.length_m,
        half_gauge_m: span.half_gauge_m,
        curve_radius_m: span.curve_radius_m,
        curve_angle_deg: span.curve_angle_deg,
    }
}

pub fn procedural_segment_from_chord(chord: TdbChord) -> Option<ProceduralTrackSegment> {
    if let (Some(r), Some(a)) = (chord.curve_radius_m, chord.curve_angle_deg) {
        let (_, rot) = arc_local_frame(r, a, 0.0);
        return Some(ProceduralTrackSegment {
            position: chord.start_world,
            rotation: rot,
            length_m: None,
            half_gauge_m: Some(MSTS_STANDARD_HALF_GAUGE_M),
            curve_radius_m: chord.curve_radius_m,
            curve_angle_deg: chord.curve_angle_deg,
        });
    }
    let (heading_deg, length_m) = chord_heading_and_length(chord.start_world, chord.end_world)?;
    Some(straight_segment_from_heading(
        chord.start_world,
        heading_deg,
        length_m,
        None,
    ))
}

pub fn straight_segment_from_tsection_link(
    position: Vec3,
    rotation: Quat,
    length_m: f32,
    link: Option<&TrackProceduralLink>,
) -> ProceduralTrackSegment {
    let half_gauge = link
        .map(|l| l.dims.half_gauge_m as f32)
        .or(Some(MSTS_STANDARD_HALF_GAUGE_M));

    ProceduralTrackSegment {
        position,
        rotation,
        length_m: Some(length_m),
        half_gauge_m: half_gauge,
        curve_radius_m: None,
        curve_angle_deg: None,
    }
}

pub fn straight_segment_from_heading(
    position: Vec3,
    heading_deg: f64,
    length_m: f32,
    half_gauge_m: Option<f32>,
) -> ProceduralTrackSegment {
    ProceduralTrackSegment {
        position,
        rotation: Quat::from_rotation_y(heading_deg.to_radians() as f32),
        length_m: Some(length_m),
        half_gauge_m: half_gauge_m.or(Some(MSTS_STANDARD_HALF_GAUGE_M)),
        curve_radius_m: None,
        curve_angle_deg: None,
    }
}

pub fn section_world_vec3(section: TrVectorSectionRecord, near_hint: Option<Vec3>) -> Vec3 {
    let (dx, _, dz) = section.start.bevy_position();
    let (near_x, near_z) = near_hint.map(|h| (h.x, h.z)).unwrap_or((dx, dz));
    let (x, y, z) = section.bevy_position_nearest_to(
        near_x,
        near_z,
        Some((section.header_tile_x, section.header_tile_z)),
    );
    Vec3::new(x, y, z)
}

pub fn point_world_vec3(
    point: TrackVectorPoint,
    header_tile: (i32, i32),
    near_hint: Option<Vec3>,
) -> Vec3 {
    let (dx, _, dz) = point.bevy_position();
    let (near_x, near_z) = near_hint.map(|h| (h.x, h.z)).unwrap_or((dx, dz));
    let (x, y, z) =
        point.bevy_position_nearest_to(near_x, near_z, Some(header_tile), Some(header_tile));
    Vec3::new(x, y, z)
}

/// Terminus of a lone or trailing section when there is no next section anchor.
pub fn single_section_end_world(
    section: TrVectorSectionRecord,
    geometry: Option<TrackVectorGeometry>,
    node_length_m: f64,
    reversed: bool,
    near_hint: Option<Vec3>,
    tsection: Option<&TSectionCatalog>,
    section_count: usize,
) -> Option<Vec3> {
    let start = section_world_vec3(section, near_hint);
    if let Some(geom) = geometry {
        let header = (section.header_tile_x, section.header_tile_z);
        let end_pt = point_world_vec3(geom.end, header, near_hint);
        if distance_xz(start, end_pt) >= 0.5 {
            return Some(end_pt);
        }
    }
    let spans = section_path_spans(
        section,
        tsection,
        near_hint,
        node_length_m,
        section_count,
        None,
    );
    if let Some(last) = spans.last() {
        if distance_xz(start, last.end_world) >= 0.5 {
            return Some(last.end_world);
        }
    }
    let heading = section.heading_deg()?;
    let len = section_shape_length_m(tsection, section.shape_idx, node_length_m, section_count);
    let h = if reversed { heading + 180.0 } else { heading };
    Some(end_from_heading(start, h, len))
}

/// Junction-facing endpoint: section path origin or terminus for the entry pin.
pub fn vector_junction_face_world(
    sections: &[TrVectorSectionRecord],
    entry_pin: usize,
    tsection: Option<&TSectionCatalog>,
    junction_hint: Vec3,
    node_length_m: f64,
) -> Option<Vec3> {
    let drawable: Vec<_> = sections
        .iter()
        .copied()
        .filter(|s| section_is_drawable(s, tsection))
        .collect();
    if drawable.is_empty() {
        return None;
    }
    let section_count = drawable.len();
    let ordered: Vec<_> = if entry_pin == 0 {
        drawable
    } else {
        drawable.into_iter().rev().collect()
    };
    let near = Some(junction_hint);
    if entry_pin == 0 {
        let first = ordered[0];
        let spans = section_path_spans(first, tsection, near, node_length_m, section_count, None);
        if let Some(s) = spans.first() {
            return Some(s.start_world);
        }
        Some(section_world_vec3(first, near))
    } else {
        let last = ordered[ordered.len() - 1];
        let spans = section_path_spans(last, tsection, near, node_length_m, section_count, None);
        if let Some(s) = spans.last() {
            return Some(s.end_world);
        }
        Some(section_world_vec3(last, near))
    }
}

/// World pose on the TDB centreline (port of OR `FindLocationInSection` / TSRE `getDrawPositionOnTrNode`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPose {
    pub position: Vec3,
    pub yaw_deg: f32,
}

/// Position + heading on a `.tdb` node at `chainage_m` metres from the vector start.
pub fn tdb_node_track_pose(
    tdb: &TrackDbFile,
    node_id: u32,
    chainage_m: f64,
    tsection: Option<&TSectionCatalog>,
    near_hint: Option<Vec3>,
) -> Option<TrackPose> {
    let node = tdb.node_by_id(node_id)?;
    match &node.kind {
        TrackNodeKind::Vector {
            length_m,
            sections,
            geometry,
            ..
        } => {
            if sections.is_empty() {
                if let Some(geom) = geometry {
                    let (x0, y0, z0) = geom.start.bevy_position();
                    let (x1, _, z1) = geom.end.bevy_position();
                    let start = Vec3::new(x0, y0, z0);
                    let end = Vec3::new(x1, y0, z1);
                    let t = if *length_m > 1e-6 {
                        (chainage_m / length_m).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    };
                    let pos = start.lerp(end, t);
                    let yaw = (end.x - start.x).atan2(end.z - start.z).to_degrees();
                    return Some(TrackPose {
                        position: pos,
                        yaw_deg: yaw,
                    });
                }
                return None;
            }
            let section_count = sections.len();
            let mut accumulated = 0.0;
            for (idx, section) in sections.iter().enumerate() {
                let next_anchor = sections
                    .get(idx + 1)
                    .map(|s| section_world_vec3(*s, Some(section_world_vec3(*section, near_hint))));
                let spans = section_path_spans(
                    *section,
                    tsection,
                    near_hint,
                    *length_m,
                    section_count,
                    next_anchor,
                );
                for span in &spans {
                    let span_len = span_length_m(*span);
                    if chainage_m <= accumulated + span_len + 1e-6 {
                        let along = (chainage_m - accumulated).max(0.0);
                        let pos = point_along_span(*span, along);
                        return Some(TrackPose {
                            position: pos,
                            yaw_deg: span.world_yaw_deg as f32,
                        });
                    }
                    accumulated += span_len;
                }
            }
            sections.last().and_then(|section| {
                find_location_in_section_world(
                    *section,
                    chainage_m,
                    tsection,
                    near_hint,
                    *length_m,
                    section_count,
                )
                .map(|pos| TrackPose {
                    position: pos,
                    yaw_deg: section.heading_deg().unwrap_or(0.0) as f32,
                })
            })
        }
        TrackNodeKind::Junction { .. } | TrackNodeKind::End => node.position.map(|p| {
            let (x, y, z) = p.bevy_position_nearest_to(
                near_hint.map(|v| v.x).unwrap_or(0.0),
                near_hint.map(|v| v.z).unwrap_or(0.0),
                near_hint.map(|_| (p.tile_x, p.tile_z)),
                Some((p.tile_x, p.tile_z)),
            );
            TrackPose {
                position: Vec3::new(x, y, z),
                yaw_deg: 0.0,
            }
        }),
    }
}

/// Nearest point on any TDB vector segment within `radius_m` of `world_xz` (TSRE `findNearestPositionOnTDB` spec).
pub fn nearest_track_position(
    tdb: &TrackDbFile,
    world_xz: Vec2,
    radius_m: f32,
    tsection: Option<&TSectionCatalog>,
    tile_filter: Option<(i32, i32)>,
) -> Option<TrackPose> {
    let mut best_dist = f64::from(radius_m);
    let mut best: Option<TrackPose> = None;
    let tile_index = tile_filter.is_some().then(|| tdb.index_nodes_by_tile());
    let candidate_nodes: Vec<u32> = if let (Some((tx, tz)), Some(index)) = (tile_filter, tile_index)
    {
        index.get(&(tx, tz)).cloned().unwrap_or_default()
    } else {
        tdb.nodes.iter().map(|n| n.id).collect()
    };
    for node_id in candidate_nodes {
        let Some(node) = tdb.node_by_id(node_id) else {
            continue;
        };
        let TrackNodeKind::Vector {
            length_m, sections, ..
        } = &node.kind
        else {
            continue;
        };
        if sections.is_empty() {
            continue;
        }
        let section_count = sections.len();
        for (idx, section) in sections.iter().enumerate() {
            let near = None;
            let next_anchor = sections
                .get(idx + 1)
                .map(|s| section_world_vec3(*s, Some(section_world_vec3(*section, near))));
            let spans = section_path_spans(
                *section,
                tsection,
                near,
                *length_m,
                section_count,
                next_anchor,
            );
            for span in spans {
                let dist = point_segment_distance_xz(world_xz, span.start_world, span.end_world);
                if dist >= best_dist {
                    continue;
                }
                let seg_len = distance_xz(span.start_world, span.end_world).max(1e-6);
                let t = ((world_xz - Vec2::new(span.start_world.x, span.start_world.z)).dot(
                    Vec2::new(
                        span.end_world.x - span.start_world.x,
                        span.end_world.z - span.start_world.z,
                    ),
                ) / (seg_len * seg_len))
                    .clamp(0.0, 1.0);
                let pos = span.start_world.lerp(span.end_world, t);
                best_dist = dist;
                best = Some(TrackPose {
                    position: pos,
                    yaw_deg: span.world_yaw_deg as f32,
                });
            }
        }
    }
    best
}

fn point_segment_distance_xz(p: Vec2, a: Vec3, b: Vec3) -> f64 {
    let a_xz = Vec2::new(a.x, a.z);
    let ab = Vec2::new(b.x - a.x, b.z - a.z);
    let ab_len_sq = ab.length_squared().max(1e-9);
    let t = ((p - a_xz).dot(ab) / ab_len_sq).clamp(0.0, 1.0);
    let closest = a_xz + ab * t;
    // `closest` is absolute XZ; must compare against `p`, not the relative `p - a`.
    f64::from(p.distance(closest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::TrackDbNode;
    use openrailsrs_formats::typed::{TrackSectionDef, TrackShapeDef, TrackShapePath};

    fn section_at(x: f64, z: f64, shape_idx: u32) -> TrVectorSectionRecord {
        let start = TrackVectorPoint {
            tile_x: 0,
            tile_z: 0,
            x,
            y: 0.0,
            z,
        };
        TrVectorSectionRecord {
            shape_idx,
            aux_shape_idx: 0,
            header_tile_x: start.tile_x,
            header_tile_z: start.tile_z,
            start,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
        }
    }

    fn catalog_with_straight_shape(shape_idx: u32, length_m: f64) -> TSectionCatalog {
        let mut catalog = TSectionCatalog::default();
        catalog.sections.insert(
            shape_idx,
            TrackSectionDef {
                gauge_m: 1.435,
                length_m,
                curve_radius_m: None,
                curve_angle_deg: None,
                skew_deg: None,
            },
        );
        catalog.shapes.insert(
            shape_idx,
            TrackShapeDef {
                file_name: format!("test_{shape_idx}.s"),
                road_shape: false,
                paths: vec![TrackShapePath {
                    offset: [0.0, 0.0, 0.0],
                    angle_deg: 0.0,
                    num_sections: 1,
                    section_indices: vec![shape_idx],
                }],
                main_route: Some(0),
                clearance_dist_m: None,
            },
        );
        catalog
    }

    fn catalog_with_curve_shape() -> TSectionCatalog {
        let mut catalog = TSectionCatalog::default();
        catalog.sections.insert(
            5005,
            TrackSectionDef {
                gauge_m: 1.435,
                length_m: 0.0,
                curve_radius_m: Some(500.0),
                curve_angle_deg: Some(-5.0),
                skew_deg: None,
            },
        );
        catalog.shapes.insert(
            99,
            TrackShapeDef {
                file_name: "curve.s".into(),
                road_shape: false,
                paths: vec![TrackShapePath {
                    offset: [0.0, 0.0, 0.0],
                    angle_deg: 0.0,
                    num_sections: 1,
                    section_indices: vec![5005],
                }],
                main_route: Some(0),
                clearance_dist_m: None,
            },
        );
        catalog
    }

    #[test]
    fn chord_heading_turns_at_right_angle() {
        let (h, len) =
            chord_heading_and_length(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 100.0))
                .unwrap();
        assert!((len - 141.42).abs() < 0.1);
        assert!((h - 45.0).abs() < 0.1);
    }

    #[test]
    fn chained_rebase_keeps_consecutive_chord_endpoints_aligned() {
        let s0 = section_at(0.0, 0.0, 1);
        let s1 = section_at(100.0, 0.0, 2);
        let s2 = section_at(200.0, 0.0, 3);
        let start0 = section_world_vec3(s0, None);
        let end0 = section_world_vec3(s1, Some(start0));
        let end1 = section_world_vec3(s2, Some(end0));
        assert!((end0 - section_world_vec3(s1, Some(start0))).length() < 1e-4);
        assert!((end1 - section_world_vec3(s2, Some(end0))).length() < 1e-4);
    }

    #[test]
    fn shape_idx_zero_drawable_with_section_zero_catalog() {
        let section = section_at(0.0, 0.0, 0);
        let cat = catalog_with_straight_shape(0, 1000.0);
        assert!(section_is_drawable(&section, Some(&cat)));
        let spans = section_path_spans(section, Some(&cat), None, 0.0, 1, None);
        assert_eq!(spans.len(), 1);
        assert!((span_length_m(spans[0]) - 1000.0).abs() < 1.0);
    }

    #[test]
    fn curved_section_span_is_longer_than_chord() {
        let mut section = section_at(0.0, 0.0, 99);
        section.ay = 0.0;
        let cat = catalog_with_curve_shape();
        let spans = section_path_spans(section, Some(&cat), None, 0.0, 1, None);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].is_curved());
        let arc_len = span_length_m(spans[0]);
        let chord = distance_xz(spans[0].start_world, spans[0].end_world);
        assert!(arc_len > f64::from(chord));
    }

    #[test]
    fn find_location_reaches_arc_end() {
        let section = section_at(0.0, 0.0, 99);
        let cat = catalog_with_curve_shape();
        let spans = section_path_spans(section, Some(&cat), None, 0.0, 1, None);
        let end = find_location_in_section_world(
            section,
            span_length_m(spans[0]),
            Some(&cat),
            None,
            0.0,
            1,
        )
        .unwrap();
        assert!((end - spans[0].end_world).length() < 0.05);
    }

    #[test]
    fn nearest_track_position_finds_point_on_section() {
        let section = section_at(0.0, 0.0, 1);
        let cat = catalog_with_straight_shape(1, 100.0);
        let mut tdb = TrackDbFile::default();
        tdb.nodes.push(TrackDbNode {
            id: 1,
            position: Some(TrackVectorPoint {
                tile_x: 0,
                tile_z: 0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            pin_refs: Vec::new(),
            kind: TrackNodeKind::Vector {
                length_m: 100.0,
                speed_limit_mps: 0.0,
                pins: (0, 0),
                item_ids: Vec::new(),
                sections: vec![section],
                geometry: None,
            },
        });
        let start = section_world_vec3(section, None);
        let spans = section_path_spans(section, Some(&cat), None, 100.0, 1, None);
        assert!(!spans.is_empty());
        let mid = spans[0].start_world.lerp(spans[0].end_world, 0.5);
        // 10 m beside the mid-span in world XZ.
        let query = Vec2::new(mid.x + 10.0, mid.z);
        let pose = nearest_track_position(&tdb, query, 50.0, Some(&cat), Some((0, 0)))
            .expect("nearest within 50 m");
        let dist = Vec2::new(query.x - pose.position.x, query.y - pose.position.z).length();
        assert!(
            (dist - 10.0).abs() < 0.5,
            "expected ~10 m lateral distance, got {dist} (start={start:?} pose={:?})",
            pose.position
        );
    }

    #[test]
    fn point_segment_distance_uses_absolute_coordinates() {
        // Regression #27: relative `ap.distance(closest)` yielded ~world-magnitude errors.
        let d = point_segment_distance_xz(
            Vec2::new(50.0, 10.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(100.0, 0.0, 0.0),
        );
        assert!((d - 10.0).abs() < 1e-3, "got {d}");
    }
}

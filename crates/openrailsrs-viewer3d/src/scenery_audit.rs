//! Scenery pipeline diagnostics (`OPENRAILSRS_SCENERY_AUDIT=1` or `OPENRAILSRS_SCENERY_DEBUG=1`).
//!
//! Samples shapes near the route focus and reports *why* objects may render wrong
//! (missing `.s`/`.ace`, dark atlases, bad normals, lit PBR on MSTS albedo, …).

use std::path::{Path, PathBuf};

use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use openrailsrs_ace::read_ace;

use crate::shapes::{
    RouteAssets, collect_loaded_shape_texture_paths, load_shape_from_path,
    resolve_texture_path_in_dirs, texture_search_dirs_for_shape,
};
use crate::viewer_log;
use crate::world::{RouteFocus, WorldScene};

pub use crate::shapes::{DARK_TEXTURE_LUMA_THRESHOLD, ace_mean_luma};

/// Share of near-zero normals above this → mesh will look unlit/black.
pub const DEGENERATE_NORMALS_THRESHOLD: f32 = 0.25;

pub fn scenery_audit_enabled() -> bool {
    std::env::var_os("OPENRAILSRS_SCENERY_AUDIT").is_some()
        || std::env::var_os("OPENRAILSRS_SCENERY_DEBUG").is_some()
}

#[derive(Clone, Debug, Default)]
pub struct MeshGeometryStats {
    pub vertices: usize,
    pub has_normals: bool,
    pub has_uvs: bool,
    pub zero_normal_fraction: f32,
}

#[derive(Clone, Debug, Default)]
pub struct TextureStats {
    pub file: String,
    pub resolved: bool,
    pub decode_ok: bool,
    pub mean_luma: Option<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct ShapeAuditSample {
    pub shape_path: PathBuf,
    pub parse_ok: bool,
    pub mesh: MeshGeometryStats,
    pub textures: Vec<TextureStats>,
    pub issues: Vec<SceneryIssue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneryIssue {
    ShapeParseFailed,
    ShapeFileMissing,
    TextureMissing,
    TextureDecodeFailed,
    DarkTexture,
    NoNormals,
    DegenerateNormals,
    NoUvs,
}

impl SceneryIssue {
    pub fn hint(self) -> &'static str {
        match self {
            Self::ShapeParseFailed => "fix shape parser or binary LOD for this .s",
            Self::ShapeFileMissing => "add .s under route/GLOBAL SHAPES or fix resolve_world_shape",
            Self::TextureMissing => "copy .ace to route/TEXTURES or GLOBAL/TEXTURES",
            Self::TextureDecodeFailed => "check ACE decode / path case",
            Self::DarkTexture => {
                "pixel-normalize dark .ace mip0 + unlit material (MSTS atlases are baked dark)"
            }
            Self::NoNormals => "recompute vertex normals in shape → mesh builder",
            Self::DegenerateNormals => "flip Z on normals or recompute after transform",
            Self::NoUvs => "UVs missing — texture will not map",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ShapeParseFailed => "PARSE_FAIL",
            Self::ShapeFileMissing => "SHAPE_MISSING",
            Self::TextureMissing => "TEXTURE_MISSING",
            Self::TextureDecodeFailed => "ACE_FAIL",
            Self::DarkTexture => "DARK_TEXTURE",
            Self::NoNormals => "NO_NORMALS",
            Self::DegenerateNormals => "BAD_NORMALS",
            Self::NoUvs => "NO_UVS",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ShapeAuditSummary {
    pub shapes_audited: usize,
    pub issue_counts: [usize; 8],
    pub worst: Vec<ShapeAuditSample>,
}

impl ShapeAuditSummary {
    fn bump(&mut self, issue: SceneryIssue) {
        let idx = match issue {
            SceneryIssue::ShapeParseFailed => 0,
            SceneryIssue::ShapeFileMissing => 1,
            SceneryIssue::TextureMissing => 2,
            SceneryIssue::TextureDecodeFailed => 3,
            SceneryIssue::DarkTexture => 4,
            SceneryIssue::NoNormals => 5,
            SceneryIssue::DegenerateNormals => 6,
            SceneryIssue::NoUvs => 7,
        };
        self.issue_counts[idx] += 1;
    }

    pub fn merge_sample(&mut self, sample: ShapeAuditSample) {
        self.shapes_audited += 1;
        for issue in &sample.issues {
            self.bump(*issue);
        }
        if !sample.issues.is_empty() {
            if self.worst.len() < 12 {
                self.worst.push(sample);
            } else {
                let replace = self
                    .worst
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, s)| s.issues.len());
                if let Some((idx, existing)) = replace {
                    if sample.issues.len() > existing.issues.len() {
                        self.worst[idx] = sample;
                    }
                }
            }
        }
    }

    pub fn log_report(&self, context: &str) {
        if self.shapes_audited == 0 {
            viewer_log!("openrailsrs-viewer3d: scenery-audit ({context}): no shapes sampled");
            return;
        }
        viewer_log!(
            "openrailsrs-viewer3d: scenery-audit ({context}): {} shape(s) — \
             missing_tex={} dark_tex={} bad_normals={} no_uvs={} parse_fail={}",
            self.shapes_audited,
            self.issue_counts[2],
            self.issue_counts[4],
            self.issue_counts[5] + self.issue_counts[6],
            self.issue_counts[7],
            self.issue_counts[0] + self.issue_counts[1],
        );
        for sample in &self.worst {
            let name = sample
                .shape_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| sample.shape_path.display().to_string());
            let codes: Vec<_> = sample.issues.iter().map(|i| i.label()).collect();
            let luma = sample
                .textures
                .iter()
                .filter_map(|t| t.mean_luma)
                .reduce(f32::min)
                .map(|l| format!(" luma_min={l:.0}"))
                .unwrap_or_default();
            let norms = if sample.mesh.vertices > 0 {
                format!(
                    " normals_zero={:.0}%",
                    sample.mesh.zero_normal_fraction * 100.0
                )
            } else {
                String::new()
            };
            viewer_log!("openrailsrs-viewer3d: scenery-audit   {name}: {codes:?}{luma}{norms}");
            if let Some(issue) = sample.issues.first() {
                viewer_log!("openrailsrs-viewer3d: scenery-audit     → {}", issue.hint());
            }
        }
    }
}

pub fn mesh_geometry_stats(mesh: &Mesh) -> MeshGeometryStats {
    let vertices = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .map(|attr| match attr {
            VertexAttributeValues::Float32x3(v) => v.len(),
            _ => 0,
        })
        .unwrap_or(0);
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        return MeshGeometryStats {
            vertices,
            has_normals: false,
            has_uvs: mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some(),
            zero_normal_fraction: 1.0,
        };
    };
    let zero = normals
        .iter()
        .filter(|n| {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            len < 1e-4
        })
        .count();
    MeshGeometryStats {
        vertices,
        has_normals: true,
        has_uvs: mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some(),
        zero_normal_fraction: if normals.is_empty() {
            1.0
        } else {
            zero as f32 / normals.len() as f32
        },
    }
}

pub fn audit_mesh_issues(stats: &MeshGeometryStats) -> Vec<SceneryIssue> {
    let mut out = Vec::new();
    if stats.vertices == 0 {
        return out;
    }
    if !stats.has_normals {
        out.push(SceneryIssue::NoNormals);
    } else if stats.zero_normal_fraction >= DEGENERATE_NORMALS_THRESHOLD {
        out.push(SceneryIssue::DegenerateNormals);
    }
    if !stats.has_uvs {
        out.push(SceneryIssue::NoUvs);
    }
    out
}

pub fn audit_texture_file(texture_dirs: &[&Path], file: &str) -> TextureStats {
    let mut stats = TextureStats {
        file: file.to_string(),
        ..Default::default()
    };
    let Some(path) = resolve_texture_path_in_dirs(texture_dirs, file) else {
        return stats;
    };
    stats.resolved = true;
    match read_ace(&path) {
        Ok(ace) => {
            stats.decode_ok = true;
            stats.mean_luma = Some(ace_mean_luma(&ace.mip0));
        }
        Err(_) => stats.decode_ok = false,
    }
    stats
}

pub fn texture_issues(stats: &TextureStats) -> Vec<SceneryIssue> {
    let mut out = Vec::new();
    if !stats.resolved {
        out.push(SceneryIssue::TextureMissing);
        return out;
    }
    if !stats.decode_ok {
        out.push(SceneryIssue::TextureDecodeFailed);
        return out;
    }
    if stats
        .mean_luma
        .is_some_and(|l| l < DARK_TEXTURE_LUMA_THRESHOLD)
    {
        out.push(SceneryIssue::DarkTexture);
    }
    out
}

/// Audit one resolved `.s` on disk (parse, mesh stats, textures).
pub fn audit_shape_path(shape_path: &Path, route_dir: &Path) -> ShapeAuditSample {
    let mut sample = ShapeAuditSample {
        shape_path: shape_path.to_path_buf(),
        ..Default::default()
    };
    if !shape_path.is_file() {
        sample.issues.push(SceneryIssue::ShapeFileMissing);
        return sample;
    }
    let tex_dirs: Vec<PathBuf> = texture_search_dirs_for_shape(shape_path, route_dir);
    let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();

    let Some(loaded) = load_shape_from_path(shape_path, None) else {
        sample.issues.push(SceneryIssue::ShapeParseFailed);
        return sample;
    };
    sample.parse_ok = true;
    sample.mesh = mesh_geometry_stats(&loaded.mesh);
    sample.issues.extend(audit_mesh_issues(&sample.mesh));

    let texture_paths = collect_loaded_shape_texture_paths(&loaded, &tex_refs);
    let mut seen = std::collections::HashSet::new();
    for path in texture_paths {
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !seen.insert(file.clone()) {
            continue;
        }
        let tex = audit_texture_file(&tex_refs, &file);
        sample.issues.extend(texture_issues(&tex));
        sample.textures.push(tex);
    }
    sample
}

/// Audit unique `.s` files referenced by world objects near `focus`.
pub fn audit_world_shapes_near(
    world: &WorldScene,
    focus: &RouteFocus,
    assets: &RouteAssets,
    radius_m: f32,
    max_shapes: usize,
) -> ShapeAuditSummary {
    let mut summary = ShapeAuditSummary::default();
    let mut seen = std::collections::HashSet::new();
    let mut objects: Vec<_> = world
        .items
        .iter()
        .filter(|obj| {
            focus.horizontal_distance(obj.position) <= radius_m
                && (obj
                    .shape_file
                    .as_ref()
                    .is_some_and(|f| f.to_ascii_lowercase().ends_with(".s"))
                    || (obj.kind == "TrackObj" && obj.section_idx.is_some()))
        })
        .collect();
    objects.sort_by(|a, b| {
        focus
            .horizontal_distance(a.position)
            .total_cmp(&focus.horizontal_distance(b.position))
    });

    for obj in objects {
        if summary.shapes_audited >= max_shapes {
            break;
        }
        let Some(file) = obj.shape_file.as_deref() else {
            if obj.kind == "TrackObj" {
                let Some(path) = assets.resolve_trackobj_shape(None, obj.section_idx) else {
                    continue;
                };
                if !seen.insert(path.clone()) {
                    continue;
                }
                summary.merge_sample(audit_shape_path(&path, &assets.route_dir));
            }
            continue;
        };
        let resolve_path = if obj.kind == "TrackObj" {
            assets.resolve_trackobj_shape(Some(file), obj.section_idx)
        } else {
            assets.resolve_world_shape(obj.kind, file)
        };
        let Some(path) = resolve_path else {
            let mut sample = ShapeAuditSample {
                shape_path: PathBuf::from(file),
                ..Default::default()
            };
            sample.issues.push(SceneryIssue::ShapeFileMissing);
            summary.merge_sample(sample);
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        summary.merge_sample(audit_shape_path(&path, &assets.route_dir));
    }
    summary
}

/// Audit parsed shapes from progressive spawn (unique paths only).
pub fn audit_parsed_shapes(
    parsed: &[(PathBuf, Option<crate::shapes::LoadedShape>)],
    route_dir: &Path,
) -> ShapeAuditSummary {
    let mut summary = ShapeAuditSummary::default();
    for (path, loaded) in parsed {
        if let Some(loaded) = loaded {
            let mut sample = ShapeAuditSample {
                shape_path: path.clone(),
                parse_ok: true,
                mesh: mesh_geometry_stats(&loaded.mesh),
                ..Default::default()
            };
            sample.issues.extend(audit_mesh_issues(&sample.mesh));
            let tex_dirs: Vec<PathBuf> = texture_search_dirs_for_shape(path, route_dir);
            let tex_refs: Vec<&Path> = tex_dirs.iter().map(|p| p.as_path()).collect();
            for tex_path in collect_loaded_shape_texture_paths(loaded, &tex_refs) {
                let file = tex_path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let tex = audit_texture_file(&tex_refs, &file);
                sample.issues.extend(texture_issues(&tex));
                sample.textures.push(tex);
            }
            summary.merge_sample(sample);
        } else {
            summary.merge_sample(audit_shape_path(path, route_dir));
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ace_mean_luma_mid_grey() {
        let rgba = vec![128u8, 128, 128, 255];
        assert!((ace_mean_luma(&rgba) - 128.0).abs() < 1.0);
    }

    #[test]
    fn audit_minimal_shape_fixture() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../openrailsrs-formats/tests/fixtures/minimal.s");
        if !path.is_file() {
            return;
        }
        let route = path.parent().unwrap().parent().unwrap();
        let sample = audit_shape_path(&path, route);
        assert!(sample.parse_ok, "minimal.s should parse");
    }

    #[test]
    fn chiltern_near_shapes_audit_runs() {
        let route = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
        if !route.join("WORLD").is_dir() {
            return;
        }
        let world = crate::world::load_world_from_route_dir_near(
            &route,
            Some(crate::world::world_tile_center_hint(&route).unwrap()),
            2000.0,
        );
        let focus = crate::world::RouteFocus {
            center: crate::world::world_tile_center_hint(&route).unwrap(),
            height_origin: 20.0,
        };
        let assets = RouteAssets::new(&route);
        let summary = audit_world_shapes_near(&world, &focus, &assets, 2000.0, 8);
        assert!(summary.shapes_audited > 0);
    }
}

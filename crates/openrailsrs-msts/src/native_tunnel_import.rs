//! Signals.ProcessTunnels: consecutive TSECTION tunnel shapes within each TDB vector.
use crate::error::MstsError;
use openrailsrs_formats::{RouteFile, TSectionCatalog, TrackDbFile, TrackNodeKind};
use openrailsrs_track::tunnel::{NativeTunnel, effective_geometry};
use std::path::Path;

pub(super) fn bind(
    route: &Path,
    tdb: &TrackDbFile,
    catalog: &TSectionCatalog,
    source: &str,
) -> Result<String, MstsError> {
    let has_tunnel = tdb.nodes.iter().any(|node| match &node.kind {
        TrackNodeKind::Vector { sections, .. } => sections.iter().any(|section| {
            catalog
                .shapes
                .get(&section.shape_index)
                .is_some_and(|s| s.tunnel_shape == Some(true))
        }),
        _ => false,
    });
    if !has_tunnel {
        return Ok(source.into());
    }
    let route = RouteFile::from_route_dir(route)?;
    let mut doc: toml::Value = toml::from_str(source)?;
    let profiles = doc
        .as_table_mut()
        .ok_or_else(|| MstsError::msg("Invalid track document"))?
        .entry("edge_profiles")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| MstsError::msg("Invalid edge profiles"))?;
    for node in &tdb.nodes {
        let TrackNodeKind::Vector {
            sections, length_m, ..
        } = &node.kind
        else {
            continue;
        };
        let mut groups: Vec<(f32, f32, u32)> = Vec::new();
        let mut active: Option<(f32, f32, u32)> = None;
        let mut position = 0_f32;
        for section in sections {
            let Some(def) = catalog.sections.get(&section.section_index) else {
                continue;
            };
            let length =
                if let (Some(radius), Some(angle)) = (def.curve_radius_m, def.curve_angle_deg) {
                    ((angle as f32).abs() * 0.017_453_292_f32) * (radius as f32)
                } else {
                    def.length_m as f32
                };
            if !length.is_finite() || length < 0. {
                return Err(MstsError::msg("Invalid native tunnel section length"));
            }
            let shape = catalog.shapes.get(&section.shape_index);
            if shape.is_some_and(|s| s.tunnel_shape == Some(true)) {
                let paths = shape.and_then(|s| s.num_paths).unwrap_or(0);
                if let Some(group) = &mut active {
                    group.1 += length;
                    group.2 = group.2.min(paths);
                } else {
                    active = Some((position, length, paths));
                }
            } else if let Some(group) = active.take() {
                groups.push(group);
            }
            position += length;
        }
        if let Some(group) = active {
            groups.push(group);
        }
        let mut forward = Vec::new();
        for (start, length, paths) in groups {
            if length <= 0. {
                continue;
            }
            let (area, perimeter) = if paths >= 2 {
                (
                    route.tunnels.double_area_m2,
                    route.tunnels.double_perimeter_m,
                )
            } else {
                (
                    route.tunnels.single_area_m2,
                    route.tunnels.single_perimeter_m,
                )
            };
            let (area_m2, perimeter_m) = effective_geometry(
                route.speed_limit_mps.unwrap_or(500.),
                paths,
                area,
                perimeter,
            );
            let tunnel = NativeTunnel {
                start_m: f64::from(start).min(*length_m),
                end_m: f64::from(start + length).min(*length_m),
                length_m: f64::from(length),
                num_paths: paths,
                area_m2,
                perimeter_m,
            };
            if !tunnel.valid() {
                return Err(MstsError::msg("Invalid native tunnel metadata"));
            }
            forward.push(tunnel);
        }
        if forward.is_empty() {
            continue;
        }
        let mut reverse: Vec<_> = forward.iter().map(|t| t.mirrored(*length_m)).collect();
        reverse.reverse();
        for (id, groups) in [
            (format!("e{}", node.id), forward),
            (format!("e{}_r", node.id), reverse),
        ] {
            let profile = profiles
                .entry(id)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .ok_or_else(|| MstsError::msg("Invalid tunnel edge profile"))?;
            profile.insert("native_tunnels".into(), toml::Value::try_from(groups)?);
        }
    }
    Ok(toml::to_string_pretty(&doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::typed::{TrackSectionDef, TrackShapeDef};
    #[test]
    fn consecutive_original_shapes_keep_group_length_minimum_paths_and_reverse_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("fixture.trk"),
            "Tr_RouteFile ( RouteID ( fixture ) SpeedLimit (223.52) )",
        )
        .unwrap();
        let mut catalog = TSectionCatalog::default();
        for (id, length) in [(1, 25.), (2, 50.), (3, 75.)] {
            catalog.sections.insert(
                id,
                TrackSectionDef {
                    gauge_m: 1.435,
                    length_m: length,
                    curve_radius_m: None,
                    curve_angle_deg: None,
                    skew_deg: None,
                },
            );
        }
        for (id, tunnel, paths) in [(0, false, 1), (10, true, 2), (11, true, 1)] {
            catalog.shapes.insert(
                id,
                TrackShapeDef {
                    file_name: "fixture.s".into(),
                    main_route: None,
                    clearance_dist_m: None,
                    road_shape: false,
                    tunnel_shape: Some(tunnel),
                    num_paths: Some(paths),
                    paths: vec![],
                },
            );
        }
        let records = [(1, 0), (2, 10), (3, 11), (1, 0), (1, 10)]
            .into_iter()
            .scan(0., |start, (section, shape)| {
                // Native records require plausible world and point tiles;
                // all-zero tile fields are intentionally rejected by TDB.
                let record = format!(
                    "{section} {shape} -6110 14951 0 1 0 00 -6110 14951 0 88 {start} 0 0 0"
                );
                *start += catalog.sections[&section].length_m;
                Some(record)
            })
            .collect::<Vec<_>>()
            .join(" ");
        let text = format!(
            "TrackDB ( TrackNodes (1 TrackNode(1 TrVectorNode(TrVectorSections(5 {records})) TrPins(1 1 TrPin(2 1) TrPin(3 0)))) TrItemTable(0))"
        );
        let mut tdb =
            TrackDbFile::from_ast(&openrailsrs_formats::parse_named_stf(&text).unwrap()).unwrap();
        let TrackNodeKind::Vector {
            sections, length_m, ..
        } = &mut tdb.nodes[0].kind
        else {
            panic!("Native vector expected");
        };
        assert_eq!(
            sections
                .iter()
                .map(|section| (section.section_index, section.shape_index))
                .collect::<Vec<_>>(),
            [(1, 0), (2, 10), (3, 11), (1, 0), (1, 10)]
        );
        *length_m = 200.;
        let result = bind(
            dir.path(),
            &tdb,
            &catalog,
            "[edge_profiles.e1]\n[edge_profiles.e1_r]\n",
        )
        .unwrap();
        let doc: toml::Value = toml::from_str(&result).unwrap();
        let forward: Vec<NativeTunnel> = doc["edge_profiles"]["e1"]["native_tunnels"]
            .clone()
            .try_into()
            .unwrap();
        let reverse: Vec<NativeTunnel> = doc["edge_profiles"]["e1_r"]["native_tunnels"]
            .clone()
            .try_into()
            .unwrap();
        assert_eq!(forward.len(), 2);
        assert_eq!(
            (
                forward[0].start_m,
                forward[0].end_m,
                forward[0].length_m,
                forward[0].num_paths
            ),
            (25., 150., 125., 1)
        );
        assert_eq!((forward[0].area_m2, forward[0].perimeter_m), (70., 32.));
        assert_eq!(
            (
                forward[1].start_m,
                forward[1].length_m,
                forward[1].num_paths
            ),
            (175., 25., 2)
        );
        assert_eq!(
            (reverse[0].start_m, reverse[0].end_m, reverse[0].length_m),
            (0., 25., 25.)
        );
        assert_eq!(
            (reverse[1].start_m, reverse[1].end_m, reverse[1].length_m),
            (50., 175., 125.)
        );
        assert_eq!(reverse[1].num_paths, 1);
    }
}

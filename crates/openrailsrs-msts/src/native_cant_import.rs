//! Bridge OR's existing cant compiler to headless route physics. Visual roll
//! remains authored AZ; this channel stores the separate PhysElevTable values.
use std::collections::HashMap;
use std::path::Path;

use openrailsrs_formats::typed::cant_speeds::{CantSpeedLimit, limits_for_node, speeds_at};
use openrailsrs_formats::typed::{
    CantProfile, CantSection, RouteCantSettings, generate_cant_profiles_with_table,
};
use openrailsrs_formats::{RouteFile, TSectionCatalog, TrItemKind, TrackDbFile, TrackNodeKind};
use openrailsrs_track::PositionPhysicalCant;

use super::{native_speed_post_is_reverse, section_lengths};
use crate::error::MstsError;

fn signs(tdb: &TrackDbFile, catalog: &TSectionCatalog) -> HashMap<u32, Vec<CantSpeedLimit>> {
    tdb.nodes
        .iter()
        .filter_map(|node| {
            let TrackNodeKind::Vector {
                length_m,
                sections,
                item_ids,
                ..
            } = &node.kind
            else {
                return None;
            };
            let lengths = section_lengths(*length_m, sections, Some(catalog));
            let mut local = vec![];
            for id in item_ids {
                let Some(item) = tdb.item_by_id(*id) else {
                    continue;
                };
                let TrItemKind::SpeedPost { speed_mph } = item.kind else {
                    continue;
                };
                let flags = item.speed_post_flags;
                if flags.is_some_and(|f| f & 2 == 0) {
                    continue;
                }
                let resume = flags.is_some_and(|f| f & 3 == 3);
                if !resume && (speed_mph <= 0. || !speed_mph.is_finite()) {
                    continue;
                }
                let direction = item.speed_post_angle_rad.map_or(0, |angle| {
                    let mut before = 0.;
                    for (section, span) in sections.iter().zip(&lengths) {
                        if item.distance_m <= before + span + 1e-6 {
                            let yaw = catalog
                                .sections
                                .get(&section.section_index)
                                .and_then(|d| d.curve_angle_deg)
                                .map_or(0., |a| {
                                    a.to_radians()
                                        * ((item.distance_m - before) / span).clamp(0., 1.)
                                });
                            return if native_speed_post_is_reverse(section.ay + yaw, angle) {
                                -1
                            } else {
                                1
                            };
                        }
                        before += span;
                    }
                    0
                });
                let speed = if resume {
                    f32::MAX
                } else {
                    (speed_mph * 0.44704) as f32
                };
                local.push(CantSpeedLimit {
                    position_m: item.distance_m,
                    direction,
                    passenger_mps: flags
                        .is_none_or(|f| f & ((1 << 5) | (1 << 7)) != 0)
                        .then_some(speed),
                    freight_mps: flags
                        .is_none_or(|f| f & ((1 << 6) | (1 << 7)) != 0)
                        .then_some(speed),
                });
            }
            local.sort_by(|a, b| a.position_m.total_cmp(&b.position_m));
            Some((node.id, local))
        })
        .collect()
}

/// Grouping follows SuperElevation's physical constructor. Visual eligibility
/// (turnout shapes/authored AZ) is handled separately by the renderer.
fn physical_profiles(
    inputs: &[CantSection],
    directions: &[f32],
    settings: &RouteCantSettings,
) -> Vec<CantProfile> {
    let mut profiles = vec![CantProfile::default(); inputs.len()];
    let total: f32 = inputs.iter().map(|s| s.length_m).sum();
    let mut before = 0.;
    let mut active = false;
    let mut direction = 0.;
    let mut group = vec![];
    let finish = |group: &mut Vec<usize>, direction: f32, profiles: &mut Vec<CantProfile>| {
        let selected: Vec<_> = group.iter().map(|i| inputs[*i]).collect();
        for (index, profile) in group.drain(..).zip(generate_cant_profiles_with_table(
            &selected,
            settings.gauge_m,
            &settings.standards,
            direction,
            &settings.legacy_radius_cant,
        )) {
            profiles[index] = profile;
        }
    };
    for (i, input) in inputs.iter().enumerate() {
        let current = directions[i];
        let next = directions.get(i + 1).copied().unwrap_or(0.);
        if !(before + input.length_m < 20. || total - before < 20.) {
            if current != 0. {
                if !active {
                    active = true;
                    direction = current;
                } else if direction != current {
                    finish(&mut group, direction, &mut profiles);
                    direction = current;
                }
                group.push(i);
            } else if active {
                if i + 1 < inputs.len() && !(next != 0. && direction != next) {
                    group.push(i);
                }
                if !(next != 0. && direction == next) {
                    finish(&mut group, direction, &mut profiles);
                    active = false;
                }
            } else if i + 1 < inputs.len() && next != 0. {
                active = true;
                direction = next;
                group.push(i);
            }
        }
        before += input.length_m;
    }
    if active {
        finish(&mut group, direction, &mut profiles);
    }
    profiles
}

pub(super) fn bind(
    route_dir: &Path,
    tdb: &TrackDbFile,
    catalog: &TSectionCatalog,
    source: &str,
) -> Result<String, MstsError> {
    let Ok(route) = RouteFile::from_route_dir(route_dir) else {
        return Ok(source.into());
    };
    let settings = &route.cant;
    if settings.legacy_table_present && settings.legacy_radius_cant.is_empty() {
        return Ok(source.into());
    }
    let signs = signs(tdb, catalog);
    let branches: HashMap<_, _> = tdb
        .nodes
        .iter()
        .filter_map(|node| {
            let shape = catalog.shapes.get(&node.junction_shape_index?)?;
            Some((node.id, shape.main_route.unwrap_or(0) as usize))
        })
        .collect();
    let mut document: toml::Value = toml::from_str(source)?;
    let Some(edges) = document
        .get_mut("edge_profiles")
        .and_then(toml::Value::as_table_mut)
    else {
        return Ok(source.into());
    };
    for node in &tdb.nodes {
        let TrackNodeKind::Vector {
            length_m, sections, ..
        } = &node.kind
        else {
            continue;
        };
        if sections.is_empty() {
            continue;
        }
        let imported_lengths = section_lengths(*length_m, sections, Some(catalog));
        let definitions = sections
            .iter()
            .map(|s| catalog.sections.get(&s.section_index))
            .collect::<Option<Vec<_>>>();
        let Some(definitions) = definitions else {
            continue;
        };
        let limits = limits_for_node(tdb, &signs, &branches, node.id, *length_m);
        let mut before = 0.;
        let inputs: Vec<_> = definitions
            .iter()
            .zip(&imported_lengths)
            .map(|(definition, span)| {
                let speed = speeds_at(&limits, before + span / 2., settings.design_speed_mps);
                before += span;
                let native_length = match definition.curve_radius_m.zip(definition.curve_angle_deg)
                {
                    Some((radius, angle)) => radius as f32 * (angle as f32).to_radians().abs(),
                    None => definition.effective_length_m() as f32,
                };
                CantSection {
                    length_m: native_length,
                    radius_m: definition.curve_radius_m.unwrap_or(0.) as f32,
                    passenger_speed_mps: speed[0],
                    freight_speed_mps: speed[1],
                }
            })
            .collect();
        let directions: Vec<_> = definitions
            .iter()
            .map(|d| d.curve_angle_deg.map_or(0., |a| a.signum() as f32))
            .collect();
        let profiles = physical_profiles(&inputs, &directions, settings);
        let mut points = vec![];
        let mut before = 0.;
        for (profile, span) in profiles.iter().zip(&imported_lengths) {
            if profile.positions.is_empty() {
                points.push(PositionPhysicalCant {
                    position_m: before,
                    cant_m: 0.,
                });
                points.push(PositionPhysicalCant {
                    position_m: (before + span).min(*length_m),
                    cant_m: 0.,
                });
            } else {
                for (fraction, cant) in profile.positions.iter().zip(&profile.elevations_m) {
                    points.push(PositionPhysicalCant {
                        position_m: (before + span * f64::from(*fraction)).clamp(0., *length_m),
                        cant_m: f64::from(*cant),
                    });
                }
            }
            before += span;
        }
        let reverse: Vec<_> = points
            .iter()
            .rev()
            .map(|p| PositionPhysicalCant {
                position_m: (*length_m - p.position_m).clamp(0., *length_m),
                cant_m: p.cant_m,
            })
            .collect();
        for (id, points) in [
            (format!("e{}", node.id), points),
            (format!("e{}_r", node.id), reverse),
        ] {
            if let Some(profile) = edges.get_mut(&id).and_then(toml::Value::as_table_mut) {
                profile.insert("physical_cant".into(), toml::Value::try_from(points)?);
            }
        }
    }
    Ok(toml::to_string_pretty(&document)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrailsrs_formats::typed::TrackSectionDef;
    use openrailsrs_formats::{TrVectorSectionRecord, TrackDbNode, TrackVectorPoint};

    #[test]
    fn physical_cant_bridge_keeps_native_tables_and_reverse_magnitudes_separate_from_roll() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("cant.trk"),
            "Tr_RouteFile ( RouteID ( cant ) SpeedLimit ( 40 ) )",
        )
        .unwrap();
        let definition = TrackSectionDef {
            gauge_m: 1.435,
            length_m: 0.,
            curve_radius_m: Some(500.),
            curve_angle_deg: Some(20.),
            skew_deg: None,
        };
        let length = definition.effective_length_m();
        let tdb = TrackDbFile {
            nodes: vec![TrackDbNode {
                id: 1,
                junction_shape_index: None,
                position: None,
                pin_refs: vec![],
                kind: TrackNodeKind::Vector {
                    pins: (0, 0),
                    length_m: length,
                    speed_limit_mps: 40.,
                    item_ids: vec![],
                    geometry: None,
                    sections: vec![TrVectorSectionRecord {
                        section_index: 1,
                        shape_index: 0,
                        header_tile_x: 0,
                        header_tile_z: 0,
                        start: TrackVectorPoint {
                            tile_x: 0,
                            tile_z: 0,
                            x: 0.,
                            y: 0.,
                            z: 0.,
                        },
                        ax: 0.,
                        ay: 0.,
                        az: 0.123,
                    }],
                },
            }],
            items: vec![],
        };
        let catalog = TSectionCatalog {
            sections: HashMap::from([(1, definition)]),
            shapes: HashMap::new(),
        };
        let source = "[edge_profiles.e1]\ncurves=[{position_m=0.0, curvature_m_inv=0.002, cant_rad=0.123, heading_rad=0.0}]\n[edge_profiles.e1_r]\ncurves=[{position_m=0.0, curvature_m_inv=-0.002, cant_rad=-0.123, heading_rad=0.0}]\n";
        let output = bind(directory.path(), &tdb, &catalog, source).unwrap();
        let document: toml::Value = toml::from_str(&output).unwrap();
        let forward: openrailsrs_track::EdgePhysicsProfile =
            document["edge_profiles"]["e1"].clone().try_into().unwrap();
        let reverse: openrailsrs_track::EdgePhysicsProfile = document["edge_profiles"]["e1_r"]
            .clone()
            .try_into()
            .unwrap();
        let route = RouteFile::from_route_dir(directory.path()).unwrap();
        let generated = generate_cant_profiles_with_table(
            &[CantSection {
                length_m: 500f32 * 20f32.to_radians(),
                radius_m: 500.,
                passenger_speed_mps: 40.,
                freight_speed_mps: 40.,
            }],
            route.cant.gauge_m,
            &route.cant.standards,
            1.,
            &route.cant.legacy_radius_cant,
        );
        assert_eq!(forward.physical_cant.len(), generated[0].positions.len());
        assert!(forward.physical_cant.iter().any(|p| p.cant_m > 0.));
        for ((a, b), expected) in forward
            .physical_cant
            .iter()
            .zip(reverse.physical_cant.iter().rev())
            .zip(&generated[0].elevations_m)
        {
            assert_eq!(a.cant_m, f64::from(*expected));
            assert_eq!(a.cant_m, b.cant_m);
            assert!((a.position_m + b.position_m - length).abs() < 1e-10);
        }
        assert_eq!(forward.curves[0].cant_rad, 0.123);
        assert_eq!(reverse.curves[0].cant_rad, -0.123);
    }
}

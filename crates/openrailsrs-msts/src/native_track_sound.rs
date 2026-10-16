//! Bind original WORLD/*.ws rolling sound regions to both directed TDB edges.
use crate::error::MstsError;
use openrailsrs_formats::typed::track_sound::WorldSoundRegion;
use openrailsrs_formats::{TrItemKind, TrackDbFile, TrackNodeKind, resolve_path_case_insensitive};
use openrailsrs_track::track_sound::TrackSoundBoundary;
use std::{collections::HashMap, path::Path};

pub(super) fn bind(route: &Path, tdb: &TrackDbFile, source: &str) -> Result<String, MstsError> {
    let Some(world) = resolve_path_case_insensitive(&route.join("WORLD")) else {
        return Ok(source.into());
    };
    let mut files: Vec<_> = std::fs::read_dir(world)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ws")))
        .collect();
    files.sort();
    let hosts: HashMap<_, _> = tdb
        .nodes
        .iter()
        .filter_map(|node| match &node.kind {
            TrackNodeKind::Vector {
                item_ids, length_m, ..
            } => Some(item_ids.iter().map(move |id| (*id, (node.id, *length_m)))),
            _ => None,
        })
        .flatten()
        .collect();
    let mut profiles: HashMap<String, Vec<TrackSoundBoundary>> = HashMap::new();
    let doc: toml::Value = toml::from_str(source)?;
    for path in files {
        openrailsrs_formats::cancellation::check()?;
        for region in WorldSoundRegion::from_path(&path)
            .map_err(|e| MstsError::msg(format!("{}: {e}", path.display())))?
        {
            for id in region.track_item_ids {
                let Some(item) = tdb
                    .item_by_id(id)
                    .filter(|item| matches!(item.kind, TrItemKind::SoundRegion))
                else {
                    continue;
                };
                let Some(&(host, length)) = hosts.get(&id) else {
                    continue;
                };
                let position = item.distance_m;
                if !position.is_finite() || position < 0. || position > length {
                    continue;
                }
                let edge = format!("e{host}");
                let heading = doc
                    .get("edge_profiles")
                    .and_then(|p| p.get(&edge))
                    .and_then(|p| p.get("curves"))
                    .and_then(toml::Value::as_array)
                    .and_then(|samples| {
                        samples.iter().rev().find(|p| {
                            p.get("position_m")
                                .and_then(toml::Value::as_float)
                                .is_some_and(|s| s <= position)
                        })
                    })
                    .and_then(|p| p.get("heading_rad"))
                    .and_then(toml::Value::as_float)
                    .unwrap_or(0.);
                let faces_forward = (heading - region.rotation_y_rad).cos() >= 0.;
                for (edge, position_m, faces_forward) in [
                    (edge, position, faces_forward),
                    (format!("e{host}_r"), length - position, !faces_forward),
                ] {
                    profiles.entry(edge).or_default().push(TrackSoundBoundary {
                        position_m,
                        track_type: region.track_type,
                        faces_forward,
                        track_item_id: id,
                    });
                }
            }
        }
    }
    if profiles.is_empty() {
        return Ok(source.into());
    }
    let mut doc = doc;
    let table = doc
        .as_table_mut()
        .ok_or_else(|| MstsError::msg("track root is not a table"))?
        .entry("edge_profiles")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| MstsError::msg("invalid edge_profiles"))?;
    for (edge, mut boundaries) in profiles {
        boundaries.sort_by(|a, b| {
            a.position_m
                .total_cmp(&b.position_m)
                .then(a.track_item_id.cmp(&b.track_item_id))
        });
        let profile = table
            .entry(edge)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| MstsError::msg("invalid track sound profile"))?;
        profile.insert(
            "track_sound_regions".into(),
            toml::Value::try_from(boundaries)?,
        );
    }
    Ok(toml::to_string_pretty(&doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_ws_boundaries_mirror_without_becoming_ambient_emitters() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("WORLD")).unwrap();
        std::fs::write(temp.path().join("WORLD/w.ws"),"Tr_Worldsoundfile ( SoundRegion ( SoundRegionTrackType ( 2 ) SoundRegionRoty ( 0 ) TrItemId ( 0 0 ) TrItemId ( 0 1 ) ) )").unwrap();
        let tdb=TrackDbFile::from_ast(&openrailsrs_formats::parse_named_stf("TrackDB ( TrackNodes ( 1 TrackNode ( 1 TrVectorNode ( TrVectorSections ( 1 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 ) TrItemRefs ( 2 TrItemRef ( 0 ) TrItemRef ( 1 ) ) ) TrPins ( 1 1 TrPin ( 2 1 ) TrPin ( 3 0 ) ) ) ) TrItemTable ( 2 SoundRegionItem ( TrItemId ( 0 ) TrItemSData ( 12 00000000 ) ) SoundRegionItem ( TrItemId ( 1 ) TrItemSData ( 32 00000000 ) ) ) )").unwrap()).unwrap();
        // Supply an explicit vector length: the deliberately minimal fixture has no section catalog.
        let mut tdb = tdb;
        if let TrackNodeKind::Vector { length_m, .. } = &mut tdb.nodes[0].kind {
            *length_m = 50.;
        }
        let out=bind(temp.path(),&tdb,"[edge_profiles.e1]\ncurves=[{position_m=0.0,heading_rad=0.0,curvature_m_inv=0.0,cant_rad=0.0}]\n").unwrap();
        let value: toml::Value = toml::from_str(&out).unwrap();
        let forward: openrailsrs_track::EdgePhysicsProfile =
            value["edge_profiles"]["e1"].clone().try_into().unwrap();
        let reverse: openrailsrs_track::EdgePhysicsProfile =
            value["edge_profiles"]["e1_r"].clone().try_into().unwrap();
        assert_eq!(forward.track_sound_regions.len(), 2);
        assert_eq!(reverse.track_sound_regions[0].position_m, 18.);
        assert!(!reverse.track_sound_regions[0].faces_forward);
    }
}

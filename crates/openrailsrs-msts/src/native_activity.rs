//! ACT actors and conditions. Derived files go to the import cache, never the author's package.
use crate::{
    MstsError,
    path_placement::{native_path_polylines, snap_world_to_edge, switches_for_edges},
};
use openrailsrs_core::activity::{ActivityCondition, ActivityDefinition, ActivitySiding};
use openrailsrs_formats::{
    ActivityFile, ConsistEntry, ConsistFile, TSectionCatalog, TrItemKind, TrackDbFile,
    TrackNodeKind,
};
use openrailsrs_scenarios::{ScenarioFile, TrainEntryDef};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

fn content_root(route: &Path) -> Result<PathBuf, MstsError> {
    route
        .ancestors()
        .take(5)
        .find_map(|p| {
            openrailsrs_formats::resolve_path_case_insensitive(&p.join("TRAINS/TRAINSET"))
        })
        .ok_or_else(|| MstsError::msg("ACT requires its original TRAINS/TRAINSET"))
}
fn original_consist(route: &Path, name: &str) -> Result<PathBuf, MstsError> {
    let name = Path::new(name)
        .file_name()
        .ok_or_else(|| MstsError::msg("ACT has no original consist name"))?;
    route
        .ancestors()
        .take(5)
        .find_map(|p| {
            openrailsrs_formats::resolve_path_case_insensitive(
                &p.join("TRAINS/CONSISTS").join(name),
            )
        })
        .ok_or_else(|| MstsError::msg(format!("Missing ACT consist {}", name.to_string_lossy())))
}
pub(crate) fn service_uids(route: &Path, name: &str, service: u32) -> Result<Vec<u32>, MstsError> {
    let con = original_consist(route, name)?;
    let ast = openrailsrs_formats::parse_vehicle_text(
        &openrailsrs_formats::read_msts_file_to_string(&con)?,
    )?;
    ConsistFile::from_ast(&ast)?
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let uid = e.uid().unwrap_or(i as u32);
            if uid > 65535 {
                return Err(MstsError::msg("ACT vehicle UiD exceeds16bits"));
            }
            Ok(service << 16 | uid)
        })
        .collect()
}

fn static_vehicle_source(
    trainset: &Path,
    entry: &ConsistEntry,
) -> Result<Option<PathBuf>, MstsError> {
    let relative = entry.path().replace('\\', "/");
    // Original Chiltern ACTs contain WagonData ( "" "" ) placeholders.
    // OR1.6.1 Simulator.InitializeStaticConsists (1543–1547) warns and skips
    // that missing vehicle while retaining the actor and every real car.
    if matches!(entry, ConsistEntry::Wagon { .. }) && relative == "trains//.wag" {
        return Ok(None);
    }
    let suffix = relative
        .strip_prefix("trains/")
        .ok_or_else(|| MstsError::msg("Static ACT vehicle must use Trainset"))?;
    if Path::new(suffix)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || suffix
            .chars()
            .any(|c| c.is_control() || "\"<>|?:*".contains(c))
    {
        return Err(MstsError::msg(format!(
            "Unsafe static ACT resource path: {relative:?}"
        )));
    }
    let requested = trainset.join(suffix);
    let source =
        openrailsrs_formats::resolve_path_case_insensitive(&requested).ok_or_else(|| {
            MstsError::msg(format!(
                "Missing static ACT vehicle: {}",
                requested.display()
            ))
        })?;
    let source = source.canonicalize()?;
    if !source.starts_with(trainset.canonicalize()?) {
        return Err(MstsError::msg("Static ACT resource outside Trainset"));
    }
    if source.to_string_lossy().contains('"') {
        return Err(MstsError::msg("Unsupported quote in native content path"));
    }
    Ok(Some(source))
}

fn activity_track_database(route: &Path) -> Result<TrackDbFile, MstsError> {
    let path = std::fs::read_dir(route)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tdb")))
        .ok_or_else(|| MstsError::msg("Native ACT requires original TDB"))?;
    Ok(TrackDbFile::from_path(path)?)
}

fn static_track_extension(
    loaded: &openrailsrs_route::LoadedRoute,
    tdb: &TrackDbFile,
    catalog: &TSectionCatalog,
    current: &str,
    backward: bool,
) -> Result<String, MstsError> {
    let base = current.strip_suffix("_r").unwrap_or(current);
    let original = loaded
        .msts_aliases
        .values()
        .find(|alias| alias.is_edge() && alias.id == base)
        .ok_or_else(|| MstsError::msg(format!("ACT static vector {base} has no native alias")))?;
    let mut node_id = original.tdb_id;
    let mut forward = !current.ends_with("_r");
    if backward {
        forward = !forward;
    }
    let mut visited = HashSet::new();
    loop {
        if !visited.insert((node_id, forward)) {
            return Err(MstsError::msg(
                "ACT static native pins form a cycle without a vector",
            ));
        }
        let node = tdb
            .node_by_id(node_id)
            .ok_or_else(|| MstsError::msg(format!("ACT static native node {node_id} missing")))?;
        // OR1.6.1 Traveller.NextTrackNode: trailing movement always uses
        // common pin0; facing movement uses Inpins + SelectedRoute.
        // Signals initializes SelectedRoute from TrackShape.MainRoute.
        let pin_index = match &node.kind {
            TrackNodeKind::Vector { .. } => usize::from(forward),
            TrackNodeKind::Junction { .. } if forward => {
                1 + node
                    .junction_shape_index
                    .and_then(|index| catalog.shapes.get(&index))
                    .and_then(|shape| shape.main_route)
                    .unwrap_or(0) as usize
            }
            TrackNodeKind::Junction { .. } => 0,
            TrackNodeKind::End => {
                return Err(MstsError::msg(format!(
                    "ACT static reaches end of track at native node {node_id}"
                )));
            }
        };
        let pin = node.pin_refs.get(pin_index).ok_or_else(|| {
            MstsError::msg(format!(
                "ACT static native node {node_id} has no pin {pin_index}"
            ))
        })?;
        node_id = pin.node_id;
        // Despite this legacy field name, native TrPin's second integer is
        // the Traveller direction on the linked node, not a branch index.
        forward = match pin.branch_index {
            0 => false,
            1 => true,
            direction => {
                return Err(MstsError::msg(format!(
                    "ACT static native node {} pin {pin_index} has invalid direction {direction}",
                    node.id
                )));
            }
        };
        if !matches!(
            tdb.node_by_id(node_id).map(|node| &node.kind),
            Some(TrackNodeKind::Vector { .. })
        ) {
            continue;
        }
        let alias = loaded
            .msts_aliases
            .get(&node_id)
            .filter(|alias| alias.is_edge())
            .ok_or_else(|| {
                MstsError::msg(format!(
                    "ACT static native vector {node_id} has no imported edge"
                ))
            })?;
        if backward {
            forward = !forward;
        }
        let id = if forward {
            alias.id.clone()
        } else {
            format!("{}_r", alias.id)
        };
        let previous = loaded
            .graph
            .edge(current)
            .ok_or_else(|| MstsError::msg("ACT static current edge missing"))?;
        let next = loaded
            .graph
            .edge(&id)
            .ok_or_else(|| MstsError::msg("ACT static linked edge missing"))?;
        if if backward {
            next.to != previous.from
        } else {
            previous.to != next.from
        } {
            return Err(MstsError::msg(format!(
                "ACT static native pins do not connect imported edges {current} and {id}"
            )));
        }
        return Ok(id);
    }
}

pub(crate) fn apply(
    route: &Path,
    act: &Path,
    pat: &Path,
    cache: Option<&Path>,
    activity: &ActivityFile,
    scenario: &mut ScenarioFile,
) -> Result<(), MstsError> {
    if activity.events.is_empty() && activity.static_trains.is_empty() {
        return Ok(());
    }
    let mut definition = ActivityDefinition {
        events: activity.events.clone(),
        ..Default::default()
    };
    let con = original_consist(route, &scenario.train.consist)?;
    let ast = openrailsrs_formats::parse_vehicle_text(
        &openrailsrs_formats::read_msts_file_to_string(&con)?,
    )?;
    let entries = ConsistFile::from_ast(&ast)?.entries;
    for (i, entry) in entries.iter().enumerate() {
        let uid = entry.uid().unwrap_or(i as u32);
        if uid > 65535 {
            return Err(MstsError::msg("ACT consist vehicle UiD exceeds 16 bits"));
        }
        definition
            .vehicle_uids
            .push((activity.player_uid << 16) | uid);
    }
    if activity
        .events
        .iter()
        .any(|e| matches!(e.condition, ActivityCondition::AllStops))
        && activity.player_stops.is_empty()
    {
        definition.diagnostics.push(
            "AllStops no tiene tareas de estación; no se considera cumplido al terminar el PAT"
                .into(),
        );
    }
    for e in &activity.events {
        if let ActivityCondition::Unsupported { token } = &e.condition {
            definition
                .diagnostics
                .push(format!("Evento ACT {} no admitido: {token}", e.id));
        }
        for token in &e.outcomes.unsupported {
            definition
                .diagnostics
                .push(format!("Acción ACT {} no admitida: {token}", e.id));
        }
    }
    let cache =
        cache.ok_or_else(|| MstsError::msg("Import track.toml before native ACT actors/events"))?;
    let cache = cache.canonicalize()?;
    let loaded = openrailsrs_route::load_route_from_dir(&cache)
        .map_err(|e| MstsError::msg(e.to_string()))?;
    let polylines = native_path_polylines(&loaded.graph, &loaded.msts_aliases, pat)?;
    for event in &mut definition.events {
        if let ActivityCondition::Location {
            tile_x,
            tile_z,
            x,
            z,
            edge,
            position_m,
            ..
        } = &mut event.condition
        {
            let (start, position, dest) = snap_world_to_edge(
                &loaded.graph,
                f64::from(*tile_x) * 2048. + *x,
                -(f64::from(*tile_z) * 2048. + *z),
                None,
                polylines.as_ref(),
            )?;
            let id = loaded
                .graph
                .outgoing_edges(&start)
                .iter()
                .find(|id| loaded.graph.edge(id).is_some_and(|e| e.to.0 == dest))
                .ok_or_else(|| MstsError::msg("ACT location has no vector"))?;
            let data = loaded.graph.edge(id).unwrap();
            *edge = Some(id.strip_suffix("_r").unwrap_or(id).to_string());
            *position_m = Some(if id.ends_with("_r") {
                data.length_m - position
            } else {
                position
            });
        }
    }
    let trainset = content_root(route)?;
    let tag = format!("{:x}", Sha256::digest(std::fs::read(act)?));
    let generated = cache.join("native-activity").join(&tag[..16]);
    if !activity.static_trains.is_empty() {
        std::fs::create_dir_all(&generated)?;
    }
    let static_track = if activity.static_trains.is_empty() {
        None
    } else {
        Some((
            activity_track_database(route)?,
            TSectionCatalog::load_for_route(route)?,
        ))
    };
    for object in &activity.static_trains {
        // OR InitializeStaticConsists first reverses the source order/flip,
        // then ReverseFormation(false) reverses it again. The final source
        // order is unchanged, and Tile locates the centre of the first car.
        let mut text = String::from("(Train\n");
        let mut uids = vec![];
        for entry in &object.entries {
            let Some(source) = static_vehicle_source(&trainset, entry)? else {
                definition.diagnostics.push(format!("ACT estático {}: WagonData vacío UID {} ignorado como en OR 1.6.1; se conservan los demás coches", object.id, entry.uid().unwrap_or_default()));
                continue;
            };
            let kind = if matches!(entry, ConsistEntry::Engine { .. }) {
                "Engine"
            } else {
                "Wagon"
            };
            let uid = entry
                .uid()
                .ok_or_else(|| MstsError::msg("Missing static vehicle UiD"))?;
            uids.push((object.id << 16) | uid);
            text.push_str(&format!(
                " ({kind} \"{}\" (UiD {uid}) (Flip {}))\n",
                source.display(),
                u8::from(entry.flipped())
            ));
        }
        text.push_str(")\n");
        let file = generated.join(format!("static-{}.con", object.id));
        if std::fs::read_to_string(&file).ok().as_deref() != Some(&text) {
            std::fs::write(&file, text)?;
        }
        let consist = openrailsrs_train::load_consist_with_asset_root(&file, &trainset)
            .map_err(|e| MstsError::msg(e.to_string()))?;
        let lengths = consist.vehicle_lengths_m();
        let length = lengths.iter().sum::<f64>();
        let (start, at, destination) = snap_world_to_edge(
            &loaded.graph,
            object.tile[0] * 2048. + object.tile[2],
            -(object.tile[1] * 2048. + object.tile[3]),
            None,
            polylines.as_ref(),
        )?;
        let snapped = loaded
            .graph
            .outgoing_edges(&start)
            .iter()
            .find(|id| loaded.graph.edge(id).is_some_and(|e| e.to.0 == destination))
            .ok_or_else(|| MstsError::msg("ACT static placement has no vector"))?;
        let base = snapped.strip_suffix("_r").unwrap_or(snapped);
        let native = loaded
            .graph
            .edge(base)
            .ok_or_else(|| MstsError::msg("ACT static native vector missing"))?;
        let native_at = if snapped.ends_with("_r") {
            native.length_m - at
        } else {
            at
        };
        let selected = if object.direction {
            format!("{base}_r")
        } else {
            base.to_string()
        };
        let edge = loaded
            .graph
            .edge(&selected)
            .ok_or_else(|| MstsError::msg("ACT static reverse vector missing"))?;
        let mut head = if object.direction {
            edge.length_m - native_at
        } else {
            native_at
        } + lengths[0] / 2.;
        let mut edges = vec![selected];
        // Retain vectors under the entire stationary formation. Native pins
        // distinguish a trailing move from an illegal branch-to-branch turn.
        let (tdb, catalog) = static_track.as_ref().expect("native actors loaded above");
        while head < length {
            let previous = static_track_extension(&loaded, tdb, catalog, &edges[0], true).map_err(
                |error| MstsError::msg(format!("ACT static {} rear: {error}", object.id)),
            )?;
            head += loaded.graph.edge(&previous).unwrap().length_m;
            edges.insert(0, previous);
        }
        while head
            >= edges
                .iter()
                .map(|id| loaded.graph.edge(id).unwrap().length_m)
                .sum::<f64>()
        {
            let next = static_track_extension(&loaded, tdb, catalog, edges.last().unwrap(), false)
                .map_err(|error| {
                    MstsError::msg(format!("ACT static {} front: {error}", object.id))
                })?;
            edges.push(next);
        }
        let start = loaded.graph.edge(&edges[0]).unwrap().from.0.clone();
        let destination = loaded
            .graph
            .edge(edges.last().unwrap())
            .unwrap()
            .to
            .0
            .clone();
        let mut waypoints = vec![start.clone()];
        waypoints.extend(
            edges
                .iter()
                .map(|id| loaded.graph.edge(id).unwrap().to.0.clone()),
        );
        scenario.extra_trains.push(TrainEntryDef {
            activity_uids: uids,
            bogie_dynamics: vec![],
            legs: vec![],
            presentation: vec![],
            cab_equipment: vec![],
            pre_roll_s: 0.,
            relative_stop_times: false,
            parked: true,
            freight: None,
            systems: vec![],
            electric_pickups: vec![],
            id: format!("act-static-{}", object.id),
            consist: file.to_string_lossy().into_owned(),
            start,
            destination,
            start_time_s: 0.,
            waypoints,
            start_offset_m: head,
            stops: vec![],
            davis: None,
            switches: switches_for_edges(&loaded.graph, &edges),
            output_csv: format!("static-{}.csv", object.id),
        });
    }
    let required = definition
        .events
        .iter()
        .filter_map(|e| match &e.condition {
            ActivityCondition::AssembleTrain { siding, .. }
            | ActivityCondition::DropOffWagons { siding, .. } => *siding,
            _ => None,
        })
        .collect::<Vec<_>>();
    if !required.is_empty() {
        let tdb = activity_track_database(route)?;
        let mut hosts = HashMap::new();
        for n in &tdb.nodes {
            if let TrackNodeKind::Vector { item_ids, .. } = &n.kind {
                for id in item_ids {
                    hosts.insert(*id, n.id);
                }
            }
        }
        for id in required {
            if definition.sidings.iter().any(|s| s.id == id) {
                continue;
            }
            let item = tdb
                .items
                .iter()
                .find(|i| i.id == id)
                .ok_or_else(|| MstsError::msg(format!("ACT siding {id} missing")))?;
            let TrItemKind::Siding { pair_id, .. } = item.kind else {
                return Err(MstsError::msg(format!("ACT item {id} is not a siding")));
            };
            let pair = tdb
                .items
                .iter()
                .find(|i| i.id == pair_id)
                .ok_or_else(|| MstsError::msg("ACT siding pair missing"))?;
            if hosts.get(&id) != hosts.get(&pair_id) {
                return Err(MstsError::msg(
                    "ACT siding across several vectors requires additional validation",
                ));
            }
            let vector = hosts
                .get(&id)
                .ok_or_else(|| MstsError::msg("ACT siding has no vector"))?;
            let alias = loaded
                .msts_aliases
                .get(vector)
                .ok_or_else(|| MstsError::msg("ACT siding vector not imported"))?;
            definition.sidings.push(ActivitySiding {
                id,
                edge: alias.id.clone(),
                start_m: item.distance_m.min(pair.distance_m),
                end_m: item.distance_m.max(pair.distance_m),
            });
        }
    }
    if !definition.valid() {
        return Err(MstsError::msg(
            "ACT definition has invalid or duplicate identities/events",
        ));
    }
    scenario.native_activity = Some(definition);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_track_extension_uses_native_common_pin_and_main_route() {
        use openrailsrs_core::{EdgeId, NodeId};
        use openrailsrs_formats::typed::TrackShapeDef;
        use openrailsrs_formats::{TrPinRef, TrackDbNode};
        use openrailsrs_route::{LoadedRoute, MstsAlias};
        use openrailsrs_track::{Edge, Node, NodeKind, TrackGraph};

        let pins = |values: &[(u32, u8)]| {
            values
                .iter()
                .map(|&(node_id, branch_index)| TrPinRef {
                    node_id,
                    branch_index,
                })
                .collect::<Vec<_>>()
        };
        let vector = |id, links: &[(u32, u8)]| TrackDbNode {
            id,
            junction_shape_index: None,
            position: None,
            pin_refs: pins(links),
            kind: TrackNodeKind::Vector {
                length_m: 100.,
                speed_limit_mps: 20.,
                pins: (links[0].0, links[1].0),
                item_ids: vec![],
                sections: vec![],
                geometry: None,
            },
        };
        let mut tdb = TrackDbFile {
            nodes: vec![
                TrackDbNode {
                    id: 1,
                    junction_shape_index: Some(42),
                    position: None,
                    pin_refs: pins(&[(20, 1), (10, 1), (30, 0)]),
                    kind: TrackNodeKind::Junction {
                        pins: pins(&[(20, 1), (10, 1), (30, 0)]),
                    },
                },
                vector(10, &[(1, 0), (2, 1)]),
                vector(20, &[(1, 1), (9, 1)]),
                vector(30, &[(3, 0), (1, 0)]),
            ],
            items: vec![],
        };
        for id in [2, 3, 9] {
            tdb.nodes.push(TrackDbNode {
                id,
                junction_shape_index: None,
                position: None,
                pin_refs: vec![],
                kind: TrackNodeKind::End,
            });
        }
        let mut graph = TrackGraph::new();
        for id in [1, 2, 3, 9] {
            graph
                .insert_node(Node {
                    id: NodeId(format!("n{id}")),
                    x_m: 0.,
                    y_m: 0.,
                    kind: if id == 1 {
                        NodeKind::Switch {
                            stem_edge: EdgeId("e10".into()),
                            diverging_edge: EdgeId("e30_r".into()),
                        }
                    } else {
                        NodeKind::Plain
                    },
                })
                .unwrap();
        }
        let mut aliases = HashMap::new();
        for (id, from, to, length_m) in [(10, 1, 2, 100.), (20, 1, 9, 20.), (30, 3, 1, 80.)] {
            let from = format!("n{from}");
            let to = format!("n{to}");
            let name = format!("e{id}");
            for (name, from, to) in [
                (name.clone(), from.clone(), to.clone()),
                (format!("{name}_r"), to.clone(), from.clone()),
            ] {
                graph
                    .insert_edge(Edge {
                        id: EdgeId(name),
                        from: NodeId(from),
                        to: NodeId(to),
                        length_m,
                        speed_limit_mps: 20.,
                        grade_percent: 0.,
                    })
                    .unwrap();
            }
            aliases.insert(
                id,
                MstsAlias {
                    tdb_id: id,
                    kind: "edge".into(),
                    id: name,
                    from: Some(from),
                    to: Some(to),
                },
            );
        }
        let loaded = LoadedRoute {
            graph,
            msts_aliases: aliases,
        };
        let mut catalog = TSectionCatalog::default();
        for main_route in [0, 1] {
            catalog.shapes.insert(
                42,
                TrackShapeDef {
                    file_name: String::new(),
                    main_route: Some(main_route),
                    clearance_dist_m: None,
                    road_shape: false,
                    paths: vec![],
                },
            );
            // A long static formation's tail crosses this 3-pin junction.
            // Trailing movement must use the common pin, even with route1.
            assert_eq!(
                static_track_extension(&loaded, &tdb, &catalog, "e10", true).unwrap(),
                "e20_r"
            );
            // Facing movement follows the native MainRoute and pin Direction.
            assert_eq!(
                static_track_extension(&loaded, &tdb, &catalog, "e20_r", false).unwrap(),
                if main_route == 0 { "e10" } else { "e30_r" }
            );
        }
        assert!(
            static_track_extension(&loaded, &tdb, &catalog, "e10", false).is_err(),
            "a real end of track cannot be extended"
        );
        tdb.nodes
            .iter_mut()
            .find(|node| node.id == 10)
            .unwrap()
            .pin_refs[0]
            .branch_index = 2;
        assert!(
            static_track_extension(&loaded, &tdb, &catalog, "e10", true)
                .unwrap_err()
                .to_string()
                .contains("invalid direction 2")
        );
    }

    fn wagon(path: &str) -> ConsistEntry {
        ConsistEntry::Wagon {
            path: path.into(),
            uid: Some(3),
            flipped: false,
        }
    }

    #[test]
    fn static_vehicle_skips_only_exact_empty_wagon_data() {
        let trainset = tempfile::tempdir().unwrap();
        std::fs::create_dir(trainset.path().join("Stock")).unwrap();
        let real = trainset.path().join("Stock/real.wag");
        std::fs::write(&real, "original vehicle").unwrap();
        let ast = openrailsrs_formats::parse_vehicle_text(
            r#"Train (
                Wagon ( WagonData ( "" "" ) UiD ( 3 ) )
                Wagon ( WagonData ( "real" "Stock" ) UiD ( 4 ) )
            )"#,
        )
        .unwrap();
        let entries = ConsistFile::from_ast(&ast).unwrap().entries;
        assert_eq!(entries[0].path(), "trains//.wag");
        assert_eq!(
            static_vehicle_source(trainset.path(), &entries[0]).unwrap(),
            None
        );
        assert_eq!(
            static_vehicle_source(trainset.path(), &entries[1]).unwrap(),
            Some(real.canonicalize().unwrap())
        );
        assert_eq!(entries[1].uid(), Some(4));
        assert!(
            static_vehicle_source(trainset.path(), &wagon("trains/Stock/missing.wag"))
                .unwrap_err()
                .to_string()
                .contains("Missing static ACT vehicle")
        );
        assert!(static_vehicle_source(trainset.path(), &wagon("trains/Stock/.wag")).is_err());
        assert!(
            static_vehicle_source(
                trainset.path(),
                &ConsistEntry::Engine {
                    path: "trains//.eng".into(),
                    uid: Some(3),
                    flipped: false,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn static_vehicle_rejects_unsafe_nonempty_references() {
        let trainset = tempfile::tempdir().unwrap();
        for path in [
            "trains/../outside.wag",
            "trains//absolute.wag",
            "/absolute.wag",
            "trains/Stock/../../outside.wag",
            "trains/Stock/car?.wag",
            "trains/Stock/car*.wag",
            "trains/C:/car.wag",
            "trains/Stock/car\".wag",
            "trains/Stock/car\n.wag",
        ] {
            assert!(
                static_vehicle_source(trainset.path(), &wagon(path)).is_err(),
                "{path:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn static_vehicle_rejects_symlink_outside_trainset() {
        let root = tempfile::tempdir().unwrap();
        let trainset = root.path().join("Trainset");
        std::fs::create_dir(&trainset).unwrap();
        let outside = root.path().join("outside.wag");
        std::fs::write(&outside, "outside vehicle").unwrap();
        std::os::unix::fs::symlink(&outside, trainset.join("escape.wag")).unwrap();
        assert!(
            static_vehicle_source(&trainset, &wagon("trains/escape.wag"))
                .unwrap_err()
                .to_string()
                .contains("outside Trainset")
        );
    }
}

//! ACT event/loose-consist metadata, retaining native identifiers and unknown tokens.
use super::{ConsistEntry, ConsistFile, atom_to_number, atom_to_string, named_blocks};
use crate::{Ast, Atom, FormatError};
use openrailsrs_core::activity::{ActivityCondition, ActivityEvent, ActivityOutcomes};

#[derive(Clone, Debug, PartialEq)]
pub struct StaticActivityTrain {
    pub id: u32,
    pub direction: bool,
    /// Native Tile (tileX tileZ localX localZ), locating the first source car's centre.
    pub tile: [f64; 4],
    pub entries: Vec<ConsistEntry>,
}

fn error(key: &str) -> FormatError {
    FormatError::UnexpectedAtom {
        key: key.into(),
        context: "native ACT event".into(),
        expected: "finite, complete original event metadata".into(),
    }
}

// The recovered ACT parser preserves native `key (...)` siblings. Convert
// only those siblings, without rewriting the source text or quoted messages.
fn normalize_block(ast: &Ast, key: &str) -> Ast {
    let Ast::List(items) = ast else {
        return ast.clone();
    };
    let skip = usize::from(
        matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(key)),
    );
    let mut out = vec![Ast::Atom(Atom::Symbol(key.into()))];
    let mut index = skip;
    while index < items.len() {
        if let Ast::Atom(Atom::Symbol(name)) = &items[index]
            && let Some(Ast::List(_)) = items.get(index + 1)
        {
            out.push(normalize_block(&items[index + 1], name));
            index += 2;
        } else {
            let child = match &items[index] {
                Ast::List(body) => match body.first() {
                    Some(Ast::Atom(Atom::Symbol(name))) => normalize_block(&items[index], name),
                    _ => items[index].clone(),
                },
                _ => items[index].clone(),
            };
            out.push(child);
            index += 1;
        }
    }
    Ast::List(out)
}

fn values(ast: &Ast, key: &str) -> Vec<Ast> {
    named_blocks(ast, key).first().map_or_else(Vec::new, |body| {
        let Ast::List(items) = body else { return vec![] };
        items.iter().filter(|v| matches!(v, Ast::Atom(_))).skip(usize::from(matches!(items.first(), Some(Ast::Atom(Atom::Symbol(s))) if s.eq_ignore_ascii_case(key)))).cloned().collect()
    })
}
fn text(ast: &Ast, key: &str) -> Option<String> {
    values(ast, key).first().and_then(|v| {
        if let Ast::Atom(a) = v {
            atom_to_string(a)
        } else {
            None
        }
    })
}
fn number(ast: &Ast, key: &str) -> Result<Option<f64>, FormatError> {
    let vals = values(ast, key);
    let Some(Ast::Atom(a)) = vals.first() else {
        return Ok(None);
    };
    atom_to_number(a)
        .filter(|v| v.is_finite())
        .map(Some)
        .ok_or_else(|| error(key))
}
fn direct_number(ast: &Ast, key: &str) -> Result<Option<f64>, FormatError> {
    let Ast::List(items) = ast else {
        return Ok(None);
    };
    for item in items {
        if let Ast::List(body) = item
            && matches!(body.first(), Some(Ast::Atom(Atom::Symbol(name))) if name.eq_ignore_ascii_case(key))
        {
            return number(item, key);
        }
    }
    Ok(None)
}
fn int(ast: &Ast, key: &str, default: i32) -> Result<i32, FormatError> {
    let n = number(ast, key)?.unwrap_or(f64::from(default));
    if n.fract() != 0. || n < i32::MIN as f64 || n > i32::MAX as f64 {
        return Err(error(key));
    }
    Ok(n as i32)
}
fn tokens(ast: &Ast) -> Vec<String> {
    let Ast::List(items) = ast else { return vec![] };
    items
        .iter()
        .filter_map(|v| {
            let Ast::List(body) = v else { return None };
            match body.first() {
                Some(Ast::Atom(Atom::Symbol(s))) => Some(s.clone()),
                _ => None,
            }
        })
        .collect()
}
fn ids(ast: &Ast, key: &str) -> Result<Vec<i32>, FormatError> {
    named_blocks(ast, key)
        .iter()
        .map(|b| int(b, key, 0))
        .collect()
}

pub(super) fn parse_events(ast: &Ast) -> Result<Vec<ActivityEvent>, FormatError> {
    fn visit(ast: &Ast, order: &mut std::collections::HashMap<usize, usize>) {
        let n = order.len();
        order.insert(ast as *const Ast as usize, n);
        if let Ast::List(items) = ast {
            for item in items {
                visit(item, order);
            }
        }
    }
    let mut order = std::collections::HashMap::new();
    visit(ast, &mut order);
    let mut result = vec![];
    for category in [
        "EventCategoryAction",
        "EventCategoryLocation",
        "EventCategoryTime",
    ] {
        for raw in named_blocks(ast, category) {
            let normalized = normalize_block(raw, category);
            let event = &normalized;
            let wagon_ids = || -> Result<Vec<u32>, FormatError> {
                let mut uids = vec![];
                for list in named_blocks(event, "Wagon_List") {
                    for block in named_blocks(list, "UiD") {
                        let n = number(block, "UiD")?.ok_or_else(|| error("UiD"))?;
                        if n.fract() != 0. || !(0. ..=u32::MAX as f64).contains(&n) {
                            return Err(error("UiD"));
                        }
                        uids.push(n as u32);
                    }
                }
                Ok(uids)
            };
            // Wagon_List has a per-car SidingItem sentinel (often UINT_MAX).
            // The event's destination is its own direct child, as in BNSF
            // 4_lumberrun's Drop Woodchips: sentinel UINT_MAX, destination 553.
            let siding = direct_number(event, "SidingItem")?
                .map(|n| {
                    if n.fract() == 0. && (0. ..=u32::MAX as f64).contains(&n) {
                        Ok(n as u32)
                    } else {
                        Err(error("SidingItem"))
                    }
                })
                .transpose()?;
            let present = |key: &str| !named_blocks(event, key).is_empty();
            let condition = if present("ORTSTriggeringTrain") {
                ActivityCondition::Unsupported {
                    token: "ORTSTriggeringTrain".into(),
                }
            } else if category == "EventCategoryTime" {
                ActivityCondition::Time {
                    seconds: number(event, "Time")?.unwrap_or(0.).trunc(),
                }
            } else if category == "EventCategoryLocation" {
                let location = values(event, "Location")
                    .into_iter()
                    .filter_map(|v| {
                        if let Ast::Atom(a) = v {
                            atom_to_number(&a)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if location.len() != 5
                    || location.iter().any(|n| !n.is_finite())
                    || location[0].fract() != 0.
                    || location[1].fract() != 0.
                    || location[4] <= 0.
                {
                    return Err(error("Location"));
                }
                ActivityCondition::Location {
                    tile_x: location[0] as i32,
                    tile_z: location[1] as i32,
                    x: location[2],
                    z: location[3],
                    radius_m: location[4],
                    on_stop: int(event, "TriggerOnStop", 0)? != 0,
                    edge: None,
                    position_m: None,
                }
            } else if present("EventTypeAllStops") {
                ActivityCondition::AllStops
            } else if present("EventTypeReachSpeed") {
                ActivityCondition::ReachSpeed {
                    speed_mps: number(event, "Speed")?.unwrap_or(0.),
                }
            } else if present("EventTypePickUpWagons") {
                ActivityCondition::PickUpWagons { uids: wagon_ids()? }
            } else if present("EventTypeAssembleTrain")
                || present("EventTypeAssembleTrainAtLocation")
            {
                ActivityCondition::AssembleTrain {
                    uids: wagon_ids()?,
                    siding: if present("EventTypeAssembleTrainAtLocation") {
                        Some(siding.ok_or_else(|| error("SidingItem"))?)
                    } else {
                        None
                    },
                }
            } else if present("EventTypeDropOffWagonsAtLocation") {
                ActivityCondition::DropOffWagons {
                    uids: wagon_ids()?,
                    siding: Some(siding.ok_or_else(|| error("SidingItem"))?),
                }
            } else {
                ActivityCondition::Unsupported {
                    token: tokens(event)
                        .into_iter()
                        .find(|t| t.to_ascii_lowercase().starts_with("eventtype"))
                        .unwrap_or_else(|| category.into()),
                }
            };
            let mut outcomes = ActivityOutcomes::default();
            for block in named_blocks(event, "Outcomes") {
                outcomes.message = text(block, "DisplayMessage");
                outcomes.success = !named_blocks(block, "ActivitySuccess").is_empty();
                outcomes.failure = text(block, "ActivityFail");
                outcomes.activate.extend(ids(block, "ActivateEvent")?);
                outcomes.restore.extend(ids(block, "RestoreActLevel")?);
                outcomes.increment.extend(ids(block, "IncActLevel")?);
                outcomes.decrement.extend(ids(block, "DecActLevel")?);
                outcomes
                    .unsupported
                    .extend(tokens(block).into_iter().filter(|token| {
                        !matches!(
                            token.to_ascii_lowercase().as_str(),
                            "displaymessage"
                                | "activitysuccess"
                                | "activityfail"
                                | "activateevent"
                                | "restoreactlevel"
                                | "incactlevel"
                                | "decactlevel"
                        )
                    }));
            }
            result.push((
                order[&(raw as *const Ast as usize)],
                ActivityEvent {
                    id: int(event, "ID", 0)?,
                    name: text(event, "Name").unwrap_or_default(),
                    activation: int(event, "Activation_Level", 0)?,
                    reversible: present("Reversable_Event") || present("Reversible_Event"),
                    condition,
                    outcomes,
                },
            ));
        }
    }
    result.sort_by_key(|e| e.0);
    let mut ids = std::collections::HashSet::new();
    if result.iter().any(|(_, e)| !ids.insert(e.id)) {
        return Err(error("duplicate event ID"));
    }
    Ok(result.into_iter().map(|(_, e)| e).collect())
}

pub(super) fn parse_statics(ast: &Ast) -> Result<Vec<StaticActivityTrain>, FormatError> {
    let mut trains = vec![];
    for raw in named_blocks(ast, "ActivityObject") {
        let normalized = normalize_block(raw, "ActivityObject");
        let block = &normalized;
        if !text(block, "ObjectType").is_some_and(|s| s.eq_ignore_ascii_case("WagonsList")) {
            continue;
        }
        let tile = values(block, "Tile")
            .iter()
            .filter_map(|v| {
                if let Ast::Atom(a) = v {
                    atom_to_number(a)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if tile.len() != 4
            || tile.iter().any(|v| !v.is_finite())
            || tile[0].fract() != 0.
            || tile[1].fract() != 0.
        {
            return Err(error("static Tile"));
        }
        let id = int(block, "ID", -1)?;
        if !(0..=65535).contains(&id) {
            return Err(error("static ID"));
        }
        let cfg = named_blocks(block, "Train_Config")
            .first()
            .copied()
            .ok_or_else(|| error("Train_Config"))?;
        let entries = ConsistFile::from_ast(cfg)?.entries;
        if entries.iter().any(|e| e.uid().is_none_or(|id| id > 65535)) {
            return Err(error("static vehicle UiD"));
        }
        trains.push(StaticActivityTrain {
            id: id as u32,
            direction: int(block, "Direction", 0)? != 0,
            tile: tile.try_into().unwrap(),
            entries,
        });
    }
    Ok(trains)
}

pub(super) fn player_uid(ast: &Ast) -> Result<u32, FormatError> {
    let n = named_blocks(ast, "Player_Service_Definition")
        .first()
        .map_or(Ok(0), |b| {
            int(&normalize_block(b, "Player_Service_Definition"), "UiD", 0)
        })?;
    if !(0..=65535).contains(&n) {
        return Err(error("player UiD"));
    }
    Ok(n as u32)
}
pub(super) fn service_uid(ast: &Ast) -> Result<Option<u32>, FormatError> {
    let normalized = normalize_block(ast, "Service_Definition");
    let Some(n) = number(&normalized, "UiD")? else {
        return Ok(None);
    };
    if n.fract() != 0. || !(0. ..=65535.).contains(&n) {
        return Err(error("service UiD"));
    }
    Ok(Some(n as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_act_preserves_wagon_conditions_outcomes_and_static_ids() {
        let ast=crate::parse_activity_text("Tr_Activity ( Tr_Activity_Header ( Name ( \"Test\" ) ) Tr_Activity_File ( Player_Service_Definition ( player UiD ( 2 ) ) Events ( EventCategoryAction ( EventTypeAssembleTrainAtLocation () ID ( 8 ) Activation_Level ( 1 ) Wagon_List ( UiD ( 2147483648 ) SidingItem ( 4294967295 ) UiD ( 2147483649 ) SidingItem ( 4294967295 ) ) SidingItem ( 3 ) Outcomes ( IncActLevel ( 9 ) DisplayMessage ( \"Original message\" ) ) ) EventCategoryTime ( ID ( 9 ) Activation_Level ( 0 ) Time ( 20 ) Outcomes ( ActivitySuccess () ) ) ) ActivityObjects ( ActivityObject ( ObjectType ( WagonsList ) Train_Config ( TrainCfg ( \"\" Wagon ( WagonData ( box author ) UiD ( 0 ) ) Wagon ( WagonData ( box author ) UiD ( 1 ) Flip () ) ) ) ID ( 32768 ) Direction ( 0 ) Tile ( 1 -2 10 20 ) ) ) ) )").unwrap();
        let events = parse_events(&ast).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, 8);
        assert!(
            matches!(&events[0].condition,ActivityCondition::AssembleTrain{uids,siding:Some(3)} if uids==&[2147483648,2147483649])
        );
        assert_eq!(events[0].outcomes.increment, [9]);
        assert_eq!(
            events[0].outcomes.message.as_deref(),
            Some("Original message")
        );
        let objects = parse_statics(&ast).unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].id, 32768);
        assert_eq!(objects[0].entries[1].uid(), Some(1));
        assert!(objects[0].entries[1].flipped());
        assert_eq!(player_uid(&ast).unwrap(), 2);
    }
    #[test]
    fn unsupported_act_event_is_not_relabelled_as_loading_or_success() {
        let ast=crate::parse_activity_text("(Tr_Activity (Events (EventCategoryAction (EventTypeLoadWagons) (ID 4) (Activation_Level 1) (Outcomes (ActivitySuccess)))))").unwrap();
        let events = parse_events(&ast).unwrap();
        assert!(
            matches!(&events[0].condition,ActivityCondition::Unsupported{token} if token=="EventTypeLoadWagons")
        );
    }
    #[test]
    fn event_source_order_and_native_integer_time_are_preserved() {
        let ast=crate::parse_activity_text("Tr_Activity ( Events ( EventCategoryTime ( ID ( 9 ) Time ( 5.9 ) Activation_Level ( 1 ) ) EventCategoryAction ( ID ( 1 ) EventTypeReachSpeed () Speed ( 2 ) Activation_Level ( 1 ) ) EventCategoryLocation ( ID ( 4 ) Location ( 1 2 3 4 5 ) Activation_Level ( 1 ) ORTSTriggeringTrain ( other ) ) ) )").unwrap();
        let events = parse_events(&ast).unwrap();
        assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), [9, 1, 4]);
        assert!(matches!(
            events[0].condition,
            ActivityCondition::Time { seconds: 5. }
        ));
        assert!(
            matches!(&events[2].condition,ActivityCondition::Unsupported{token} if token=="ORTSTriggeringTrain")
        );
    }

    #[test]
    fn explicit_native_service_is_distinct_from_the_header_path() {
        let ast = crate::parse_activity_text("Tr_Activity ( Tr_Activity_Header ( Name ( \"4. Everett Switching\" ) PathID ( \"Lumber Run\" ) ) Tr_Activity_File ( Player_Service_Definition ( LumberRun UiD ( 0 ) ) ) )").unwrap();
        let activity = super::super::ActivityFile::from_ast(&ast).unwrap();
        assert_eq!(activity.player_service_id.as_deref(), Some("LumberRun"));
        assert_eq!(activity.player_path, "PATHS/Lumber Run.pat");
    }
}

//! Native LevelCr metadata, shape keys and road stop points share one saved state.
use bevy::prelude::*;
use openrailsrs_formats::WorldItem;
use openrailsrs_sim::level_crossing::{CrossingDef, CrossingLane, CrossingState};
use std::collections::{HashMap, HashSet};
#[derive(Clone, Debug, PartialEq)]
pub struct CrossingWorld {
    pub id: String,
    pub track_ids: Vec<u32>,
    pub road_ids: Vec<u32>,
    pub warning_s: f64,
    pub minimum_m: f64,
    pub animation_s: f64,
    pub looped: bool,
}
impl CrossingWorld {
    pub fn from_item(x: i32, z: i32, item: &WorldItem) -> Option<Self> {
        let WorldItem::Other {
            tag,
            uid,
            tr_item_refs,
            crossing_parameters,
            crossing_timing,
            ..
        } = item
        else {
            return None;
        };
        if !tag.eq_ignore_ascii_case("LevelCr") {
            return None;
        }
        let parameters = crossing_parameters.unwrap_or([20., 10.]);
        let timing = crossing_timing.unwrap_or([0., 0., 4.]);
        Some(Self {
            id: format!("{x}:{z}:{}", uid.unwrap_or(0)),
            track_ids: tr_item_refs
                .iter()
                .filter(|r| r.db == 0)
                .map(|r| r.item_id)
                .collect(),
            road_ids: tr_item_refs
                .iter()
                .filter(|r| r.db == 1)
                .map(|r| r.item_id)
                .collect(),
            warning_s: parameters[0].clamp(0., 120.),
            minimum_m: parameters[1].clamp(0., 2000.),
            animation_s: timing[2].abs().clamp(0.1, 120.),
            looped: timing[2] < 0.,
        })
    }
}
#[derive(Component, Clone)]
pub struct CrossingAnimation {
    pub id: String,
    pub frames: f32,
    pub speed: f32,
    pub looped: bool,
}
impl CrossingAnimation {
    pub fn new(native: &CrossingWorld, shape: &openrailsrs_formats::ShapeFile) -> Self {
        let (frames, speed) = shape.animations.first().map_or((0., 0.), |a| {
            (
                if native.looped {
                    a.frame_count as f32
                } else {
                    a.frame_rate as f32 / 30.
                },
                a.frame_rate as f32 / 30. / native.animation_s as f32,
            )
        });
        Self {
            id: native.id.clone(),
            frames,
            speed,
            looped: native.looped,
        }
    }
}
#[derive(Resource, Default)]
pub struct RoadCrossings {
    pub stops: Vec<(Vec3, bool)>,
    pub unresolved: Vec<String>,
}
#[derive(Resource, Default)]
struct NativeCrossingRegistry {
    graph_edges: Option<usize>,
    initialized: bool,
    /// Absolute RDB points and failed registrations are cached together. A
    /// missing graph edge is stable during a play session, not a per-frame job.
    points: HashMap<String, Vec<Vec3>>,
    definitions: HashMap<String, Option<CrossingDef>>,
    visible: Vec<(String, Vec<Vec3>)>,
}
impl NativeCrossingRegistry {
    fn sync_graph(&mut self, edges: usize) -> bool {
        if self.graph_edges == Some(edges) {
            return false;
        }
        *self = Self {
            graph_edges: Some(edges),
            ..default()
        };
        true
    }
    fn retain_visible(&mut self) {
        let visible = self
            .visible
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<HashSet<_>>();
        self.points.retain(|id, _| visible.contains(id.as_str()));
        self.definitions
            .retain(|id, _| visible.contains(id.as_str()));
    }
}
pub fn install(app: &mut App) {
    app.init_resource::<RoadCrossings>()
        .init_resource::<NativeCrossingRegistry>()
        .add_systems(
            OnEnter(crate::ViewerAppState::Playing),
            |mut roads: ResMut<RoadCrossings>, mut cache: ResMut<NativeCrossingRegistry>| {
                *roads = default();
                *cache = default();
            },
        )
        .add_systems(
            Update,
            register
                .before(crate::road_cars::update_road_cars)
                .run_if(in_state(crate::ViewerAppState::Playing)),
        )
        .add_systems(
            Update,
            animate
                .before(openrailsrs_bevy_scenery::shapes::update_world_shape_anim)
                .run_if(in_state(crate::ViewerAppState::Playing)),
        );
}
fn register(
    world: Res<crate::world::WorldScene>,
    assets: Res<crate::shapes::RouteAssets>,
    focus: Res<crate::world::RouteFocus>,
    mut live: Option<ResMut<crate::live::LiveDrive>>,
    mut roads: ResMut<RoadCrossings>,
    mut cache: ResMut<NativeCrossingRegistry>,
) {
    let _profile = crate::performance::profile_system("level_crossing_register");
    let Some(live) = live.as_mut() else {
        return;
    };
    let Some(tdb) = assets.track_db() else {
        return;
    };
    // TrackGraph's iterator has an exact size hint, avoiding a graph scan.
    // Control/coupling can merge a service graph; retry missing links then.
    let graph_changed = cache.sync_graph(live.session.graph.edges_iter().size_hint().0);
    if graph_changed {
        roads.unresolved.clear();
    }
    if !cache.initialized || world.is_changed() || focus.is_changed() {
        cache.visible.clear();
        for object in &world.items {
            let Some(native) = &object.crossing else {
                continue;
            };
            if !cache.points.contains_key(&native.id) {
                let existing = live
                    .session
                    .state
                    .crossings
                    .get(&native.id)
                    .map(|crossing| crossing.definition.clone());
                let definition = if let Some(definition) = existing {
                    Some(definition)
                } else {
                    let lanes = native
                        .track_ids
                        .iter()
                        .filter_map(|id| {
                            let item = tdb.item_by_id(*id)?;
                            let host = tdb.tr_item_host(*id)?;
                            let edge = format!("e{}", host.vector_id);
                            let data = live.session.graph.edge(&edge)?;
                            Some(CrossingLane {
                                edge,
                                position_m: item.distance_m.clamp(0., data.length_m),
                            })
                        })
                        .collect::<Vec<_>>();
                    let definition = CrossingDef {
                        id: native.id.clone(),
                        lanes,
                        warning_s: native.warning_s,
                        minimum_m: native.minimum_m,
                        animation_s: native.animation_s,
                    };
                    if definition.valid() {
                        Some(definition)
                    } else {
                        if roads.unresolved.len() < 128 && !roads.unresolved.contains(&native.id) {
                            roads.unresolved.push(native.id.clone());
                        }
                        None
                    }
                };
                cache.definitions.insert(native.id.clone(), definition);
                let points = assets.road_db().map_or_else(Vec::new, |rdb| {
                    native
                        .road_ids
                        .iter()
                        .filter_map(|id| {
                            rdb.item_bevy_position(*id)
                                .map(|(x, y, z)| Vec3::new(x, y, z))
                        })
                        .collect()
                });
                cache.points.insert(native.id.clone(), points);
            }
            let points = cache.points[&native.id]
                .iter()
                .map(|point| focus.to_render_surface(*point))
                .collect();
            cache.visible.push((native.id.clone(), points));
        }
        cache.retain_visible();
        cache.initialized = true;
    }
    for (id, _) in &cache.visible {
        // A snapshot from before streaming this sector can omit its state.
        // Re-register from cached metadata while paused, without TDB/RDB scans.
        if !live.session.state.crossings.contains_key(id)
            && let Some(Some(definition)) = cache.definitions.get(id)
        {
            if live.session.state.crossings.len() < 512 {
                live.session
                    .state
                    .crossings
                    .insert(id.clone(), CrossingState::new(definition.clone()));
            } else if roads.unresolved.len() < 128 && !roads.unresolved.contains(id) {
                roads.unresolved.push(id.clone());
            }
        }
    }
    // A newly streamed crossing observes occupancy before the first road tick,
    // also while paused. Update once after all registrations, rather than once
    // per object (which repeated every train/crossing scan quadratically).
    let drive = &mut **live;
    drive.traffic.update_crossings(&mut drive.session, 0.);
    roads.stops.clear();
    for (id, points) in &cache.visible {
        let closed = live
            .session
            .state
            .crossings
            .get(id)
            .is_none_or(|s| s.road_closed);
        for point in points {
            roads.stops.push((*point, closed));
        }
    }
}
fn animate(
    live: Option<Res<crate::live::LiveDrive>>,
    mut parts: Query<(
        &CrossingAnimation,
        &mut openrailsrs_bevy_scenery::shapes::ShapeAnimState,
    )>,
) {
    let Some(live) = live else {
        return;
    };
    for (gate, mut animation) in &mut parts {
        animation.key = live.session.state.crossings.get(&gate.id).map_or(0., |s| {
            if gate.looped && gate.frames > 0. && s.demand {
                (-s.elapsed_closed_s as f32 * gate.speed).rem_euclid(gate.frames)
            } else {
                s.fraction as f32 * gate.frames
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_links_survive_frames_but_retry_on_graph_change_and_release_unloaded_points() {
        let mut cache = NativeCrossingRegistry::default();
        assert!(cache.sync_graph(12));
        cache.definitions.insert("missing".into(), None);
        cache
            .points
            .insert("missing".into(), vec![Vec3::new(1., 7., 3.)]);
        cache.points.insert("unloaded".into(), vec![Vec3::ZERO]);
        cache.definitions.insert("unloaded".into(), None);
        cache
            .visible
            .push(("missing".into(), vec![Vec3::new(1., 2., 3.)]));
        cache.initialized = true;
        assert!(!cache.sync_graph(12));
        assert!(cache.definitions.contains_key("missing"));
        cache.retain_visible();
        assert_eq!(cache.points.len(), 1);
        assert!(!cache.definitions.contains_key("unloaded"));
        assert!(cache.sync_graph(14));
        assert!(cache.points.is_empty() && cache.definitions.is_empty());
        assert!(!cache.initialized);
    }
}

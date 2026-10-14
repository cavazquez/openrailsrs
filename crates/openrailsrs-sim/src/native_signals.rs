//! Original SIGSCR programs evaluated on each service's directed signal blocks.
use crate::LiveDriveSession;
use openrailsrs_track::{
    SignalAspect, TrackGraph,
    sigscript::{SignalContext, SignalProgram, activity_call_on, aggregate_signal_aspects},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackOccupancy {
    pub edge: String,
    pub start_m: f64,
    pub end_m: f64,
    pub owner: String,
}
/// Optional version-one save extension. Entries are sorted when captured;
/// explicit vectors let validation reject duplicate IDs instead of silently
/// accepting the last duplicate JSON object key.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeSignalSnapshot {
    pub version: u32,
    pub needs_refresh: bool,
    pub aspects: Vec<(String, u8)>,
    #[serde(default)]
    pub errors: Vec<(String, String)>,
}

#[derive(Default)]
pub struct NativeSignalRuntime {
    programs: HashMap<String, Arc<SignalProgram>>,
    pub aspects: HashMap<String, u8>,
    pub errors: HashMap<String, String>,
    pub(crate) needs_refresh: bool,
}
impl NativeSignalRuntime {
    pub fn from_graph(graph: &TrackGraph) -> Result<Self, String> {
        let mut runtime = Self {
            needs_refresh: true,
            ..Self::default()
        };
        let mut compiled = HashMap::<String, Arc<SignalProgram>>::new();
        for signal in graph.signals() {
            if let Some(native) = signal.script.as_ref().and_then(|s| s.native.as_ref()) {
                if !matches!(
                    native.function.to_ascii_uppercase().as_str(),
                    "NORMAL" | "DISTANCE" | "INFO" | "REPEATER" | "SHUNTING" | "UNKNOWN"
                ) {
                    return Err(format!(
                        "{}: unsupported signal function {}",
                        native.name, native.function
                    ));
                }
                let program = if let Some(program) = compiled.get(&native.source) {
                    Arc::clone(program)
                } else {
                    let program = SignalProgram::compile_cached(&native.source)
                        .map_err(|e| format!("{}: {e}", native.name))?;
                    // Validate all aspects used by the route, not just an easy clear branch.
                    for next in 0..8 {
                        for clear in [false, true] {
                            for enabled in [false, true] {
                                for route_set in [false, true] {
                                    program
                                        .evaluate(SignalContext {
                                            enabled,
                                            route_set,
                                            next_normal: next,
                                            distant_normal: next,
                                            block_clear: clear,
                                            ..Default::default()
                                        })
                                        .map_err(|e| format!("{}: {e}", native.name))?;
                                }
                            }
                        }
                    }
                    let program = Arc::new(program);
                    compiled.insert(native.source.clone(), Arc::clone(&program));
                    program
                };
                runtime.programs.insert(signal.id.clone(), program);
            }
        }
        Ok(runtime)
    }
    pub fn count(&self) -> usize {
        self.programs.len()
    }

    pub fn snapshot(&self) -> NativeSignalSnapshot {
        // The pending-refresh flag also schedules generic aspects on routes
        // without native programs, so an empty aspect list still has state.
        let mut aspects: Vec<_> = self
            .aspects
            .iter()
            .map(|(id, aspect)| (id.clone(), *aspect))
            .collect();
        aspects.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let mut errors: Vec<_> = self
            .errors
            .iter()
            .map(|(id, error)| (id.clone(), error.clone()))
            .collect();
        errors.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        NativeSignalSnapshot {
            version: 1,
            needs_refresh: self.needs_refresh,
            aspects,
            errors,
        }
    }

    pub(crate) fn validate_snapshot(
        &self,
        saved: &NativeSignalSnapshot,
        graph: &TrackGraph,
        path_edges: &[String],
        signal_runtime: &HashMap<String, SignalAspect>,
        overrides: &HashMap<String, SignalAspect>,
    ) -> Result<(), String> {
        if saved.version != 1
            || saved.aspects.len() > self.programs.len()
            || saved.errors.len() > saved.aspects.len()
        {
            return Err("Versión o tamaño de caché SIGSCR guardada inválidos".into());
        }
        let expected: HashSet<_> = path_edges
            .iter()
            .flat_map(|edge| graph.signals_on_edge(edge))
            .filter(|signal| self.programs.contains_key(&signal.id))
            .map(|signal| signal.id.as_str())
            .collect();
        let mut seen = HashSet::new();
        for (id, aspect) in &saved.aspects {
            if !expected.contains(id.as_str()) || !seen.insert(id.as_str()) || *aspect > 7 {
                return Err("Señal, duplicado o aspecto de caché SIGSCR guardada inválidos".into());
            }
            if !saved.needs_refresh {
                let native = graph
                    .signal(id)
                    .and_then(|signal| signal.script.as_ref())
                    .and_then(|script| script.native.as_ref())
                    .ok_or("Programa SIGSCR guardado no disponible")?;
                if native.forced_stop && *aspect != 0 {
                    return Err("Caché SIGSCR intenta liberar una parada forzada".into());
                }
                let info = matches!(
                    native.function.to_ascii_uppercase().as_str(),
                    "INFO" | "REPEATER" | "SHUNTING" | "UNKNOWN"
                );
                let normal = native.function.eq_ignore_ascii_case("NORMAL");
                let mut safety = match aspect {
                    0..=1 => SignalAspect::Stop,
                    2..=5 => SignalAspect::Caution,
                    _ => SignalAspect::Clear,
                };
                if overrides.get(id) == Some(&SignalAspect::Stop) && *aspect != 0
                    || overrides.get(id) == Some(&SignalAspect::Caution) && *aspect > 5
                {
                    return Err("Caché SIGSCR contradice una restricción guardada".into());
                }
                if overrides.get(id) == Some(&SignalAspect::Stop) {
                    safety = SignalAspect::Stop;
                }
                if overrides.get(id) == Some(&SignalAspect::Caution)
                    && safety == SignalAspect::Clear
                {
                    safety = SignalAspect::Caution;
                }
                if info {
                    safety = SignalAspect::Clear;
                } else if !normal && safety == SignalAspect::Stop {
                    safety = SignalAspect::Caution;
                }
                let actual = signal_runtime
                    .get(id)
                    .copied()
                    .ok_or("Aspecto de seguridad SIGSCR guardado ausente")?;
                // STOP_AND_PROCEED can be Caution for an enabled activity call-on.
                if actual != safety
                    && !(*aspect == 1
                        && normal
                        && overrides.get(id) != Some(&SignalAspect::Stop)
                        && actual == SignalAspect::Caution)
                {
                    return Err("Aspectos nativo y de seguridad guardados incompatibles".into());
                }
            }
        }
        if seen != expected {
            return Err("Faltan aspectos del itinerario en la caché SIGSCR guardada".into());
        }
        let mut errors_seen = HashSet::new();
        for (id, error) in &saved.errors {
            if !seen.contains(id.as_str())
                || !errors_seen.insert(id.as_str())
                || error.is_empty()
                || error.len() > 4096
                || saved
                    .aspects
                    .iter()
                    .find(|(signal, _)| signal == id)
                    .is_none_or(|(_, aspect)| *aspect != 0)
            {
                return Err("Error de caché SIGSCR guardada inválido".into());
            }
        }
        Ok(())
    }

    pub(crate) fn restore_validated_snapshot(&mut self, saved: NativeSignalSnapshot) {
        self.aspects = saved.aspects.into_iter().collect();
        self.errors = saved.errors.into_iter().collect();
        self.needs_refresh = saved.needs_refresh;
    }
}
impl LiveDriveSession {
    pub fn own_track_occupancy(&self) -> Vec<TrackOccupancy> {
        let mut intervals = vec![];
        for (i, car) in self.formation.cars.iter().enumerate() {
            let head = if i < self.formation.coupled_count {
                self.head_chainage_m()
            } else {
                self.formation
                    .parked_head_chainage_m
                    .unwrap_or(self.head_chainage_m())
            };
            let offset = self.formation.center_offset_m(i);
            let center = head
                + if i < self.formation.coupled_count {
                    self.oriented_car_offset(offset)
                } else {
                    self.parked_car_offset(offset)
                };
            let rear = (center - car.length_m * 0.5).max(0.0);
            let front = center + car.length_m * 0.5;
            let mut start = 0.0;
            let (edges, pd) = self.presentation_path_for_car(offset);
            for (edge, data) in edges.iter().zip(&pd.edges) {
                let end = start + data.length_m;
                if end >= rear && start <= front {
                    let mut a = (rear - start).max(0.0);
                    let mut b = (front - start).min(data.length_m);
                    if edge.ends_with("_r") {
                        (a, b) = (data.length_m - b, data.length_m - a);
                    }
                    intervals.push(TrackOccupancy {
                        edge: edge.strip_suffix("_r").unwrap_or(edge).into(),
                        start_m: a,
                        end_m: b,
                        owner: if i < self.formation.coupled_count {
                            self.service_id.clone()
                        } else {
                            format!("{} · estacionado", self.service_id)
                        },
                    });
                }
                start = end;
            }
        }
        intervals
    }
    pub(crate) fn evaluate_native_signals(&mut self) {
        self.native_signals.needs_refresh = false;
        if self.native_signals.count() == 0 {
            return;
        }
        let mut start = 0.0;
        let mut heads = vec![];
        for (index, edge) in self.state.path_edges.iter().enumerate() {
            for signal in self.graph.signals_on_edge(edge) {
                if let Some(native) = signal.script.as_ref().and_then(|s| s.native.as_ref()) {
                    heads.push((
                        start + signal.position_m,
                        index,
                        signal.id.clone(),
                        native.function.eq_ignore_ascii_case("NORMAL"),
                    ));
                }
            }
            start += self.path_data.edges[index].length_m;
        }
        heads.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| b.3.cmp(&a.3)));
        let first_normal_position = heads
            .iter()
            .find(|h| h.3 && h.0 + 1.0 >= self.head_chainage_m())
            .map(|h| h.0);
        let mut footprint = self.external_track_occupancy.clone();
        footprint.extend(self.external_track_reservations.iter().cloned());
        footprint.extend(
            self.own_track_occupancy()
                .into_iter()
                .filter(|i| i.owner != self.service_id),
        );
        let occupied = |from: f64, to: f64| {
            let mut start = 0.0;
            for (edge, data) in self.state.path_edges.iter().zip(&self.path_data.edges) {
                let end = start + data.length_m;
                if end > from && start < to {
                    let mut a = (from - start).max(0.0);
                    let mut b = (to - start).min(data.length_m);
                    if edge.ends_with("_r") {
                        (a, b) = (data.length_m - b, data.length_m - a);
                    }
                    let base = edge.strip_suffix("_r").unwrap_or(edge);
                    if footprint
                        .iter()
                        .any(|i| i.edge == base && i.end_m > a + 0.01 && i.start_m < b - 0.01)
                    {
                        return true;
                    }
                    // External callers without positional footprints still restrict authority.
                    if !footprint.iter().any(|i| i.edge == base)
                        && [base.to_owned(), format!("{base}_r")]
                            .iter()
                            .any(|e| self.external_occupancy.contains_key(e))
                    {
                        return true;
                    }
                }
                start = end;
            }
            false
        };
        let mut states = HashMap::new();
        let mut errors = HashMap::new();
        // Normal heads first, downstream to upstream. Distant heads then inspect
        // all normal heads up to the next distant, as OR's DIST_MULTI_SIG_MR does.
        for normal in [true, false] {
            for (position, index, id, is_normal) in heads.iter().rev().filter(|h| h.3 == normal) {
                let next = heads.iter().find(|h| h.3 && h.0 > *position + 0.1);
                let next_position = next.map_or(start, |h| h.0);
                let next_aspect = next.and_then(|h| states.get(&h.2).copied()).unwrap_or(0);
                let end_distant = heads
                    .iter()
                    .find(|h| {
                        !h.3 && h.0 > *position + 0.1
                            && self
                                .graph
                                .signal(&h.2)
                                .and_then(|s| s.script.as_ref())
                                .and_then(|s| s.native.as_ref())
                                .is_some_and(|s| s.function.eq_ignore_ascii_case("DISTANCE"))
                    })
                    .map_or(start, |h| h.0);
                let distant = heads
                    .iter()
                    .filter(|h| h.3 && h.0 > *position + 0.1 && h.0 <= end_distant + 0.1)
                    .filter_map(|h| states.get(&h.2).copied())
                    .min()
                    .unwrap_or(0);
                let this = heads
                    .iter()
                    .find(|h| h.3 && (h.0 - position).abs() < 3.0)
                    .and_then(|h| states.get(&h.2).copied())
                    .unwrap_or(next_aspect);
                let route_set = (*index..next.map_or(*index, |h| h.1)).all(|i| {
                    let edge = self.graph.edge(&self.state.path_edges[i]).unwrap();
                    match self.graph.node(&edge.to.0).map(|n| &n.kind) {
                        Some(openrailsrs_track::NodeKind::Switch {
                            stem_edge,
                            diverging_edge,
                        }) => {
                            let desired =
                                match self.graph.switch_position(&edge.to.0).unwrap_or_default() {
                                    openrailsrs_track::SwitchPosition::Straight => stem_edge,
                                    openrailsrs_track::SwitchPosition::Diverging => diverging_edge,
                                };
                            let Some(next) = self.state.path_edges.get(i + 1) else {
                                return true;
                            };
                            if next == &stem_edge.0 || next == &diverging_edge.0 {
                                desired.0 == *next
                            } else {
                                // A trailing traversal must enter from the selected
                                // branch before leaving through the common pin.
                                let incoming = &self.state.path_edges[i];
                                let reverse = incoming
                                    .strip_suffix("_r")
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| format!("{incoming}_r"));
                                desired.0 == reverse
                            }
                        }
                        _ => true,
                    }
                });
                let native = self
                    .graph
                    .signal(id)
                    .unwrap()
                    .script
                    .as_ref()
                    .unwrap()
                    .native
                    .as_ref()
                    .unwrap();
                let same_group = |other: &str| {
                    other == id
                        || self
                            .graph
                            .signal(other)
                            .and_then(|s| s.script.as_ref())
                            .and_then(|s| s.native.as_ref())
                            .is_some_and(|other| {
                                native.group.is_some() && native.group == other.group
                            })
                };
                let group_aspect = |function: &str| {
                    heads
                        .iter()
                        .filter(|h| {
                            same_group(&h.2)
                                && self
                                    .graph
                                    .signal(&h.2)
                                    .unwrap()
                                    .script
                                    .as_ref()
                                    .unwrap()
                                    .native
                                    .as_ref()
                                    .unwrap()
                                    .function
                                    .eq_ignore_ascii_case(function)
                        })
                        .filter_map(|h| {
                            states
                                .get(&h.2)
                                .or_else(|| self.native_signals.aspects.get(&h.2))
                                .copied()
                        })
                        .max()
                        .unwrap_or(0)
                };
                let next_function_state = |function: &str, restrictive: bool| {
                    let first = heads.iter().find(|h| {
                        h.0 > *position + 0.1
                            && !same_group(&h.2)
                            && self
                                .graph
                                .signal(&h.2)
                                .unwrap()
                                .script
                                .as_ref()
                                .unwrap()
                                .native
                                .as_ref()
                                .unwrap()
                                .function
                                .eq_ignore_ascii_case(function)
                    });
                    let Some(first) = first else {
                        return 0;
                    };
                    let group = self
                        .graph
                        .signal(&first.2)
                        .and_then(|s| s.script.as_ref())
                        .and_then(|s| s.native.as_ref())
                        .and_then(|s| s.group.as_ref());
                    let values = heads
                        .iter()
                        .filter(|h| {
                            let Some(other) = self
                                .graph
                                .signal(&h.2)
                                .and_then(|s| s.script.as_ref())
                                .and_then(|s| s.native.as_ref())
                            else {
                                return false;
                            };
                            other.function.eq_ignore_ascii_case(function)
                                && (h.2 == first.2
                                    || group
                                        .is_some_and(|group| other.group.as_ref() == Some(group)))
                        })
                        .filter_map(|h| {
                            states
                                .get(&h.2)
                                .or_else(|| self.native_signals.aspects.get(&h.2))
                                .copied()
                        });
                    aggregate_signal_aspects(values.map(|aspect| (0, aspect)), 0, restrictive)
                };
                let functions = ["NORMAL", "DISTANCE", "REPEATER", "SHUNTING", "INFO"];
                let next_mr = functions.map(|function| next_function_state(function, true));
                let this_mr = functions.map(|function| {
                    heads
                        .iter()
                        .filter(|h| {
                            same_group(&h.2)
                                && self
                                    .graph
                                    .signal(&h.2)
                                    .and_then(|s| s.script.as_ref())
                                    .and_then(|s| s.native.as_ref())
                                    .is_some_and(|s| s.function.eq_ignore_ascii_case(function))
                        })
                        .filter_map(|h| {
                            states
                                .get(&h.2)
                                .or_else(|| self.native_signals.aspects.get(&h.2))
                                .copied()
                        })
                        .min()
                        .unwrap_or(0)
                });
                let mut prefix = 0.0;
                let mut into_platform = false;
                for (edge, data) in self.state.path_edges.iter().zip(&self.path_data.edges) {
                    if let Some(profile) = self.graph.physics_profile(edge) {
                        into_platform |= profile.platforms.iter().any(|p| {
                            prefix + p.end_m > *position && prefix + p.start_m < next_position
                        });
                    }
                    prefix += data.length_m;
                    if prefix >= next_position {
                        break;
                    }
                }
                let enabled = *position + 1.0 >= self.head_chainage_m()
                    && !(self.dispatcher.protected_signal.as_ref() == Some(id)
                        && !self.dispatcher.waiting_for.is_empty());
                let first_signal =
                    first_normal_position.is_some_and(|first| (*position - first).abs() < 3.0);
                let switch_diverging = self.state.path_edges[*index..]
                    .iter()
                    .filter_map(|edge| self.graph.edge(edge))
                    .find_map(|edge| {
                        let node = self.graph.node(&edge.from.0)?;
                        let openrailsrs_track::NodeKind::Switch {
                            stem_edge,
                            diverging_edge,
                        } = &node.kind
                        else {
                            return None;
                        };
                        if edge.id.0 != stem_edge.0 && edge.id.0 != diverging_edge.0 {
                            return None;
                        }
                        Some(
                            self.graph.switch_position(&edge.from.0)
                                == Some(openrailsrs_track::SwitchPosition::Diverging),
                        )
                    })
                    .unwrap_or(false);
                let c = SignalContext {
                    enabled,
                    draw_states: native
                        .draw_states
                        .unwrap_or(SignalContext::default().draw_states),
                    route_set,
                    block_clear: !occupied(*position, next_position)
                        && !(self.dispatcher.protected_signal.as_ref() == Some(id)
                            && !self.dispatcher.waiting_for.is_empty()),
                    next_normal: next_function_state("NORMAL", false),
                    distant_normal: distant,
                    distant_info: heads
                        .iter()
                        .filter(|h| h.0 > *position + 0.1 && h.0 <= next_position + 0.1)
                        .filter(|h| {
                            self.graph
                                .signal(&h.2)
                                .unwrap()
                                .script
                                .as_ref()
                                .unwrap()
                                .native
                                .as_ref()
                                .unwrap()
                                .function
                                .eq_ignore_ascii_case("INFO")
                        })
                        .filter_map(|h| {
                            states
                                .get(&h.2)
                                .or_else(|| self.native_signals.aspects.get(&h.2))
                                .copied()
                        })
                        .min()
                        .unwrap_or(0),
                    this_normal: this,
                    feature_flags: native.feature_flags,
                    head_flags: native.head_flags,
                    next_mr,
                    this_mr,
                    switch_diverging,
                    call_on: activity_call_on(
                        enabled,
                        first_signal,
                        route_set,
                        into_platform,
                        false,
                        true,
                    ),
                    call_on_restricted: activity_call_on(
                        enabled,
                        first_signal,
                        route_set,
                        into_platform,
                        false,
                        false,
                    ),
                    this_shunting: group_aspect("SHUNTING"),
                    this_info: group_aspect("INFO"),
                    next_info: next_function_state("INFO", false),
                    next_distance: next_function_state("DISTANCE", false),
                    next_repeater: next_function_state("REPEATER", false),
                    next_shunting: next_function_state("SHUNTING", false),
                    this_distance: group_aspect("DISTANCE"),
                    this_repeater: group_aspect("REPEATER"),
                };
                let aspect = match self.native_signals.programs[id].evaluate(c) {
                    Ok(r) => r.aspect,
                    Err(e) => {
                        errors.insert(id.clone(), e);
                        0
                    }
                };
                // Restrict the native state before evaluating upstream heads.
                // Otherwise NEXT_SIG_LR sees Clear behind a dispatcher Stop.
                let aspect = match self.signal_overrides.get(id) {
                    _ if native.forced_stop => 0,
                    Some(SignalAspect::Stop) => 0,
                    Some(SignalAspect::Caution) if aspect > 5 => 3,
                    _ => aspect,
                };
                states.insert(id.clone(), aspect);
                let safety = match aspect {
                    1 if c.call_on => SignalAspect::Caution,
                    0..=1 => SignalAspect::Stop,
                    2..=5 => SignalAspect::Caution,
                    _ => SignalAspect::Clear,
                };
                let safety = match self.signal_overrides.get(id) {
                    Some(SignalAspect::Stop) => SignalAspect::Stop,
                    Some(SignalAspect::Caution) if safety == SignalAspect::Clear => {
                        SignalAspect::Caution
                    }
                    _ => safety,
                };
                // A distant head warns; it never creates a second stop authority.
                let info = self
                    .graph
                    .signal(id)
                    .and_then(|s| s.script.as_ref())
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|s| {
                        matches!(
                            s.function.to_ascii_uppercase().as_str(),
                            "INFO" | "REPEATER" | "SHUNTING" | "UNKNOWN"
                        )
                    });
                self.signal_runtime.insert(
                    id.clone(),
                    if info {
                        SignalAspect::Clear
                    } else if !is_normal && safety == SignalAspect::Stop {
                        SignalAspect::Caution
                    } else {
                        safety
                    },
                );
            }
        }
        self.native_signals.aspects = states;
        self.native_signals.errors = errors;
    }
    /// Older version-one saves did not record the native cache or its pending
    /// refresh. Rebuild against all restored footprints, preserve the signal
    /// cadence and conservatively request refresh at the next traffic update.
    pub fn rebuild_legacy_native_signal_cache(&mut self) {
        self.evaluate_native_signals();
        self.native_signals.needs_refresh = true;
    }

    pub fn native_signal_aspect(&self, id: &str) -> Option<u8> {
        let id = if self.native_signals.aspects.contains_key(id) {
            id.to_owned()
        } else {
            format!("{id}_r")
        };
        let aspect = *self.native_signals.aspects.get(&id)?;
        match self.signal_overrides.get(&id) {
            Some(SignalAspect::Stop) => Some(0),
            Some(SignalAspect::Caution) if aspect > 5 => Some(3),
            _ => Some(aspect),
        }
    }
}

#[cfg(test)]
mod tests {
    fn native_fixture(specs: &[(&str, f64, &str, &str, &str)]) -> LiveDriveSession {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/smoke");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
        let mut session = LiveDriveSession::from_scenario(&directory, &scenario).unwrap();
        for &(id, position, group, function, source) in specs {
            let native=serde_json::from_value(serde_json::json!({"name":id,"source":source,"function":function,"group":group,"head_flags":8})).unwrap();
            session
                .graph
                .insert_signal(openrailsrs_track::TrackSignal {
                    id: id.into(),
                    edge_id: "e1".into(),
                    position_m: position,
                    aspect: SignalAspect::Stop,
                    clear_after_s: None,
                    script: Some(openrailsrs_track::SignalScript {
                        native: Some(native),
                        ..Default::default()
                    }),
                })
                .unwrap();
        }
        session.native_signals = NativeSignalRuntime::from_graph(&session.graph).unwrap();
        session.evaluate_native_signals();
        session
    }
    #[test]
    fn native_multi_head_most_restrictive_aspect_uses_the_physical_group() {
        let session = native_fixture(&[
            (
                "upstream",
                100.0,
                "up",
                "NORMAL",
                "state=NEXT_SIG_MR(SIGFN_NORMAL);",
            ),
            ("green", 500.0, "next", "NORMAL", "state=SIGASP_CLEAR_2;"),
            (
                "warning",
                501.0,
                "next",
                "NORMAL",
                "state=SIGASP_APPROACH_1;",
            ),
            (
                "other-function",
                500.0,
                "next",
                "INFO",
                "state=SIGASP_STOP;",
            ),
        ]);
        assert_eq!(session.native_signal_aspect("upstream"), Some(3));
        assert_eq!(
            session.signal_runtime["other-function"],
            SignalAspect::Clear
        );
        assert!(session.native_signals.errors.is_empty());
    }
    #[test]
    fn call_on_is_limited_to_the_first_signal_and_not_granted_for_a_platform_or_reserved_route() {
        let source = "if (TRAINHASCALLON()) state=SIGASP_STOP_AND_PROCEED; else state=SIGASP_STOP;";
        let mut session = native_fixture(&[
            ("first", 100.0, "first", "NORMAL", source),
            ("later", 500.0, "later", "NORMAL", source),
        ]);
        session.state.pos_on_edge_m = 80.0;
        session.external_track_occupancy = vec![TrackOccupancy {
            edge: "e1".into(),
            start_m: 200.0,
            end_m: 250.0,
            owner: "Other".into(),
        }];
        session.evaluate_native_signals();
        assert_eq!(session.native_signal_aspect("first"), Some(1));
        assert_eq!(session.native_signal_aspect("later"), Some(0));
        assert_eq!(session.signal_runtime["first"], SignalAspect::Caution);
        assert_eq!(session.signal_runtime["later"], SignalAspect::Stop);
        let mut profile = session
            .graph
            .physics_profile("e1")
            .cloned()
            .unwrap_or_default();
        profile.platforms.push(openrailsrs_track::PositionPlatform {
            start_m: 200.0,
            end_m: 300.0,
            name: "Original platform".into(),
        });
        session.graph.set_physics_profile("e1", profile);
        session.evaluate_native_signals();
        assert_eq!(session.native_signal_aspect("first"), Some(0));
        session.graph.set_physics_profile("e1", Default::default());
        session.dispatcher.protected_signal = Some("first".into());
        session.dispatcher.waiting_for = vec!["Other".into()];
        session.evaluate_native_signals();
        assert_eq!(session.native_signal_aspect("first"), Some(0));
    }
    use super::*;
    use std::path::Path;
    fn extended() -> (LiveDriveSession, crate::LiveTraffic) {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern_extended");
        let scenario =
            openrailsrs_scenarios::load_scenario(directory.join("scenario.toml")).unwrap();
        (
            LiveDriveSession::from_scenario(&directory, &scenario).unwrap(),
            crate::LiveTraffic::from_scenario(&directory, &scenario).unwrap(),
        )
    }
    #[test]
    fn dispatcher_stop_propagates_to_the_upstream_native_program() {
        let (mut session, _) = extended();
        let before = session.native_signals.aspects.clone();
        let targets: Vec<_> = session
            .graph
            .signals()
            .filter(|s| {
                s.script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|n| n.function.eq_ignore_ascii_case("NORMAL"))
                    && before.get(&s.id).is_some_and(|a| *a > 5)
            })
            .map(|s| s.id.clone())
            .collect();
        let mut propagated = false;
        for id in targets {
            session.signal_overrides.clear();
            session
                .signal_overrides
                .insert(id.clone(), SignalAspect::Stop);
            session.evaluate_native_signals();
            assert_eq!(session.native_signals.aspects[&id], 0);
            propagated |= session.native_signals.aspects.iter().any(|(upstream, a)| {
                upstream != &id
                    && before.get(upstream).is_some_and(|b| *b > 5)
                    && (3..=5).contains(a)
            });
        }
        assert!(
            propagated,
            "downstream Stop must produce an upstream native warning"
        );
    }
    #[test]
    fn native_counterbalance_keeps_its_visual_state_without_creating_train_authority() {
        let (mut session, _) = extended();
        let info = session
            .graph
            .signals()
            .find(|s| {
                s.script
                    .as_ref()
                    .and_then(|s| s.native.as_ref())
                    .is_some_and(|n| n.function.eq_ignore_ascii_case("INFO"))
            })
            .unwrap()
            .id
            .clone();
        session
            .signal_overrides
            .insert(info.clone(), SignalAspect::Stop);
        session.evaluate_native_signals();
        assert_eq!(session.native_signals.aspects[&info], 0);
        assert_eq!(session.signal_runtime[&info], SignalAspect::Clear);
    }
    #[test]
    fn original_chiltern_scripts_run_through_all_six_stations_with_live_traffic() {
        let (mut player, mut traffic) = extended();
        assert!(player.native_signals.count() > 20);
        let mut saw_stop = false;
        let mut saw_warning = false;
        for _ in 0..2600 {
            traffic.advance(&mut player, 1.0, Some(0.75), |_| {});
            saw_stop |= player.native_signals.aspects.values().any(|a| *a == 0);
            saw_warning |= player
                .native_signals
                .aspects
                .values()
                .any(|a| (3..=5).contains(a));
            assert!(
                player.native_signals.errors.is_empty(),
                "{:?}",
                player.native_signals.errors
            );
            for train in &traffic.services {
                assert!(train.session.native_signals.errors.is_empty());
            }
            if player.arrived {
                break;
            }
        }
        assert!(saw_stop && saw_warning);
        assert!(
            player.arrived,
            "{}m {:?} {:?}",
            player.head_chainage_m(),
            player.gameplay.phase,
            player.gameplay.failure
        );
        assert_eq!(player.gameplay.stop_results.len(), 6);
        assert!(traffic.services.iter().all(|t| t.session.arrived));
    }
    #[test]
    fn restored_native_aspects_match_player_and_traffic_without_advancing_the_clock() {
        let (mut player, mut traffic) = extended();
        assert!(!player.native_signals.aspects.is_empty());
        assert_eq!(player.state.time_s(), 0.0);
        traffic.advance(&mut player, 800.0, Some(0.75), |_| {});
        player.native_signals.needs_refresh = true;
        for service in &mut traffic.services {
            service.session.native_signals.needs_refresh = true;
        }
        traffic.synchronize_occupancy(&mut player);
        let (mut restored, mut restored_traffic) = extended();
        restored.restore_snapshot(player.snapshot()).unwrap();
        restored_traffic
            .restore_snapshot(traffic.snapshot())
            .unwrap();
        restored_traffic.synchronize_occupancy(&mut restored);
        assert_eq!(restored.state.time_s(), player.state.time_s());
        assert!(!restored.native_signals.aspects.is_empty());
        assert_eq!(
            restored.native_signals.aspects,
            player.native_signals.aspects
        );
        assert!(restored.native_signals.errors.is_empty());
        for (a, b) in traffic.services.iter().zip(&restored_traffic.services) {
            assert_eq!(
                a.session.native_signals.aspects,
                b.session.native_signals.aspects
            );
            assert!(b.session.native_signals.errors.is_empty());
        }
    }
    #[test]
    fn original_home_red_is_not_cleared_by_dispatch_over_occupied_block() {
        let (mut player, mut traffic) = extended();
        traffic.advance(&mut player, 0.05, Some(0.75), |_| {});
        let red = player
            .native_signals
            .aspects
            .iter()
            .find(|(id, a)| {
                **a == 0
                    && player
                        .graph
                        .signal(id)
                        .unwrap()
                        .script
                        .as_ref()
                        .unwrap()
                        .native
                        .as_ref()
                        .unwrap()
                        .function
                        == "NORMAL"
                    && player
                        .state
                        .path_edges
                        .contains(&player.graph.signal(id).unwrap().edge_id)
            })
            .map(|(id, _)| id.clone())
            .unwrap();
        player
            .signal_overrides
            .insert(red.clone(), SignalAspect::Clear);
        player.refresh_traffic_signals();
        assert_eq!(player.native_signal_aspect(&red), Some(0));
    }
    #[test]
    fn cab_brake_gauge_uses_the_lead_vehicle_native_pressure() {
        let (player, _) = extended();
        assert!((player.cab_telemetry().brake_cyl_bar - 45.0 * 0.0689475729).abs() < 1e-6);
        assert!(
            player.state.brake_system.cylinders[1].full_pressure_bar
                > player.cab_telemetry().brake_cyl_bar
        );
    }
}

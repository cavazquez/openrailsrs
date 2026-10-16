//! Inventory and evaluate native route programs/links without a renderer.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 2 {
        return Err("signal_operational_audit ROUTES.json OUTPUT_DIRECTORY".into());
    }
    let routes: BTreeMap<String, PathBuf> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    std::fs::create_dir_all(&args[1])?;
    let mut results = vec![];
    let mut failed = false;
    for (name, route) in routes {
        let dir = args[1].join(&name);
        std::fs::create_dir_all(&dir)?;
        let text = match openrailsrs_msts::import_route(&route) {
            Ok(text) => text,
            Err(error) => {
                failed = true;
                results.push(
                    serde_json::json!({"route":name,"passed":false,"error":error.to_string()}),
                );
                continue;
            }
        };
        std::fs::write(dir.join("track.toml"), text)?;
        let graph = openrailsrs_route::load::load_track_graph_from_route_dir(&dir)?;
        let mut programs = BTreeSet::new();
        let mut links = vec![];
        let mut contexts = 0;
        let mut functions = BTreeMap::<String, usize>::new();
        for signal in graph.signals() {
            let Some(native) = signal.script.as_ref().and_then(|s| s.native.as_ref()) else {
                continue;
            };
            *functions.entry(native.function.clone()).or_default() += 1;
            if let Some(link) = &native.route_link {
                if graph.node(&link.junction_node).is_none()
                    || graph.edge(&link.required_vector).is_none()
                {
                    return Err(
                        format!("{name}: missing native link target on {}", signal.id).into(),
                    );
                }
                links.push(serde_json::json!({"signal":signal.id,"group":native.group,"junction":link.junction_node,"required_vector":link.required_vector,"junction_path":link.junction_path}));
            }
            if !programs.insert(native.name.clone()) {
                continue;
            }
            let program = openrailsrs_track::sigscript::SignalProgram::compile(&native.source)?;
            for obstruction in 0..3 {
                for enabled in [false, true] {
                    for route_set in [false, true] {
                        for next in 0..8 {
                            let context = openrailsrs_track::sigscript::SignalContext {
                                enabled,
                                route_set,
                                block_clear: obstruction == 0,
                                junction_obstructed: obstruction == 2,
                                next_normal: next,
                                next_mr: [next, 0, 0, 0, 0],
                                head_flags: native.head_flags,
                                feature_flags: native.feature_flags,
                                draw_states: native.draw_states.unwrap_or(
                                    openrailsrs_track::sigscript::SignalContext::default()
                                        .draw_states,
                                ),
                                ..Default::default()
                            };
                            program
                                .evaluate(context)
                                .map_err(|e| format!("{name}/{}: {e}", native.name))?;
                            contexts += 1;
                        }
                    }
                }
            }
        }
        results.push(serde_json::json!({"route":name,"passed":true,"heads":functions.values().sum::<usize>(),"functions":functions,"unique_programs":programs.len(),"contexts_evaluated":contexts,"links":links,"scope":"Original compiled programs with explicit native inputs; route import and linkage, not certification of a full operational trip"}));
    }
    let receipt =
        serde_json::json!({"reference":"Open Rails 1.6.1","routes":results,"passed":!failed});
    std::fs::write(
        args[1].join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "{}",
        serde_json::json!({"routes":results.len(),"passed":!failed})
    );
    if failed {
        return Err("Unsupported route signalling; see receipt".into());
    }
    Ok(())
}

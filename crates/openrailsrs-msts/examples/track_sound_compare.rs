//! Compare native WORLD sound selection with a read-only original route.
use std::{collections::BTreeMap, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.len() != 3 {
        return Err("track_sound_compare ROUTE REFERENCE.json OUTPUT_DIRECTORY".into());
    }
    std::fs::create_dir_all(&args[2])?;
    let text = openrailsrs_msts::import_route(&args[0])?;
    std::fs::write(args[2].join("track.toml"), text)?;
    let graph = openrailsrs_route::load::load_track_graph_from_route_dir(&args[2])?;
    let reference: serde_json::Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let mut mismatches = vec![];
    let mut counts = BTreeMap::<String, usize>::new();
    let rows = reference["rows"].as_array().ok_or("Missing native rows")?;
    for (index, row) in rows.iter().enumerate() {
        let path: Vec<String> = serde_json::from_value(row["path"].clone())?;
        let actual = graph.track_sound_type_on_path(
            &path,
            row["edge"].as_str().ok_or("edge")?,
            row["position_m"].as_f64().ok_or("position")?,
            row["reverse"].as_bool().ok_or("direction")?,
        );
        let expected = row["result"].as_u64().map(|t| t as u32);
        *counts.entry(format!("{actual:?}")).or_default() += 1;
        if actual != expected {
            mismatches.push(
                serde_json::json!({"index":index,"expected":expected,"actual":actual,"input":row}),
            );
        }
    }
    let receipt = serde_json::json!({"scope":"Exact WORLD selection against real WorldSounds.GetTType; the original route remains outside git","rows":rows.len(),"types":counts,"mismatches":mismatches,"passed":mismatches.is_empty()});
    std::fs::write(
        args[2].join("comparison.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "{}",
        serde_json::json!({"rows":rows.len(),"mismatches":mismatches.len()})
    );
    if !mismatches.is_empty() {
        return Err("Native track sound type mismatch".into());
    }
    Ok(())
}

//! Link authored SIGCFG/SIGSCR and WORLD features to directed imported signals.
//! Unsupported programs remain an explicit import error, never a green fallback.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use openrailsrs_formats::{
    Ast, Atom, WorldFile, WorldItem, parse_named_stf, read_msts_file_to_string,
    resolve_path_case_insensitive, scan_world_tile_files,
};
use openrailsrs_track::{
    SignalScript,
    sigscript::{NativeSignalDef, SignalProgram},
};

use crate::MstsError;

const MAX_NATIVE_FILE_BYTES: u64 = 16 * 1024 * 1024;
// A whole route's TDB is larger than one script or WORLD tile: Chiltern v4
// has a 24 MiB TDB. Keep the smaller budget for individual signalling files.
const MAX_TRACK_DB_BYTES: u64 = 64 * 1024 * 1024;
const FEATURE_TYPES: [&str; 10] = [
    "DECOR",
    "SIGNAL_HEAD",
    "DUMMY1",
    "DUMMY2",
    "NUMBER_PLATE",
    "GRADIENT_PLATE",
    "USER1",
    "USER2",
    "USER3",
    "USER4",
];

fn blocks<'a>(ast: &'a Ast, key: &str) -> Vec<&'a Ast> {
    fn collect<'a>(ast: &'a Ast, key: &str, out: &mut Vec<&'a Ast>) {
        if let Ast::List(items) = ast {
            if matches!(items.first(), Some(Ast::Atom(Atom::Symbol(name))) if name.eq_ignore_ascii_case(key))
            {
                out.push(ast);
            }
            for item in items {
                collect(item, key, out);
            }
        }
    }
    let mut out = Vec::new();
    collect(ast, key, &mut out);
    out
}
fn atoms(ast: &Ast) -> Vec<String> {
    let Ast::List(items) = ast else {
        return Vec::new();
    };
    items
        .iter()
        .skip(1)
        .filter_map(|item| match item {
            Ast::Atom(Atom::String(s) | Atom::Symbol(s)) => Some(s.clone()),
            Ast::Atom(Atom::Integer(n)) => Some(n.to_string()),
            Ast::Atom(Atom::Number(n)) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}
fn field(ast: &Ast, key: &str) -> Option<String> {
    blocks(ast, key)
        .first()
        .and_then(|value| atoms(value).first().cloned())
}
fn bounded_text(path: &Path, maximum_bytes: u64) -> Result<String, MstsError> {
    if std::fs::metadata(path)?.len() > maximum_bytes {
        return Err(MstsError::msg(format!(
            "Native signalling file exceeds {} MiB: {}",
            maximum_bytes / (1024 * 1024),
            path.display()
        )));
    }
    Ok(read_msts_file_to_string(path)?)
}
fn script_path(root: &Path, base: &Path, name: &str) -> Result<PathBuf, MstsError> {
    let name = name.replace('\\', "/");
    if name.is_empty()
        || Path::new(&name)
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(MstsError::msg(format!(
            "Invalid route-relative signal script: {name}"
        )));
    }
    let path = resolve_path_case_insensitive(&base.join(&name))
        .ok_or_else(|| MstsError::msg(format!("Missing original signal script {name}")))?
        .canonicalize()?;
    if !path.starts_with(root.canonicalize()?) {
        return Err(MstsError::msg(
            "Signal script leaves the original route directory",
        ));
    }
    Ok(path)
}
fn programs(text: &str) -> Result<HashMap<String, String>, MstsError> {
    let mut out = HashMap::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        let mut parts = line.split_ascii_whitespace();
        if parts
            .next()
            .is_some_and(|key| key.eq_ignore_ascii_case("SCRIPT"))
        {
            if let Some((name, source)) = current.take()
                && out.insert(name.clone(), source).is_some()
            {
                return Err(MstsError::msg(format!(
                    "Duplicate native signal program {name}"
                )));
            }
            let name = parts
                .next()
                .ok_or_else(|| MstsError::msg("Missing SIGSCR program name"))?;
            if parts.next().is_some()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/'))
                || out.len() >= 4096
            {
                return Err(MstsError::msg(
                    "Invalid or excessive SIGSCR program headers",
                ));
            }
            current = Some((name.to_ascii_uppercase(), String::new()));
        } else if let Some((_, source)) = current.as_mut() {
            source.push_str(line);
            source.push('\n');
        }
    }
    if let Some((name, source)) = current
        && out.insert(name.clone(), source).is_some()
    {
        return Err(MstsError::msg(format!(
            "Duplicate native signal program {name}"
        )));
    }
    Ok(out)
}

struct WorldHead {
    group: String,
    features: u16,
}

fn world_heads(route: &Path, cfg: &Ast) -> Result<HashMap<u32, WorldHead>, MstsError> {
    let mut shapes = HashMap::new();
    for shape in blocks(cfg, "SignalShape") {
        let Some(name) = atoms(shape).first().cloned() else {
            continue;
        };
        let mut subobjects = HashMap::new();
        for sub in blocks(shape, "SignalSubObj") {
            let index = atoms(sub)
                .first()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| MstsError::msg("Invalid original signal subobject index"))?;
            let feature = field(sub, "SigSubType").and_then(|s| {
                FEATURE_TYPES
                    .iter()
                    .position(|v| v.eq_ignore_ascii_case(&s))
            });
            let back = blocks(sub, "SignalFlags")
                .iter()
                .flat_map(|b| atoms(b))
                .any(|s| s.eq_ignore_ascii_case("BACK_FACING"));
            subobjects.insert(index, (feature, back));
        }
        shapes.insert(
            name.replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap()
                .to_ascii_uppercase(),
            subobjects,
        );
    }
    let mut heads = HashMap::new();
    for (x, z, path) in scan_world_tile_files(route) {
        if std::fs::metadata(&path)?.len() > MAX_NATIVE_FILE_BYTES {
            return Err(MstsError::msg("WORLD signalling file exceeds 16 MiB"));
        }
        let world = WorldFile::from_path(&path)?;
        for item in &world.items {
            let WorldItem::Signal {
                uid,
                file_name: Some(name),
                signal_sub_obj,
                signal_units,
                ..
            } = item
            else {
                continue;
            };
            let name = name
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap()
                .to_ascii_uppercase();
            let Some(subobjects) = shapes.get(&name) else {
                continue;
            };
            let mut features = [0u16; 2];
            for (&index, &(feature, back)) in subobjects {
                if index < 32
                    && signal_sub_obj & (1u32 << index) != 0
                    && let Some(feature) = feature.filter(|feature| *feature > 0)
                {
                    features[usize::from(back)] |= 1 << feature;
                }
            }
            for unit in signal_units {
                let Some((_, back)) = subobjects.get(&unit.sub_obj) else {
                    continue;
                };
                if heads
                    .insert(
                        unit.tr_item_id,
                        WorldHead {
                            group: format!("{x}/{z}/{uid}/{back}"),
                            features: features[usize::from(*back)],
                        },
                    )
                    .is_some()
                {
                    return Err(MstsError::msg(format!(
                        "Ambiguous WORLD signal head {}",
                        unit.tr_item_id
                    )));
                }
            }
        }
    }
    Ok(heads)
}

pub(super) fn bind(
    route: &Path,
    tdb: &Path,
    imported: &str,
    failed_signals: &[u32],
) -> Result<String, MstsError> {
    let cfg_path = ["OpenRails/sigcfg.dat", "sigcfg.dat"]
        .iter()
        .find_map(|name| resolve_path_case_insensitive(&route.join(name)));
    let Some(cfg_path) = cfg_path else {
        return Ok(imported.into());
    };
    let cfg = parse_named_stf(&bounded_text(&cfg_path, MAX_NATIVE_FILE_BYTES)?)?;
    let mut functions = HashMap::new();
    for signal_type in blocks(&cfg, "SignalType") {
        if let (Some(name), Some(function)) = (
            atoms(signal_type).first().cloned(),
            field(signal_type, "SignalFnType"),
        ) {
            functions.insert(name.to_ascii_uppercase(), function.to_ascii_uppercase());
        }
    }
    let mut sources = HashMap::new();
    let files = blocks(&cfg, "ScriptFile");
    for file in files {
        let name = atoms(file)
            .first()
            .cloned()
            .ok_or_else(|| MstsError::msg("Missing SIGCFG ScriptFile name"))?;
        let path = script_path(route, cfg_path.parent().unwrap(), &name)?;
        for (name, source) in programs(&bounded_text(&path, MAX_NATIVE_FILE_BYTES)?)? {
            if sources.insert(name.clone(), source).is_some() {
                return Err(MstsError::msg(format!(
                    "Duplicate native signal program {name}"
                )));
            }
        }
    }
    if sources.is_empty() {
        return Ok(imported.into());
    }
    let world = world_heads(route, &cfg)?;
    let ast = parse_named_stf(&bounded_text(tdb, MAX_TRACK_DB_BYTES)?)?;
    let mut definitions = HashMap::new();
    for signal in blocks(&ast, "SignalItem") {
        let Some(id) = field(signal, "TrItemId").and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let Some(info) = blocks(signal, "TrSignalType").first().copied() else {
            continue;
        };
        let info = atoms(info);
        if info.len() < 4 {
            return Err(MstsError::msg(format!(
                "Incomplete native TrSignalType {id}"
            )));
        }
        let reverse = match info[1].as_str() {
            "0" => false,
            "1" => true,
            _ => return Err(MstsError::msg("Invalid native signal direction")),
        };
        definitions.insert(id, (reverse, info[3].to_ascii_uppercase()));
    }
    let mut track: toml::Value = toml::from_str(imported)?;
    let lengths: HashMap<_, _> = track["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| {
            (
                edge["id"].as_str().unwrap().to_string(),
                edge["length_m"].as_float().unwrap(),
            )
        })
        .collect();
    if let Some(signals) = track.get_mut("signals").and_then(toml::Value::as_array_mut) {
        for signal in signals {
            let id = signal["id"]
                .as_str()
                .unwrap()
                .strip_prefix("sig")
                .unwrap()
                .parse::<u32>()
                .map_err(|_| MstsError::msg("Invalid imported signal id"))?;
            let Some((reverse, name)) = definitions.get(&id) else {
                continue;
            };
            let function = functions.get(name).ok_or_else(|| {
                MstsError::msg(format!("Missing native SIGCFG function for {name}"))
            })?;
            if !matches!(
                function.as_str(),
                "NORMAL" | "DISTANCE" | "INFO" | "REPEATER" | "SHUNTING"
            ) {
                return Err(MstsError::msg(format!(
                    "Unsupported native signal function {function}"
                )));
            }
            let source = sources
                .get(name)
                .ok_or_else(|| MstsError::msg(format!("Missing native SIGSCR program {name}")))?;
            SignalProgram::compile(source).map_err(|e| MstsError::msg(format!("{name}: {e}")))?;
            let edge = signal["edge_id"]
                .as_str()
                .unwrap()
                .trim_end_matches("_r")
                .to_string();
            let length = lengths[&edge];
            let position = signal["position_m"].as_float().unwrap();
            // SData is commonly rounded to millimetres; section arc lengths
            // retain more precision. Reject real out-of-vector placements.
            if !position.is_finite() || position < -0.001 || position > length + 0.001 {
                return Err(MstsError::msg(format!(
                    "Native signal {id} at {position:.6} m lies outside vector {edge} ({length:.6} m)"
                )));
            }
            let position = position.clamp(0.0, length);
            signal["position_m"] = toml::Value::Float(position);
            if *reverse {
                signal["edge_id"] = toml::Value::String(format!("{edge}_r"));
                signal["id"] = toml::Value::String(format!("sig{id}_r"));
                signal["position_m"] = toml::Value::Float(length - position);
            }
            let native = NativeSignalDef {
                name: name.clone(),
                function: function.clone(),
                source: source.clone(),
                feature_flags: world.get(&id).map(|head| head.features),
                group: world.get(&id).map(|head| head.group.clone()),
                forced_stop: failed_signals.contains(&id),
            };
            let script = toml::Value::try_from(SignalScript {
                native: Some(native),
                ..Default::default()
            })
            .map_err(|e| MstsError::msg(e.to_string()))?;
            signal
                .as_table_mut()
                .unwrap()
                .insert("script".into(), script);
        }
    }
    Ok(toml::to_string_pretty(&track)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_script_names_preserve_author_identifiers_and_reject_duplicates() {
        // These are program identifiers, not filesystem paths. OR preserves
        // LQ/UQ names used by Chiltern v4; script file paths remain confined.
        let definitions = programs(
            "SCRIPT RI-Feather\nstate=1;\nSCRIPT UK_4_ASPECT\nstate=7;\nSCRIPT LQ/UQ_Headshunt\nstate=0;\nSCRIPT LQ/UQ_DummyDist\nstate=1;",
        )
        .unwrap();
        assert!(definitions.contains_key("RI-FEATHER"));
        assert!(definitions.contains_key("LQ/UQ_HEADSHUNT"));
        assert!(definitions.contains_key("LQ/UQ_DUMMYDIST"));
        assert!(programs("SCRIPT signal\nstate=1;\nSCRIPT SIGNAL\nstate=7;").is_err());
    }

    #[test]
    fn script_reference_stays_inside_author_route() {
        let root = tempfile::tempdir().unwrap();
        assert!(script_path(root.path(), root.path(), "../another/sigscr.dat").is_err());
        assert!(script_path(root.path(), root.path(), "/etc/passwd").is_err());
    }

    #[test]
    fn large_track_db_does_not_relax_signal_program_limits() {
        let route = tempfile::tempdir().unwrap();
        let tdb = route.path().join("large.tdb");
        let source = b"TrackDB ( TrItemTable ( SignalItem ( TrItemId ( 3 ) TrSignalType ( 00000000 1 1 TEST ) ) ) )";
        let mut contents = vec![b' '; 17 * 1024 * 1024];
        contents[..source.len()].copy_from_slice(source);
        std::fs::write(&tdb, contents).unwrap();
        std::fs::write(route.path().join("sigcfg.dat"), "SignalTypes ( SignalType ( TEST SignalFnType ( NORMAL ) ) ) ScriptFiles ( ScriptFile ( sigscr.dat ) )").unwrap();
        let script = route.path().join("sigscr.dat");
        std::fs::write(&script, "SCRIPT TEST\nstate=SIGASP_CLEAR_2;").unwrap();
        let imported = "[[edges]]\nid='e1'\nlength_m=100.0\n[[signals]]\nid='sig3'\nedge_id='e1'\nposition_m=25.0\naspect='stop'\n";
        let result: toml::Value =
            toml::from_str(&bind(route.path(), &tdb, imported, &[]).unwrap()).unwrap();
        assert_eq!(result["signals"][0]["edge_id"].as_str(), Some("e1_r"));

        std::fs::OpenOptions::new()
            .write(true)
            .open(&script)
            .unwrap()
            .set_len(17 * 1024 * 1024)
            .unwrap();
        assert!(
            bind(route.path(), &tdb, imported, &[])
                .unwrap_err()
                .to_string()
                .contains("exceeds 16 MiB")
        );
        std::fs::write(&script, "SCRIPT TEST\nstate=SIGASP_CLEAR_2;").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&tdb)
            .unwrap()
            .set_len(65 * 1024 * 1024)
            .unwrap();
        assert!(
            bind(route.path(), &tdb, imported, &[])
                .unwrap_err()
                .to_string()
                .contains("exceeds 64 MiB")
        );
    }

    #[test]
    fn native_direction_and_failed_activity_survive_script_binding() {
        let route = tempfile::tempdir().unwrap();
        let tdb = route.path().join("test.tdb");
        std::fs::write(&tdb, "TrackDB ( TrItemTable ( SignalItem ( TrItemId ( 3 ) TrSignalType ( 00000000 1 1 TEST ) ) ) )").unwrap();
        std::fs::write(route.path().join("sigcfg.dat"), "SignalTypes ( SignalType ( TEST SignalFnType ( NORMAL ) ) ) ScriptFiles ( ScriptFile ( sigscr.dat ) )").unwrap();
        std::fs::write(
            route.path().join("sigscr.dat"),
            "SCRIPT TEST\nstate=SIGASP_CLEAR_2;",
        )
        .unwrap();
        let imported = "[[edges]]\nid='e1'\nlength_m=100.0\n[[signals]]\nid='sig3'\nedge_id='e1'\nposition_m=25.0\naspect='stop'\n";
        let result: toml::Value =
            toml::from_str(&bind(route.path(), &tdb, imported, &[3]).unwrap()).unwrap();
        let signal = &result["signals"][0];
        assert_eq!(signal["edge_id"].as_str(), Some("e1_r"));
        assert_eq!(signal["position_m"].as_float(), Some(75.0));
        assert_eq!(
            signal["script"]["native"]["forced_stop"].as_bool(),
            Some(true)
        );
        let rounded = imported.replace("25.0", "100.0003");
        let result: toml::Value =
            toml::from_str(&bind(route.path(), &tdb, &rounded, &[]).unwrap()).unwrap();
        assert_eq!(result["signals"][0]["position_m"].as_float(), Some(0.0));
        assert!(bind(route.path(), &tdb, &imported.replace("25.0", "100.1"), &[]).is_err());
    }
}

//! Original-DLL subsystem diagnostic. Programme speed/pipe are explicit
//! registered boundaries; this is never a closed-loop train trajectory.
use openrailsrs_formats::{PathFile, parse_native_steam161, read_vehicle_ast};
use openrailsrs_sim::native_steam::{NativeSteamInput, NativeSteamState, tables};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn digest(p: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(p)?)))
}
fn read(p: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn write(p: &Path, v: &Value) -> Result<()> {
    fs::write(p, serde_json::to_string_pretty(v)? + "\n")?;
    Ok(())
}
fn number(v: &Value, k: &str) -> Result<f64> {
    v[k].as_f64()
        .filter(|x| x.is_finite())
        .ok_or_else(|| format!("Missing finite {k}").into())
}
fn flag(v: &Value, k: &str) -> Result<bool> {
    v[k].as_bool()
        .ok_or_else(|| format!("Missing bool {k}").into())
}
#[derive(Default, Serialize)]
struct Metric {
    count: usize,
    maximum_absolute_error: f64,
    worst_tick: usize,
    native_at_worst: f64,
    rust_at_worst: f64,
}
impl Metric {
    fn observe(&mut self, tick: usize, n: f64, r: f64) {
        self.count += 1;
        let e = (n - r).abs();
        if e > self.maximum_absolute_error {
            self.maximum_absolute_error = e;
            self.worst_tick = tick;
            self.native_at_worst = n;
            self.rust_at_worst = r;
        }
    }
}
fn originals(m: &Value, extension: &str) -> Result<PathBuf> {
    m["originals"]
        .as_object()
        .ok_or("Missing original provenance")?
        .keys()
        .find(|p| {
            Path::new(p)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(extension))
        })
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing original {extension}").into())
}
fn verify_capture(c: &Path, m: &Value) -> Result<()> {
    if m["reference_version"] != "1.6.1"
        || m["reference_commit"] != "d16e670da333d26d2edfc97d5631a19dadf49ce5"
        || m["method"] != "MSTSSteamLocomotive.Update"
    {
        return Err("Expected a real original subsystem capture".into());
    }
    for (p, h) in m["files"].as_object().ok_or("Missing captured files")? {
        if Path::new(p).is_absolute()
            || Path::new(p)
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err("Invalid captured relative path".into());
        }
        if digest(&c.join(p))? != h.as_str().ok_or("Invalid captured digest")? {
            return Err(format!("Captured file changed: {p}").into());
        }
    }
    for (p, h) in m["originals"]
        .as_object()
        .ok_or("Missing original assets")?
    {
        if digest(Path::new(p))? != h.as_str().ok_or("Invalid original digest")? {
            return Err(format!("Original asset changed: {p}").into());
        }
    }
    let protocol = read(&c.join("protocol.json"))?;
    for (name, hash) in protocol["assemblies_sha256"]
        .as_object()
        .ok_or("Missing DLL pins")?
    {
        if m["assemblies"][name] != *hash {
            return Err(format!("Original DLL pin mismatch: {name}").into());
        }
    }
    let isolation = read(&c.join("wine-isolation/isolation.json"))?;
    let release = read(&c.join("wine-isolation/release.json"))?;
    if isolation["registry_verified"] != true
        || release["remaining_processes"]
            .as_array()
            .is_none_or(|a| !a.is_empty())
    {
        return Err("Missing verified private Wine isolation/release".into());
    }
    Ok(())
}
fn source_hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for p in fs::read_dir(root.join("crates/openrailsrs-sim/src/native_steam"))? {
        let p = p?.path();
        if p.extension().is_some_and(|e| e == "rs") {
            out.insert(
                p.strip_prefix(root)?.to_string_lossy().into_owned(),
                digest(&p)?,
            );
        }
    }
    for p in [
        "crates/openrailsrs-formats/src/typed/native_steam.rs",
        "crates/openrailsrs-sim/examples/steam_cycle_compare.rs",
        "crates/openrailsrs-formats/src/geography.rs",
        "Cargo.lock",
    ] {
        out.insert(p.into(), digest(&root.join(p))?);
    }
    Ok(out)
}
fn atmosphere(initial: &Value, pat: &Path) -> Result<(f32, Value)> {
    // Native source: TrainCar.InitializeCarTemperatures, Summer (SetExplore
    // argument1), then UpdateOutsideTemperature. No measured post-start
    // temperature is consulted. Historic01 omitted the static centre WTC;
    // its fallback is the ordered PAT rear origin, explicitly reported.
    let (tx, tz, x, z, origin) = if let Some(w) = initial.get("world_position") {
        (
            number(w, "tile_x")? as i32,
            number(w, "tile_z")? as i32,
            number(w, "location_x")?,
            number(w, "location_z")?,
            "captured_static_post_start_car_centre",
        )
    } else {
        let path = PathFile::from_path(pat)?;
        let p = path
            .pdps
            .first()
            .and_then(|p| p.world)
            .ok_or("PAT has no ordered starting position")?;
        (
            p.tile_x,
            p.tile_z,
            p.x,
            p.z,
            "derived_ordered_PAT_rear_origin_not_exact_car_centre",
        )
    };
    let geo = openrailsrs_formats::geography::geographic_position(tx, tz, x, z)
        .ok_or("Native WTC outside supported projection")?;
    let latitude = (geo.latitude as f32) * 57.29578;
    let summer = tables::interpolate(
        &[
            -50., -40., -30., -20., -10., 0., 10., 20., 30., 40., 50., 60.,
        ],
        &[
            13.4, 18.3, 22.8, 24.3, 24.4, 25., 25.2, 22.5, 26.6, 24.8, 19.4, 14.3,
        ],
        latitude,
    );
    let height = number(&initial["locomotive"], "TrainCar.CarHeightAboveSeaLevelM")? as f32;
    let outside = summer - (height / 1000. * 9.8).clamp(0., 30.);
    Ok((
        outside,
        json!({"origin":origin,"tile_x":tx,"tile_z":tz,"location_x":x,"location_z":z,
        "latitude_degrees_single":latitude,"summer_c":summer,"initial_height_m":height,"outside_c":outside,
        "post_start_observed_temperature_used":false}),
    ))
}
fn rust_sample(n: &NativeSteamState) -> Value {
    const KG: f64 = 0.45359237;
    json!({"pressure_bar":n.BoilerPressurePSI as f64*0.06894757293168,
        "boiler_mass_kg":n.BoilerMassLB as f64*KG,"boiler_enthalpy_j":n.BoilerHeatBTU as f64*1055.05585262,
        "water_fraction":n.WaterFraction,"production_kg_s":n.EvaporationLBpS as f64*KG,
        "steam_consumption_kg_s":n.PreviousTotalSteamUsageLBpS as f64*KG,
        "injection_kg_s":n.WaterConsumptionLbpS as f64*KG,"coal_burn_kg_s":n.FuelBurnRateSmoothedKGpS,
        "tender_coal_kg":n.tender_coal_kg(),"tender_water_kg":n.tender_water_ukg() as f64*10.021*KG,
        "motive_force_n":n.engine.RealTractiveForceN})
}
fn native_sample(n: &Value) -> Result<Value> {
    const KG: f64 = 0.45359237;
    Ok(
        json!({"pressure_bar":number(n,"pressure_psi")?*0.06894757293168,
        "boiler_mass_kg":number(n,"boiler_mass_lb")?*KG,"boiler_enthalpy_j":number(n,"boiler_heat_btu")?*1055.05585262,
        "water_fraction":number(n,"water_fraction")?,"production_kg_s":number(n,"evaporation_lb_s")?*KG,
        "steam_consumption_kg_s":number(n,"steam_usage_lb_s")?*KG,
        "injection_kg_s":number(n,"water_consumption_lb_s")?*KG,"coal_burn_kg_s":number(n,"fire_burn_kg_s")?,
        "tender_coal_kg":number(n,"tender_coal_kg")?,
        "tender_water_kg":number(n,"combined_tender_water_ukg")?*number(n,"water_lb_per_ukg")?*KG,
        "motive_force_n":number(n,"motive_force_n")?}),
    )
}
fn run() -> Result<bool> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut capture = None;
    let mut output = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        match a.to_str() {
            Some("--capture-dir") => {
                capture = Some(PathBuf::from(
                    args.next().ok_or("Missing capture directory")?,
                ))
            }
            Some("--out-dir") => {
                output = Some(PathBuf::from(
                    args.next().ok_or("Missing fresh output directory")?,
                ))
            }
            _ => {
                return Err(
                    "usage: steam_cycle_compare --capture-dir CAPTURE --out-dir FRESH".into(),
                );
            }
        }
    }
    let capture = capture.ok_or("Missing --capture-dir")?.canonicalize()?;
    let output = output.ok_or("Missing --out-dir")?;
    if output.exists() {
        return Err("Output directory already exists; preserve earlier measurements".into());
    }
    let manifest = read(&capture.join("manifest.json"))?;
    verify_capture(&capture, &manifest)?;
    let protocol = read(&capture.join("protocol.json"))?;
    if protocol["limits_declared_before_capture"] != true {
        return Err("Prior limits are absent".into());
    }
    let reference = read(&capture.join("native/reference.json"))?;
    let config = read(&capture.join("input.json"))?;
    if reference["input"] != config || config["kind"] != "prescribed_speed_subsystem" {
        return Err("Subsystem input/method mismatch".into());
    }
    let engine = originals(&manifest, "eng")?;
    let pat = originals(&manifest, "pat")?;
    let mut profile = parse_native_steam161(&read_vehicle_ast(&engine)?)?;
    profile.hot_start = flag(&config, "hot_start")?;
    let initial = read(&capture.join("native/initial.json"))?;
    let (outside, environment) = atmosphere(&initial, &pat)?;
    let before = source_hashes(&root)?;
    let mut native = NativeSteamState::new(&profile, flag(&config, "hot_start")?)?;
    let initial_rust = serde_json::to_value(&native)?;
    let rows = reference["rows"]
        .as_array()
        .ok_or("No original subsystem rows")?;
    let commands = config["commands"]
        .as_array()
        .ok_or("No registered commands")?;
    let ticks = config["ticks"].as_u64().ok_or("No horizon")? as usize;
    let dt = number(&config, "dt_s")? as f32;
    if rows.len() != ticks + 1 || commands.is_empty() || commands[0]["tick"] != 0 {
        return Err("Incomplete subsystem capture".into());
    }
    fs::create_dir_all(&output)?;
    write(
        &output.join("initial.json"),
        &json!({"original":initial,"rust":initial_rust,"environment":environment,"profile":profile}),
    )?;
    let mut table_results = Vec::new();
    let mut tables_pass = true;
    let table_data = read(&capture.join("native/tables.json"))?;
    if table_data["points"]
        .as_array()
        .is_none_or(|v| v.len() != 250)
        || table_data["grids"].as_array().is_none_or(|v| v.len() != 60)
    {
        return Err("Incomplete original table coverage".into());
    }
    for row in table_data["points"]
        .as_array()
        .ok_or("No native table calls")?
    {
        let factory = row["factory"].as_str().ok_or("No factory")?;
        let value = tables::by_factory(factory)
            .ok_or("Unsupported original table factory")?
            .at(number(row, "x")? as f32) as f64;
        let expected = number(row, "value")?;
        let limit = number(&protocol["new_table_limits"], "abs")?
            + number(&protocol["new_table_limits"], "relative")? * expected.abs();
        let pass = (value - expected).abs() <= limit;
        tables_pass &= pass;
        table_results.push(json!({"input":row,"rust":value,"limit":limit,"pass":pass}));
    }
    for row in table_data["grids"].as_array().ok_or("No native 2D calls")? {
        let factory = row["factory"].as_str().ok_or("No 2D factory")?;
        let upper = match factory {
            "CutoffInitialPressureUpper" => true,
            "CutoffInitialPressureLower" => false,
            _ => return Err("Unsupported 2D factory".into()),
        };
        let value = tables::cutoff_initial_pressure(
            upper,
            number(row, "rpm")? as f32,
            number(row, "cutoff")? as f32,
        ) as f64;
        let expected = number(row, "value")?;
        let limit = number(&protocol["new_table_limits"], "abs")?
            + number(&protocol["new_table_limits"], "relative")? * expected.abs();
        let pass = (value - expected).abs() <= limit;
        tables_pass &= pass;
        table_results.push(json!({"input":row,"rust":value,"limit":limit,"pass":pass}));
    }
    let mut metrics = BTreeMap::<String, Metric>::new();
    let mut internal = BTreeMap::<String, Metric>::new();
    let mut writer = csv::Writer::from_path(output.join("trace.csv"))?;
    writer.write_record(["tick", "field", "native", "rust"])?;
    let mut command = 0usize;
    for (tick, row) in rows.iter().enumerate() {
        if row["tick"].as_u64() != Some(tick as u64) {
            return Err("Original tick sequence is incomplete".into());
        }
        if tick > 0 {
            while command + 1 < commands.len()
                && commands[command + 1]["tick"]
                    .as_u64()
                    .ok_or("Bad command tick")?
                    < tick as u64
            {
                command += 1;
            }
            let c = &commands[command];
            let inject = [flag(c, "injector1")?, flag(c, "injector2")?];
            let handle = number(c, "brake")? as f32;
            let brake_release = matches!(
                native.brake_kind(handle, false),
                openrailsrs_formats::NativeSteamBrakeKind::Release
                    | openrailsrs_formats::NativeSteamBrakeKind::FullQuickRelease
            );
            let speed = number(c, "speed_mps")? as f32;
            native.advance(
                NativeSteamInput {
                    regulator: number(c, "throttle")? as f32,
                    cutoff: number(c, "cutoff")? as f32,
                    speed_mps: speed.abs(),
                    signed_speed_mps: speed,
                    damper: number(c, "damper")? as f32,
                    firing: number(c, "firing")? as f32,
                    blower: profile.initial_blower,
                    injectors_on: inject,
                    injectors: [
                        if inject[0] { 1. } else { 0. },
                        if inject[1] { 1. } else { 0. },
                    ],
                    outside_c: outside,
                    brake_release,
                    brake_pipe_absolute_psi: number(c, "pipe_absolute_psi")? as f32,
                    has_tender_coupled: !profile.tender_required,
                    generator_steam_effects: profile.generator_fitted,
                    ..Default::default()
                },
                dt,
            );
        }
        let r = rust_sample(&native);
        let n = native_sample(row)?;
        for (key, value) in r.as_object().ok_or("Invalid Rust sample")? {
            let rust = value.as_f64().ok_or("Nonfinite Rust sample")?;
            let expected = number(&n, key)?;
            metrics
                .entry(key.clone())
                .or_default()
                .observe(tick, expected, rust);
            writer.write_record([
                tick.to_string(),
                key.clone(),
                expected.to_string(),
                rust.to_string(),
            ])?;
        }
        let state = serde_json::to_value(&native)?;
        for (key, value) in state.as_object().ok_or("Invalid native state")? {
            if let (Some(r), Some(n)) = (value.as_f64(), row["locomotive"][key].as_f64()) {
                internal.entry(key.clone()).or_default().observe(tick, n, r);
            }
        }
        for (key, value) in state["engine"].as_object().ok_or("Invalid engine state")? {
            if let (Some(r), Some(n)) = (value.as_f64(), row["engine"][key].as_f64()) {
                internal
                    .entry(format!("engine.{key}"))
                    .or_default()
                    .observe(tick, n, r);
            }
        }
    }
    writer.flush()?;
    let mut passed = tables_pass;
    let mut acceptance = BTreeMap::new();
    for (field, m) in &metrics {
        let limit = number(&protocol["native_limits"], &format!("{field}_abs"))?;
        let pass = m.maximum_absolute_error <= limit;
        passed &= pass;
        acceptance.insert(field, json!({"limit":limit,"pass":pass,"metric":m}));
    }
    let after = source_hashes(&root)?;
    if after != before {
        return Err("Measurement sources changed during execution".into());
    }
    write(
        &output.join("comparison.json"),
        &json!({"status":if passed{"pass"}else{"fail"},"rows":rows.len(),
        "scope":"Original subsystem with prescribed programme speed/pipe; no route-drive claim",
        "native_thermal_state_prescribed":false,"environment":environment,"acceptance":acceptance,
        "table_calls":table_results,"tables_pass":tables_pass,"internal_diagnostic_only_no_added_limits":internal}),
    )?;
    write(
        &output.join("receipt.json"),
        &json!({"source_sha256":before,"executable_sha256":digest(&std::env::current_exe()?)?,
        "capture_manifest_sha256":digest(&capture.join("manifest.json"))?,"capture_dir":capture,
        "outputs":{"initial.json":digest(&output.join("initial.json"))?,"trace.csv":digest(&output.join("trace.csv"))?,
        "comparison.json":digest(&output.join("comparison.json"))?},"result":if passed{"pass"}else{"fail"}}),
    )?;
    println!(
        "steam subsystem: {} rows, {} table calls, {}",
        rows.len(),
        table_results.len(),
        if passed { "PASS" } else { "FAIL" }
    );
    Ok(passed)
}
fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("steam_cycle_compare: {e}");
            std::process::exit(2);
        }
    }
}

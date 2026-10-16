//! Closed-loop diagnostic against a NEW original SteamDrive capture.
//! No captured speed, force, pressure, heat or flow is fed to the Rust tick.
//! Select either the explicit Rust vessel or the bounded native OR1.6.1 cycle.
//! Preserve every diagnostic; neither selection claims all steam stock parity.
use openrailsrs_formats::{PathFile, typed::SteamThermodynamicParams};
use openrailsrs_sim::{LiveDriveSession, steam::BoilerState, steam_equipment::VehicleBoiler};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const LB_KG: f64 = 0.45359237;
const PSI_BAR: f64 = 0.06894757293168;
const BTU_J: f64 = 1055.05585262;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompareModel {
    OwnVessel,
    NativeOr161,
}
impl CompareModel {
    fn name(self) -> &'static str {
        match self {
            Self::OwnVessel => "own-vessel",
            Self::NativeOr161 => "native-or161",
        }
    }
}

// TrainCar initializes ambient weather on its first base Update, after the
// steam cycle. Simulator-only captures have no WagonViewer to assign terrain
// height or GeneratorSteamEffects. These are static context, not oracle rows.
fn native_environment(
    parameters: &Value,
    pat: &PathFile,
    static_context: Option<&Value>,
) -> Result<Value> {
    let registered_world = static_context
        .and_then(|v| v.get("world_position"))
        .or_else(|| parameters.get("world_position"));
    let (tile_x, tile_z, x, z, origin) = if let Some(world) = registered_world {
        let tile = |key| -> Result<i32> {
            Ok(i32::try_from(
                world
                    .get(key)
                    .and_then(Value::as_i64)
                    .ok_or("Missing static tile")?,
            )?)
        };
        (
            tile("tile_x")?,
            tile("tile_z")?,
            number(world, "location_x")?,
            number(world, "location_z")?,
            "registered_initial_locomotive_world_position",
        )
    } else {
        let point = pat
            .pdps
            .first()
            .and_then(|p| p.world)
            .ok_or("Native climate needs a static original PAT world point")?;
        (
            point.tile_x,
            point.tile_z,
            point.x,
            point.z,
            "original_ordered_PAT_rear_point_approximation_of_locomotive_center",
        )
    };
    let position = openrailsrs_formats::geography::geographic_position(tile_x, tile_z, x, z)
        .ok_or("Original static position is outside the MSTS projection")?;
    let latitude = (position.latitude as f32) * 57.29578_f32;
    let xs = [
        -50f32, -40., -30., -20., -10., 0., 10., 20., 30., 40., 50., 60.,
    ];
    let ys = [
        13.4f32, 18.3, 22.8, 24.3, 24.4, 25., 25.2, 22.5, 26.6, 24.8, 19.4, 14.3,
    ];
    let index = xs
        .partition_point(|p| *p <= latitude)
        .saturating_sub(1)
        .min(xs.len() - 2);
    let d = xs[index + 1] - xs[index];
    let ambient =
        ((xs[index + 1] - latitude) / d) * ys[index] + ((latitude - xs[index]) / d) * ys[index + 1];
    Ok(
        json!({"source":"OR1.6.1 TrainCar.InitializeCarTemperatures; original Summer/Clear explore settings",
        "position_source":origin,"tile_x":tile_x,"tile_z":tile_z,"location_x":x,"location_z":z,
        "latitude_deg":latitude,"initial_outside_c":0.,"subsequent_outside_c":ambient,
        "height_above_sea_level_m":0.,"generator_steam_effects":false,"paused":true,
        "trajectory_fields_used_as_inputs":false,"additional_static_context":static_context,
        "limitation":if origin.starts_with("original_ordered"){Some("Historical capture omitted static locomotive center; PAT rear is a declared geographic approximation, never a fitted observed temperature.")}else{None}}),
    )
}

fn digest(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}
fn number(v: &Value, key: &str) -> Result<f64> {
    v.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("Missing finite numeric field {key}").into())
}
fn boolean(v: &Value, key: &str) -> Result<bool> {
    v.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("Missing bool {key}").into())
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err("Read-only Git provenance query failed".into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
fn measurement_sources(root: &Path) -> Result<BTreeMap<String, String>> {
    let names = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    names
        .split('\0')
        .filter(|p| {
            !p.is_empty()
                && (p.starts_with("crates/") && (p.ends_with(".rs") || p.ends_with(".toml"))
                    || *p == "Cargo.toml"
                    || *p == "Cargo.lock")
        })
        .map(|p| Ok((p.into(), digest(&root.join(p))?)))
        .collect()
}
// A second genuine closed-loop capture can supply only the static center
// omitted by the historic helper. It cannot replace the frozen05 trajectory.
fn load_static_context(
    path: &Path,
    originals: &Value,
    assemblies: &Value,
    config: &Value,
) -> Result<Value> {
    let path = path.canonicalize()?;
    let manifest = read_json(&path.join("manifest.json"))?;
    if manifest["reference_version"] != "1.6.1"
        || manifest["reference_commit"] != "d16e670da333d26d2edfc97d5631a19dadf49ce5"
        || manifest["method"] != "Simulator.Update"
        || manifest["originals"] != *originals
        || manifest["assemblies"] != *assemblies
    {
        return Err("Static context must be a pinned closed-loop capture of the identical original ENG/PAT and DLLs".into());
    }
    let files = manifest["files"]
        .as_object()
        .ok_or("Static context lacks captured file hashes")?;
    for required in [
        "registration.json",
        "protocol.json",
        "input.json",
        "native/reference.json",
        "native/initial.json",
        "wine-isolation/isolation.json",
        "wine-isolation/release.json",
    ] {
        if !files.contains_key(required) {
            return Err(format!("Static context missing provenance: {required}").into());
        }
    }
    for (name, hash) in files {
        let relative = Path::new(name);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("Invalid static-context artifact path".into());
        }
        let file = path.join(relative).canonicalize()?;
        if !file.starts_with(&path)
            || digest(&file)? != hash.as_str().ok_or("Invalid static-context file digest")?
        {
            return Err(format!("Static-context artifact changed or escaped: {name}").into());
        }
    }
    let protocol = read_json(&path.join("protocol.json"))?;
    let reference = read_json(&path.join("native/reference.json"))?;
    let input = read_json(&path.join("input.json"))?;
    if protocol["limits_declared_before_capture"] != true
        || reference["method"] != "Simulator.Update"
        || reference["input"] != input
        || input["kind"] != "original_closed_loop_drive"
        || input["hot_start"] != true
        || input["paused"] != true
        || input["ticks"] != config["ticks"]
        || input["dt_s"] != config["dt_s"]
        || input["commands"] != config["commands"]
    {
        return Err(
            "Static context is not the corresponding registered hot/paused closed-loop experiment"
                .into(),
        );
    }
    for (file, hash) in originals.as_object().ok_or("Original map is invalid")? {
        if digest(Path::new(file))?
            != hash
                .as_str()
                .ok_or("Invalid original static-context digest")?
        {
            return Err(format!("Original static-context input changed: {file}").into());
        }
    }
    let initial = read_json(&path.join("native/initial.json"))?;
    let world = initial
        .get("world_position")
        .ok_or("Static capture omitted initial WorldPosition")?;
    // Only this static object crosses the context boundary. In particular,
    // no captured temperature, thermal inventory, pressure or later row does.
    for key in ["location_x", "location_y", "location_z"] {
        number(world, key)?;
    }
    for key in ["tile_x", "tile_z"] {
        i32::try_from(
            world
                .get(key)
                .and_then(Value::as_i64)
                .ok_or("Invalid static tile")?,
        )?;
    }
    Ok(
        json!({"capture":path,"method":"Simulator.Update","world_position":world,
        "manifest_sha256":digest(&path.join("manifest.json"))?,
        "initial_sha256":digest(&path.join("native/initial.json"))?,
        "reference_sha256":digest(&path.join("native/reference.json"))?,
        "original_ENG_PAT_and_DLLs_identical":true,"only_static_world_position_used":true}),
    )
}

#[derive(Default, Serialize)]
struct Metric {
    rows: usize,
    maximum_absolute_error: f64,
    squared_error: f64,
    native_integral: f64,
    rust_integral: f64,
    worst_tick: usize,
}
impl Metric {
    fn observe(&mut self, tick: usize, native: f64, rust: f64, dt: f64) {
        let error = (native - rust).abs();
        self.rows += 1;
        self.squared_error += (rust - native).powi(2);
        self.native_integral += native * dt;
        self.rust_integral += rust * dt;
        if error > self.maximum_absolute_error {
            self.maximum_absolute_error = error;
            self.worst_tick = tick;
        }
    }
}
#[derive(Deserialize)]
struct Command {
    tick: usize,
    phase: String,
    throttle: f64,
    brake: f64,
    cutoff: f64,
    damper: f64,
    firing: f64,
    injector1: bool,
    injector2: bool,
}
fn rust_liquid_kg(s: &LiveDriveSession) -> Option<f64> {
    let n = s.state.boiler_state.as_ref()?.native_or161.as_ref()?;
    Some(
        n.WaterFraction as f64
            * n.hardware.BoilerVolumeFT3 as f64
            * n.BoilerWaterDensityLBpFT3 as f64
            * LB_KG,
    )
}
fn rust_sample(
    s: &LiveDriveSession,
    previous_safety_kg: f64,
    previous_liquid_kg: Option<f64>,
    dt: f64,
) -> Result<Value> {
    let b = s.state.boiler_state.as_ref().ok_or("Missing Rust boiler")?;
    if let Some(n) = &b.native_or161 {
        let liquid = rust_liquid_kg(s).ok_or("Missing native liquid inventory")?;
        let net = previous_liquid_kg.map_or(0., |previous| {
            ((previous + n.BoilerWaterInputLB as f64 * LB_KG - liquid) / dt).max(0.)
        });
        return Ok(json!({
            "pressure_bar":n.BoilerPressurePSI as f64*PSI_BAR,
            "boiler_mass_kg":n.BoilerMassLB as f64*LB_KG,
            "boiler_heat_j":n.BoilerHeatBTU as f64*BTU_J,
            "pressure_psi":n.BoilerPressurePSI,"boiler_mass_lb":n.BoilerMassLB,"boiler_heat_btu":n.BoilerHeatBTU,
            "water_fraction":n.WaterFraction,"production_kg_s":n.EvaporationLBpS as f64*LB_KG,
            "net_evaporation_kg_s":net,"thermal_power_w":n.BoilerKW as f64*1000.,
            "steam_consumption_kg_s":n.PreviousTotalSteamUsageLBpS as f64*LB_KG,
            "injection_kg_s":n.WaterConsumptionLbpS as f64*LB_KG,"coal_burn_kg_s":n.FuelBurnRateSmoothedKGpS,
            "tender_coal_kg":n.tender_coal_kg(),
            "tender_water_kg":n.tender_water_ukg() as f64*10.021*LB_KG,
            "speed_mps":s.velocity_mps(),"distance_m":s.state.odometer_m,
            "motive_force_n":s.state.rail_force_n.first().copied().unwrap_or(b.tractive_force_n),
            "injector1":b.controls.injector1,"injector2":b.controls.injector2,
        }));
    }
    let t = b
        .thermodynamics
        .as_ref()
        .ok_or("Diagnostic vessel was not initialized")?;
    let properties = openrailsrs_sim::steam_thermodynamics::properties(b.pressure_bar);
    Ok(json!({
        "pressure_bar":b.pressure_bar,"boiler_mass_kg":t.total_mass_kg,
        "boiler_enthalpy_j":t.internal_energy_j+(101325.+b.pressure_bar*100000.)*t.volume_m3,
        "water_fraction":b.water_kg/(properties.water_density_kg_m3*t.volume_m3),
        "production_kg_s":t.thermal_capacity_kg_s,"net_evaporation_kg_s":b.evaporation_kg_s,
        "thermal_power_w":t.thermal_power_w,
        "steam_consumption_kg_s":b.steam_usage_kg_s+(t.safety_steam_kg-previous_safety_kg)/dt,
        "injection_kg_s":b.injection_kg_s,"coal_burn_kg_s":b.coal_burn_kg_s,
        "tender_coal_kg":b.coal_kg,"tender_water_kg":b.tender_water_kg,
        "speed_mps":s.velocity_mps(),"distance_m":s.state.odometer_m,
        "motive_force_n":s.state.rail_force_n.first().copied().unwrap_or(b.tractive_force_n),
        "mass_residual_kg":t.mass_residual_kg(),"energy_residual_j":t.energy_residual_j(),
        "injector1":b.controls.injector1,"injector2":b.controls.injector2,
    }))
}
fn native_liquid_kg(row: &Value, parameters: &Value) -> Result<f64> {
    Ok(number(row, "water_fraction")?
        * number(parameters, "boiler_volume_ft3")?
        * number(row, "boiler_water_density_lb_ft3")?
        * LB_KG)
}
fn native_sample(
    row: &Value,
    parameters: &Value,
    previous_liquid_kg: f64,
    dt: f64,
) -> Result<Value> {
    let liquid = native_liquid_kg(row, parameters)?;
    let net = if row["tick"] == 0 {
        0.
    } else {
        ((previous_liquid_kg + number(row, "injector_water_input_lb")? * LB_KG - liquid) / dt)
            .max(0.)
    };
    Ok(json!({
        "pressure_bar":number(row,"pressure_psi")?*PSI_BAR,
        "boiler_mass_kg":number(row,"boiler_mass_lb")?*LB_KG,
        "boiler_enthalpy_j":number(row,"boiler_heat_btu")?*BTU_J,
        "boiler_heat_j":number(row,"boiler_heat_btu")?*BTU_J,
        "pressure_psi":number(row,"pressure_psi")?,"boiler_mass_lb":number(row,"boiler_mass_lb")?,
        "boiler_heat_btu":number(row,"boiler_heat_btu")?,
        "water_fraction":number(row,"water_fraction")?,
        "production_kg_s":number(row,"evaporation_lb_s")?*LB_KG,
        "net_evaporation_kg_s":net,"thermal_power_w":number(row,"boiler_kw")?*1000.,
        "steam_consumption_kg_s":number(row,"steam_usage_lb_s")?*LB_KG,
        "injection_kg_s":number(row,"water_consumption_lb_s")?*LB_KG,
        "coal_burn_kg_s":number(row,"fire_burn_kg_s")?,
        "tender_coal_kg":number(row,"tender_coal_kg")?,
        "tender_water_kg":number(row,"combined_tender_water_ukg")?*number(row,"water_lb_per_ukg")?*LB_KG,
        "speed_mps":number(row,"speed_mps")?,"distance_m":number(row,"distance_m")?,
        "motive_force_n":number(row,"motive_force_n")?,
        "injector1":boolean(row,"injector1")?,"injector2":boolean(row,"injector2")?,
    }))
}

fn run() -> Result<bool> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut capture = None;
    let mut output = None;
    let mut model = CompareModel::OwnVessel;
    let mut static_context_capture = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--capture-dir")=>capture=Some(PathBuf::from(args.next().ok_or("--capture-dir needs a path")?)),
            Some("--out-dir")=>output=Some(PathBuf::from(args.next().ok_or("--out-dir needs a path")?)),
            Some("--static-context-capture")=>static_context_capture=Some(PathBuf::from(args.next().ok_or("--static-context-capture needs a path")?)),
            Some("--model")=>model=match args.next().and_then(|v|v.to_str().map(str::to_owned)).as_deref(){
                Some("own-vessel")=>CompareModel::OwnVessel,
                Some("native-or161")=>CompareModel::NativeOr161,
                _=>return Err("--model expects own-vessel or native-or161".into()),
            },
            _=>return Err("usage: steam_native_compare --model own-vessel|native-or161 --capture-dir NEW_ORIGINAL_CAPTURE --out-dir FRESH_RUST_DIRECTORY [--static-context-capture CORRESPONDING_CLOSED_LOOP_CAPTURE]".into()),
        }
    }
    let capture = capture.ok_or("--capture-dir is required")?.canonicalize()?;
    let output = output.ok_or("--out-dir is required")?;
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()?.join(output)
    };
    let manifest = read_json(&capture.join("manifest.json"))?;
    let protocol = read_json(&capture.join("protocol.json"))?;
    if manifest["capture_kind"] != "original_dll_calls"
        || manifest["reference_version"] != "1.6.1"
        || manifest["reference_commit"] != "d16e670da333d26d2edfc97d5631a19dadf49ce5"
        || manifest["native_steam_drive_captured"] != true
        || protocol["limits_declared_before_capture"] != true
    {
        return Err("A complete real SteamDrive capture and prior limits are required".into());
    }
    let artifacts = manifest["files"]
        .as_object()
        .ok_or("Missing captured file hashes")?;
    for required in [
        "inputs.json",
        "protocol.json",
        "registration.json",
        "steam-drive-config.json",
        "native-steam/steam-drive.json",
        "wine-isolation/isolation.json",
        "wine-isolation/release.json",
    ] {
        if !artifacts.contains_key(required) {
            return Err(format!("Missing captured provenance: {required}").into());
        }
    }
    for (name, hash) in protocol["assemblies_sha256"]
        .as_object()
        .ok_or("Missing pinned DLL hashes")?
    {
        if manifest["assemblies"][name] != *hash {
            return Err(format!("Native DLL pin differs: {name}").into());
        }
    }
    for (relative, hash) in artifacts {
        if Path::new(relative).is_absolute()
            || Path::new(relative)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err("Invalid captured artifact path".into());
        }
        if digest(&capture.join(relative))? != hash.as_str().ok_or("Invalid artifact digest")? {
            return Err(format!("Captured artifact changed: {relative}").into());
        }
    }
    let originals = manifest["steam_originals"]
        .as_object()
        .ok_or("Missing original ENG/PAT provenance")?;
    let original_path = |suffix: &str| -> Result<PathBuf> {
        originals
            .keys()
            .find(|p| {
                Path::new(p)
                    .extension()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.eq_ignore_ascii_case(suffix))
            })
            .map(PathBuf::from)
            .ok_or_else(|| format!("Missing original {suffix}").into())
    };
    let vehicle = original_path("eng")?;
    let pat = original_path("pat")?;
    for (file, hash) in originals {
        if digest(Path::new(file))? != hash.as_str().ok_or("Invalid original digest")? {
            return Err(format!("Original asset changed: {file}").into());
        }
    }
    let route = pat
        .parent()
        .and_then(Path::parent)
        .ok_or("PAT must be inside original route/PATHS")?;
    let original_content = route
        .parent()
        .and_then(Path::parent)
        .ok_or("Missing original content root")?;
    if output.exists()
        || output.starts_with(original_content.canonicalize()?)
        || output.starts_with(&capture)
    {
        return Err("Use a fresh output outside original content and the native capture".into());
    }
    let config = read_json(&capture.join("steam-drive-config.json"))?;
    let reference = read_json(&capture.join("native-steam/steam-drive.json"))?;
    if reference["method"] != "Simulator.Update" || reference["input"] != config {
        return Err("SteamDrive method/config does not match its registered input".into());
    }
    if static_context_capture.is_some() && model != CompareModel::NativeOr161 {
        return Err("--static-context-capture is only used by --model native-or161".into());
    }
    let static_context = static_context_capture
        .as_ref()
        .map(|p| {
            load_static_context(
                p,
                &manifest["steam_originals"],
                &manifest["assemblies"],
                &config,
            )
        })
        .transpose()?;
    let dt = number(&config, "dt_s")?;
    if (dt - 0.05).abs() > 1e-12 {
        return Err("Native helper supports the registered 50ms step only".into());
    }
    let ticks = config["ticks"].as_u64().ok_or("Missing ticks")? as usize;
    let commands: Vec<Command> = serde_json::from_value(config["commands"].clone())?;
    let rows = reference["rows"]
        .as_array()
        .ok_or("Missing real SteamDrive rows")?;
    if ticks == 0
        || rows.len() != ticks + 1
        || commands.first().is_none_or(|c| c.tick != 0)
        || commands.windows(2).any(|c| c[0].tick >= c[1].tick)
        || commands.last().is_some_and(|c| c.tick >= ticks)
    {
        return Err("Incomplete SteamDrive horizon/control phases".into());
    }
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("rust-source.diff"),
        git(&root, &["diff", "--binary", "HEAD"])?,
    )?;
    fs::write(
        output.join("rust-working-tree.txt"),
        git(&root, &["status", "--porcelain=v1"])?,
    )?;
    let derived = output.join("route");
    fs::create_dir(&derived)?;
    fs::write(
        derived.join("track.toml"),
        openrailsrs_msts::import_route(route)?,
    )?;
    let loaded = openrailsrs_route::load_route_from_dir(&derived)?;
    let locomotive = openrailsrs_train::from_ast::load_engine_from_path(&vehicle)?;
    let placement = openrailsrs_msts::placement_for_pat_with_consist(
        &loaded.graph,
        &loaded.msts_aliases,
        &pat,
        0.,
        Some(locomotive.length_m),
    )?;
    let parsed_pat = PathFile::from_path(&pat)?;
    let environment = if model == CompareModel::NativeOr161 {
        Some(native_environment(
            &reference["parameters"],
            &parsed_pat,
            static_context.as_ref(),
        )?)
    } else {
        None
    };
    let waypoints = openrailsrs_msts::pat_waypoints_with_offset(
        &loaded.graph,
        &loaded.msts_aliases,
        &parsed_pat,
        &placement.start,
        &placement.destination,
        placement.start_offset_m,
    )?;
    let consist = output.join("original-steam.con");
    fs::write(
        &consist,
        format!(
            "(Train (Engine {}))\n",
            serde_json::to_string(&vehicle.to_string_lossy())?
        ),
    )?;
    let mut scenario = openrailsrs_scenarios::load_scenario(
        root.join("examples/physics_lab/scenario_double_steam.toml"),
    )?;
    scenario.scenario.name = "Original steam route: independently evolved Rust diagnostic".into();
    scenario.scenario.start_time_s = Some(9. * 3600. + 29. * 60.);
    scenario.route.path = derived.to_string_lossy().into_owned();
    scenario.route.start = placement.start;
    scenario.route.destination = placement.destination;
    scenario.route.start_offset_m = Some(placement.start_offset_m);
    scenario.route.switches = placement.switches;
    scenario.route.waypoints = waypoints;
    scenario.route.stops.clear();
    scenario.route.assume_signals_clear = true;
    scenario.train.consist = consist.to_string_lossy().into_owned();
    scenario.train.davis = None;
    scenario.train.rack = None;
    scenario.train.bogie_dynamics.clear();
    scenario.train.systems.clear();
    scenario.simulation.time_step = dt;
    scenario.simulation.duration = ticks as f64 * dt;
    scenario.simulation.multi_body = false;
    scenario.simulation.brake_shoe_speed_factor = false;
    scenario.simulation.brake_skid_limit = false;
    scenario.simulation.native_physics = Some(openrailsrs_scenarios::NativePhysicsEnvironment {
        simple_control_physics: true,
        ambient_c: environment
            .as_ref()
            .map_or(Ok(20.), |e| number(e, "subsequent_outside_c"))?,
        adhesion_factor: 1.,
        paused_weather: true,
        first_slippery_spot: None,
        ..Default::default()
    });
    if model == CompareModel::NativeOr161 {
        scenario.simulation.legacy_power_cap = false;
    }
    fs::write(
        output.join("scenario.toml"),
        toml::to_string_pretty(&scenario)?,
    )?;
    let mut session = LiveDriveSession::from_scenario(&output, &scenario)?;
    let mut params = locomotive
        .steam
        .ok_or("Original vehicle is not a steam locomotive")?;
    let original_params = params.clone();
    let initialized = &reference["parameters"];
    if model == CompareModel::NativeOr161 {
        if initialized["settings_simple_control_physics"] != true
            || initialized["settings_use_advanced_adhesion"] != false
            || initialized["settings_hot_start"] != true
            || initialized["firing_is_manual"] != true
        {
            return Err("Native port currently requires the registered hot/manual/simple closed-loop context".into());
        }
        // Only authored ENG data and registered static settings select the
        // native cycle. No initialized thermal fields or trajectory rows are
        // copied into its factory.
        let ast = openrailsrs_formats::parse_named_stf(
            &openrailsrs_formats::read_msts_text_with_includes(&vehicle)?,
        )?;
        let mut profile = openrailsrs_formats::parse_native_steam161(&ast)?;
        profile.hot_start = true;
        if profile.tender_required {
            return Err("The single-engine SteamDrive has no separate tender".into());
        }
        let hardware = openrailsrs_sim::native_steam::NativeSteamHardware::new(&profile)?;
        params.working_pressure_bar = profile.maximum_pressure_psi as f64 * PSI_BAR;
        params.operation.max_fire_mass_kg = hardware.MaxFireMassKG as f64;
        params.operation.max_firing_rate_kg_s = hardware.MaxFiringRateKGpS as f64;
        params.operation.boiler_water_capacity_kg =
            hardware.BoilerVolumeFT3 as f64 * 0.028316846592 * 1000.;
        params.evaporation_rate_kg_per_s = hardware.TheoreticalMaxSteamOutputLBpS as f64 * LB_KG;
        params.coal_consumption_kg_per_s = hardware.MaxFuelBurnGrateKGpS as f64;
        params.initial_water_kg = profile.tender_water_capacity_kg as f64;
        params.initial_coal_kg = profile.tender_coal_capacity_kg as f64;
        params.operation.thermodynamics = None;
        params.operation.compound = None;
        params.operation.native_or161 = Some(profile);
    } else {
        let own = &config["rust_cycle_options"];
        // Static numeric hardware emitted before the native first tick, not a
        // fit to observed pressure, fuel, production or motion. Original assets
        // stay read-only. The own cycle's efficiencies are registered in advance.
        params.working_pressure_bar = number(initialized, "max_pressure_psi")? * PSI_BAR;
        params.operation.max_fire_mass_kg = number(initialized, "max_fire_mass_kg")?;
        params.operation.max_firing_rate_kg_s = number(initialized, "max_firing_rate_kg_s")?;
        params.evaporation_rate_kg_per_s = number(initialized, "max_steam_output_lb_s")? * LB_KG;
        params.coal_consumption_kg_per_s = number(initialized, "max_fuel_burn_grate_kg_s")?;
        let volume = number(initialized, "boiler_volume_ft3")? * 0.028316846592;
        params.operation.boiler_water_capacity_kg = volume * 1000.;
        params.operation.thermodynamics = Some(SteamThermodynamicParams {
            startup: serde_json::from_value(own["startup"].clone())?,
            boiler_volume_m3: volume,
            coal_heat_j_kg: number(initialized, "coal_heat_kj_kg")? * 1000.,
            boiler_efficiency: number(own, "boiler_efficiency")?,
            heat_loss_w: number(own, "heat_loss_w")?,
            feedwater_c: number(own, "feedwater_c")?,
            cylinder_efficiency: number(own, "cylinder_efficiency")?,
            idle_steam_kg_s: number(own, "idle_steam_kg_s")?,
        });
        params.operation.native_or161 = None;
    }
    if !params.valid() {
        return Err(
            "Original static parameters fall outside the declared Rust vessel domain".into(),
        );
    }
    let mut boiler = BoilerState::from_params(&params);
    if let Some(native) = &mut boiler.native_or161 {
        native.feedback.outside_c = 0.;
        native.feedback.height_m = 0.;
        native.feedback.generator_steam_effects = false;
        native.feedback.has_tender_coupled = !native.hardware.source.tender_required;
        session
            .state
            .brake_system
            .initialize_native_steam_vacuum(native)?;
        session.state.ambient_temperature_c = 0.;
    }
    session.physics.steam_params = Some(params.clone());
    session.state.steam_boilers = vec![VehicleBoiler {
        vehicle: 0,
        params: params.clone(),
        boiler: boiler.clone(),
        local_regulator: None,
    }];
    session.state.boiler_state = Some(boiler);
    session.driver_direction = 1.;
    session.driver_throttle = 0.;
    session.driver_brake = 1.;
    if model == CompareModel::OwnVessel {
        session.state.brake_system.precharge(1.);
    }
    write_json(
        &output.join("configuration-overlay.json"),
        &json!({
            "kind":"explicit_model_overlay_on_read_only_original_stock","selected_model":model.name(),
            "hardware_input_origin":if model == CompareModel::NativeOr161 {"Expanded original authored STF, parsed with separate Wagon/Engine roots; no native initialized thermal values"} else {"Prior registered own-vessel overlay using numeric initialized hardware"},
            "static_environment":environment,
            "original_imported_params":original_params,"original_initialized_params":initialized,
            "effective_rust_params":params,"native_state_prescribed":false,
            "native_speed_force_pressure_heat_flow_prescribed":false,
            "save_restore_scope":"No snapshot is emitted: this diagnostic uses a declared in-memory model overlay, not an author scenario replacement.",
        }),
    )?;
    let initial_rust = rust_sample(&session, 0., None, dt)?;
    let mut previous_rust_liquid = rust_liquid_kg(&session);
    let heat_field = if model == CompareModel::NativeOr161 {
        "boiler_heat_j"
    } else {
        "boiler_enthalpy_j"
    };
    let mut previous_native_liquid = native_liquid_kg(&rows[0], initialized)?;
    let initial_native = native_sample(&rows[0], initialized, previous_native_liquid, dt)?;
    write_json(
        &output.join("initial.json"),
        &json!({"native":initial_native,"rust":initial_rust}),
    )?;
    let mut fields = vec![
        "pressure_bar",
        "boiler_mass_kg",
        heat_field,
        "water_fraction",
        "production_kg_s",
        "net_evaporation_kg_s",
        "thermal_power_w",
        "steam_consumption_kg_s",
        "injection_kg_s",
        "coal_burn_kg_s",
        "tender_coal_kg",
        "tender_water_kg",
        "speed_mps",
        "distance_m",
        "motive_force_n",
    ];
    if model == CompareModel::NativeOr161 {
        fields.extend(["pressure_psi", "boiler_mass_lb", "boiler_heat_btu"]);
    }
    let mut writer = csv::Writer::from_path(output.join("trace.csv"))?;
    let mut header = vec!["tick".to_string(), "time_s".into(), "phase".into()];
    for &key in &fields {
        header.push(format!("native_{key}"));
        header.push(format!("rust_{key}"));
    }
    writer.write_record(&header)?;
    let mut stats: BTreeMap<String, BTreeMap<String, Metric>> = BTreeMap::new();
    let mut command = 0;
    let mut injector_mismatches = 0usize;
    let mut maximum_mass_residual: f64 = 0.;
    let mut maximum_energy_residual: f64 = 0.;
    let mut completed = 0;
    for (tick, row) in rows.iter().enumerate().take(ticks + 1) {
        if tick > 0 {
            while command + 1 < commands.len() && commands[command + 1].tick < tick {
                command += 1;
            }
            let c = &commands[command];
            session.driver_throttle = c.throttle;
            session.driver_brake = c.brake;
            let b = session
                .state
                .boiler_state
                .as_mut()
                .ok_or("Boiler disappeared")?;
            b.controls.automatic_fireman = false;
            b.controls.cutoff = c.cutoff;
            b.controls.damper = c.damper;
            b.controls.firing = c.firing;
            b.controls.injector1 = c.injector1;
            b.controls.injector2 = c.injector2;
        }
        let safety = session
            .state
            .boiler_state
            .as_ref()
            .and_then(|b| b.thermodynamics.as_ref())
            .map_or(0., |t| t.safety_steam_kg);
        if tick > 0 {
            session.step_realtime(dt, |_| {});
        }
        let native = native_sample(row, initialized, previous_native_liquid, dt)?;
        previous_native_liquid = native_liquid_kg(row, initialized)?;
        let rust = rust_sample(
            &session,
            safety,
            if tick == 0 {
                None
            } else {
                previous_rust_liquid
            },
            dt,
        )?;
        previous_rust_liquid = rust_liquid_kg(&session);
        if let Some(environment) = &environment {
            // Preserve the first steam-before-base-Update ambient0 phase,
            // then derive weather from static geography, never captured rows.
            session.state.ambient_temperature_c = number(environment, "subsequent_outside_c")?;
        }
        let phase = if tick == 0 {
            "initial"
        } else {
            &commands[command].phase
        };
        let mut record = vec![
            tick.to_string(),
            (tick as f64 * dt).to_string(),
            phase.to_string(),
        ];
        for &key in &fields {
            let mut n = number(&native, key)?;
            let mut r = number(&rust, key)?;
            if key == "distance_m" {
                n -= number(&initial_native, key)?;
                r -= number(&initial_rust, key)?;
            }
            record.push(n.to_string());
            record.push(r.to_string());
            stats
                .entry(phase.into())
                .or_default()
                .entry(key.into())
                .or_default()
                .observe(tick, n, r, if tick == 0 { 0. } else { dt });
        }
        for injector in ["injector1", "injector2"] {
            injector_mismatches += usize::from(native[injector] != rust[injector]);
        }
        if model == CompareModel::OwnVessel {
            maximum_mass_residual =
                maximum_mass_residual.max(number(&rust, "mass_residual_kg")?.abs());
            maximum_energy_residual =
                maximum_energy_residual.max(number(&rust, "energy_residual_j")?.abs());
        }
        writer.write_record(record)?;
        completed = tick;
        if tick < ticks && session.arrived {
            break;
        }
    }
    writer.flush()?;
    let limits = &protocol["native_limits"]["steam_drive_diagnostic"];
    let mapping = [
        ("pressure_bar", "pressure_bar_abs"),
        ("boiler_mass_kg", "boiler_mass_kg_abs"),
        (heat_field, "boiler_enthalpy_j_abs"),
        ("water_fraction", "water_fraction_abs"),
        ("production_kg_s", "production_kg_s_abs"),
        ("net_evaporation_kg_s", "production_kg_s_abs"),
        ("steam_consumption_kg_s", "steam_consumption_kg_s_abs"),
        ("injection_kg_s", "injection_kg_s_abs"),
        ("coal_burn_kg_s", "coal_burn_kg_s_abs"),
        ("tender_coal_kg", "tender_coal_kg_abs"),
        ("tender_water_kg", "tender_water_kg_abs"),
        ("speed_mps", "speed_mps_abs"),
        ("distance_m", "distance_m_abs"),
        ("motive_force_n", "motive_force_n_abs"),
    ];
    let mut failures = vec![];
    for (phase, metrics) in &stats {
        for (field, limit) in mapping {
            let allowed = number(limits, limit)?;
            let metric = &metrics[field];
            if metric.maximum_absolute_error > allowed {
                failures.push(json!({"phase":phase,"field":field,
            "maximum_absolute_error":metric.maximum_absolute_error,"limit":allowed,"worst_tick":metric.worst_tick}));
            }
        }
    }
    let limits_own = &protocol["own_model_limits"];
    let conserved = if model == CompareModel::OwnVessel {
        Some(
            maximum_mass_residual <= number(limits_own, "vessel_mass_residual_kg_abs")?
                && maximum_energy_residual <= number(limits_own, "vessel_energy_residual_j_abs")?,
        )
    } else {
        None
    };
    let original_unchanged = originals
        .iter()
        .all(|(p, h)| digest(Path::new(p)).ok().as_deref() == h.as_str());
    let passed = failures.is_empty()
        && completed == ticks
        && conserved.unwrap_or(true)
        && injector_mismatches == 0
        && original_unchanged;
    let report = json!({"schema":1,"status":if passed{"within_prior_limits"}else{"outside_prior_limits"},
        "comparison_kind":"closed_loop_diagnostic","full_native_steam_cycle_parity_claim":false,
        "native_reference":"Original DLL Simulator.Update; no substitute equations in helper",
        "selected_model":model.name(),
        "rust_model":if model == CompareModel::NativeOr161 {"Bounded OR1.6.1 coal/simple/manual steam port, separately initialized from authored original STF"} else {"Pinned original property tables plus declared finite-volume vessel/cylinder extension"},
        "thermal_inventory_definition":if model == CompareModel::NativeOr161 {"Direct BoilerHeatBTU versus BoilerHeatBTU; boiler_heat_j is its unit conversion. The immutable prior boiler_enthalpy_j_abs limit is reused numerically without adding PV or applying own-vessel residuals."} else {"Own finite-vessel enthalpy U+PV against original BoilerHeatBTU, retained as the historical declared diagnostic"},
        "static_environment":environment,
        "native_state_or_motion_prescribed":false,"completed_ticks":completed,"expected_ticks":ticks,
        "phase_metrics":stats,"failed_metrics":failures,"limits":limits,"injector_state_mismatches":injector_mismatches,
        "mass_residual_kg_abs":if model == CompareModel::OwnVessel {Some(maximum_mass_residual)}else{None},
        "energy_residual_j_abs":if model == CompareModel::OwnVessel {Some(maximum_energy_residual)}else{None},
        "conservation_passed":conserved,
        "own_vessel_conservation_applicable":model == CompareModel::OwnVessel,"original_content_unchanged":original_unchanged,
        "initialization":"Each factory initializes independently; initial pressure/mass/heat differences are retained and counted.",
        "production_definition":if model == CompareModel::NativeOr161 { "production_kg_s compares the independently evolved EvaporationLBpS directly; BoilerHeatBTU retains its native meaning. The separately derived liquid-mass change is diagnostic. Original pressure, production, consumption and injection limits are unchanged." } else { "production_kg_s compares gross thermal capacity Qdot/h_steam with the original EvaporationLBpS. The separate net_evaporation_kg_s observes liquid mass change plus injector input, using native raw density/volume/fraction; both retain the prior production tolerance. Thermal power is diagnostic without a new acceptance threshold." },
        "integral_units":"A rate integral has mass units; the diagnostic also emits integrals of non-rate fields without treating them as conserved amounts.",
    });
    write_json(&output.join("comparison.json"), &report)?;
    let binary = std::env::current_exe()?;
    let artifacts = [
        "rust-source.diff",
        "rust-working-tree.txt",
        "configuration-overlay.json",
        "scenario.toml",
        "route/track.toml",
        "initial.json",
        "trace.csv",
        "comparison.json",
    ]
    .into_iter()
    .map(|p| Ok((p, digest(&output.join(p))?)))
    .collect::<Result<BTreeMap<_, _>>>()?;
    write_json(
        &output.join("receipt.json"),
        &json!({"schema":1,"kind":"rust_binary_measurement",
            "git_head":git(&root,&["rev-parse","HEAD"] )?.trim(),
            "working_tree_is_dirty":!git(&root,&["status","--porcelain=v1"] )?.is_empty(),
            "binary":binary,"binary_sha256":digest(&binary)?,
            "source_sha256_at_measurement":digest(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/steam_native_compare.rs"))?,
            "measurement_source_sha256":measurement_sources(&root)?,
            "selected_model":model.name(),"static_environment":environment,
            "capture_manifest_sha256":digest(&capture.join("manifest.json"))?,
            "native_trace_sha256":digest(&capture.join("native-steam/steam-drive.json"))?,
            "protocol_sha256":digest(&capture.join("protocol.json"))?,
            "artifacts_sha256":artifacts,
            "provenance_scope":"HEAD is ancestry only. Dirty diff and actual executable/source hashes identify this measurement; later formatting does not invalidate immutable native evidence.",
        }),
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(passed)
}
fn main() {
    match run() {
        Ok(true) => (),
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("Steam diagnostic could not complete: {error}");
            std::process::exit(2);
        }
    }
}

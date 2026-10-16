//! Original Class 101 / SCE closed-loop comparison, without prescribed motion.
//! Usage: gearbox_native_drive --out-dir TMP [--content-root ORIGINAL_CONTENT]
//! Only controls and immutable original author files feed LiveDriveSession.
use openrailsrs_formats::PathFile;
use openrailsrs_sim::LiveDriveSession;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn digest(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn number(value: &Value, key: &str) -> f64 {
    value[key]
        .as_f64()
        .unwrap_or_else(|| panic!("missing {key}"))
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}
fn csv_number(row: &csv::StringRecord, header: &csv::StringRecord, key: &str) -> f64 {
    row[header.iter().position(|name| name == key).unwrap()]
        .parse()
        .unwrap()
}

fn sample(
    session: &LiveDriveSession,
    tick: usize,
    throttle: f64,
    brake: f64,
    command: i64,
) -> Vec<f64> {
    let car = &session.state.diesel.cars[0];
    let gear = car.gearbox.as_ref().expect("original authored gearbox");
    let config = &session.physics.diesel.cars[0];
    let profile = config.params.gearbox.as_ref().unwrap();
    let velocity = session.velocity_mps();
    let cylinder = &session.state.brake_system.cylinders[0];
    let vacuum_psi = cylinder.pipe_pressure_bar().unwrap_or(0.) / 0.06894757293168;
    let cylinder_psi = cylinder.pressure_bar() / 0.06894757293168;
    let motive = session
        .state
        .rail_force_n
        .first()
        .copied()
        .or_else(|| session.state.diesel_traction_force_n.first().copied())
        .unwrap_or(0.);
    let friction = if let (Some(native), Some(state)) =
        (&session.physics.native, &session.state.native_dynamics)
    {
        native.resistance(
            0,
            session.physics.mass_kg,
            &session.physics.vehicle_davis[0],
            velocity,
            state,
        )
    } else {
        session.physics.davis.resistance_n(velocity)
    };
    let head = session.head_chainage_m();
    let length = session.physics.vehicle_lengths_m[0];
    let grade = session.path_data.average_grade_between(head - length, head) / 100.;
    let edge = session.current_edge_id().unwrap_or("");
    let node = edge
        .strip_prefix('e')
        .unwrap_or(edge)
        .trim_end_matches("_r")
        .parse::<f64>()
        .unwrap_or(-1.);
    vec![
        tick as f64,
        tick as f64 * 0.05,
        throttle,
        brake,
        command as f64,
        velocity,
        session.state.odometer_m,
        car.rpm,
        car.demanded_rpm,
        throttle * 100.,
        openrailsrs_sim::gearbox::interpolate_extrapolated(&config.governor.power_tab, car.rpm),
        gear.current_gear_index as f64,
        gear.next_gear_index as f64,
        gear.shaft_rpm(profile, config.governor.max_rpm, car.rpm, velocity),
        gear.clutch_pct,
        if gear.clutch_on { 1. } else { 0. },
        car.flow_lps,
        car.fuel_l,
        motive,
        session.wheel_rail_brake_forces_n().iter().sum(),
        friction,
        -session.physics.mass_kg * 9.80665 * grade,
        14.503774 - vacuum_psi,
        cylinder_psi,
        node,
        session.pos_on_edge_m(),
    ]
}

fn run() -> Result<bool> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut content = PathBuf::from(std::env::var_os("HOME").ok_or("HOME unavailable")?)
        .join("Documentos/Open Rails/Content/Demo Model 1");
    let mut reference = root.join("docs/fixtures/compatibility/gearbox-or161");
    let mut output = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--out-dir") => output = Some(PathBuf::from(args.next().ok_or("--out-dir needs a path")?)),
            Some("--content-root") => content = PathBuf::from(args.next().ok_or("--content-root needs a path")?),
            Some("--reference-dir") => reference = PathBuf::from(args.next().ok_or("--reference-dir needs a path")?),
            _ => return Err("usage: gearbox_native_drive --out-dir FRESH_TMP [--content-root ORIGINAL] [--reference-dir FIXTURE]".into()),
        }
    }
    let output = output.ok_or("--out-dir is required")?;
    let output = if output.is_absolute() {
        output
    } else {
        std::env::current_dir()?.join(output)
    };
    if output.exists() || output.starts_with(content.canonicalize()?) {
        return Err("Use a fresh output directory outside original content".into());
    }
    fs::create_dir_all(&output)?;
    let manifest = read_json(&reference.join("manifest.json"))?;
    let protocol = read_json(&reference.join("drive-config.json"))?;
    let tolerances = read_json(&reference.join("tolerances.json"))?;
    let limits = &tolerances["native_drive"];
    let expected_initial = read_json(&reference.join("native-drive/initial.json"))?;
    let vehicle =
        content.join("TRAINS/trainset/MT_DB_Class101_BlueGrey/MT_DB_Cl101_BlGr_DMBS_SC51451.eng");
    let route = content.join("ROUTES/SCE");
    let native_path = route.join("PATHS/MT_MT_0930 Edinburgh-Glasgow Queen Street(player).PAT");
    if digest(&vehicle)? != manifest["source_vehicle"]["sha256"]
        || digest(&native_path)? != manifest["source_path"]["sha256"]
    {
        return Err("Original ENG/PAT differ from the captured native reference".into());
    }
    for file in [
        "drive-config.json",
        "tolerances.json",
        "native-drive/initial.json",
        "native-drive/trace.csv",
    ] {
        if digest(&reference.join(file))? != manifest["files"][file] {
            return Err(format!("Native reference changed: {file}").into());
        }
    }
    // Materialize derived route data under the task output, never in Content.
    let derived = output.join("route");
    fs::create_dir(&derived)?;
    fs::write(
        derived.join("track.toml"),
        openrailsrs_msts::import_route(&route)?,
    )?;
    let loaded = openrailsrs_route::load_route_from_dir(&derived)?;
    let locomotive = openrailsrs_train::from_ast::load_engine_from_path(&vehicle)?;
    let placement = openrailsrs_msts::placement_for_pat_with_consist(
        &loaded.graph,
        &loaded.msts_aliases,
        &native_path,
        0.,
        Some(locomotive.length_m),
    )?;
    let path = PathFile::from_path(&native_path)?;
    let waypoints = openrailsrs_msts::pat_waypoints_with_offset(
        &loaded.graph,
        &loaded.msts_aliases,
        &path,
        &placement.start,
        &placement.destination,
        placement.start_offset_m,
    )?;
    let consists = output.join("physics/consists");
    fs::create_dir_all(&consists)?;
    let consist = consists.join("class101.con");
    fs::write(
        &consist,
        format!(
            "(Train (Engine {}))\n",
            serde_json::to_string(&vehicle.to_string_lossy())?
        ),
    )?;
    let template = root.join("examples/class47_reference/scenario.toml");
    let mut scenario = openrailsrs_scenarios::load_scenario(&template)?;
    scenario.scenario.name = "Original Class 101: closed-loop native comparison".into();
    scenario.scenario.start_time_s = Some(9. * 3600. + 29. * 60.);
    scenario.route.path = derived.to_string_lossy().into_owned();
    scenario.route.start = placement.start.clone();
    scenario.route.destination = placement.destination.clone();
    scenario.route.start_offset_m = Some(placement.start_offset_m);
    scenario.route.switches = placement.switches;
    scenario.route.waypoints = waypoints;
    scenario.route.stops.clear();
    scenario.route.assume_signals_clear = true;
    scenario.train.consist = consist.to_string_lossy().into_owned();
    scenario.simulation.duration = number(&protocol, "ticks") * number(&protocol, "step_s");
    scenario.simulation.time_step = number(&protocol, "step_s");
    // OR TrainCar.GetBrakeShoeFrictionFactor returns a constant multiplier in
    // this reference's SimpleControlPhysics=true / advanced-adhesion=false mode.
    // The reused Class 47 template exercised the advanced brake-shoe model.
    scenario.simulation.brake_shoe_speed_factor = false;
    scenario.simulation.brake_skid_limit = false;
    scenario.simulation.native_physics = Some(openrailsrs_scenarios::NativePhysicsEnvironment {
        simple_control_physics: true,
        ambient_c: 20.,
        adhesion_factor: 1.,
        paused_weather: true,
        first_slippery_spot: None,
        ..Default::default()
    });
    fs::write(
        output.join("scenario.toml"),
        toml::to_string_pretty(&scenario)?,
    )?;
    let mut session = LiveDriveSession::from_scenario(&output, &scenario)?;
    session.driver_direction = 1.;
    session.driver_throttle = 0.;
    session.driver_brake = 1.;
    session.state.brake_system.precharge(1.);
    for _ in 0..protocol["settle_ticks"].as_u64().unwrap() {
        session.step_realtime(0.05, |_| {});
    }
    let settle_odometer = session.state.odometer_m;
    let settle_fuel = session.state.diesel.cars[0].consumed_l;
    let initial = sample(&session, 0, 0., 1., 0);
    write_json(
        &output.join("initial.json"),
        &json!({
            "kind":"rust_closed_loop_route_simulation", "motion_or_force_prescribed":false,
            "source_vehicle":vehicle, "source_path":native_path,
            "source_vehicle_sha256":digest(&vehicle)?, "source_path_sha256":digest(&native_path)?,
            "track_sha256":digest(&derived.join("track.toml"))?,
            "placement":{"start":placement.start,"offset_m":placement.start_offset_m,"destination":placement.destination},
            "mass_kg":session.physics.mass_kg,"length_m":session.physics.vehicle_lengths_m,
            "brake_system":session.state.brake_system,"diesel":session.state.diesel,
            "native_vehicle_parameters_debug":session.physics.native.as_ref().map(|p| format!("{:?}",p.vehicles)),
            "initial_native_reference":expected_initial,"initial_row":initial,
        }),
    )?;
    let mut native = csv::Reader::from_path(reference.join("native-drive/trace.csv"))?;
    let header = native.headers()?.clone();
    let native_rows = native
        .records()
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut writer = csv::Writer::from_path(output.join("trace.csv"))?;
    let mut debug = csv::Writer::from_path(output.join("diagnostics.csv"))?;
    debug.write_record([
        "tick",
        "speed_before_mps",
        "rpm",
        "traction_rpm",
        "clutch_pct",
        "gearbox_force_n",
        "motor_force_n",
        "rail_force_n",
        "traction_force_n",
        "wheel_speed_mps",
    ])?;
    writer.write_record(&header)?;
    writer.write_record(initial.iter().map(ToString::to_string))?;
    let commands = protocol["commands"].as_array().unwrap();
    let mut command = 0;
    let mut last_gear = -1;
    let mut shifts = vec![];
    let mut max_speed_error: f64 = 0.;
    let mut max_distance_error: f64 = 0.;
    let mut square_speed_error = 0.;
    let mut native_integrated_flow = 0.;
    let mut diagnostic_worst = Value::Null;
    let ticks = protocol["ticks"].as_u64().unwrap() as usize;
    if native_rows.len() != ticks + 1 {
        return Err("Native capture horizon differs".into());
    }
    for tick in 0..ticks {
        while command + 1 < commands.len()
            && commands[command + 1]["tick"].as_u64().unwrap() <= tick as u64
        {
            command += 1;
        }
        let controls = &commands[command];
        let throttle = number(controls, "throttle");
        let brake = number(controls, "brake");
        let requested = controls["gear"].as_i64().unwrap() - 1;
        session.driver_throttle = throttle;
        session.driver_brake = brake;
        let current_requested = session.state.diesel.cars[0]
            .gearbox
            .as_ref()
            .unwrap()
            .next_gear_index;
        for _ in 0..(requested - i64::from(current_requested)).abs() {
            session.shift_gear((requested - i64::from(current_requested)).signum() as i32)?;
        }
        let speed_before = session.velocity_mps();
        session.step_realtime(0.05, |_| {});
        let row = sample(&session, tick + 1, throttle, brake, requested + 1);
        writer.write_record(row.iter().map(ToString::to_string))?;
        let engine = &session.state.diesel.cars[0];
        let config = &session.physics.diesel.cars[0];
        let gear = engine.gearbox.as_ref().unwrap();
        let direct_force = gear.force_n(
            config.params.gearbox.as_ref().unwrap(),
            config.governor.idle_rpm,
            config.governor.max_rpm,
            openrailsrs_sim::gearbox::GearboxInput {
                speed_mps: speed_before,
                throttle,
                rpm: engine.rpm,
                output_power_w: openrailsrs_sim::gearbox::interpolate_extrapolated(
                    &config.governor.power_tab,
                    engine.rpm,
                ),
                running: true,
            },
        );
        let wheel_speed = session
            .state
            .native_dynamics
            .as_ref()
            .and_then(|d| d.axles.first())
            .map_or(speed_before, |a| a.speed_mps);
        debug.write_record(
            [
                tick as f64 + 1.,
                speed_before,
                engine.rpm,
                session.state.diesel_rpm[0],
                gear.clutch_pct,
                direct_force,
                session.state.motor_force_n[0],
                session.state.rail_force_n[0],
                session.state.diesel_traction_force_n[0],
                wheel_speed,
            ]
            .iter()
            .map(ToString::to_string),
        )?;
        let expected = &native_rows[tick + 1];
        let velocity_error = row[5] - csv_number(expected, &header, "velocity_mps");
        let distance_error = row[6] - settle_odometer - csv_number(expected, &header, "odometer_m");
        native_integrated_flow += csv_number(expected, &header, "flow_lps") * 0.05;
        square_speed_error += velocity_error * velocity_error;
        max_distance_error = max_distance_error.max(distance_error.abs());
        if velocity_error.abs() > max_speed_error {
            max_speed_error = velocity_error.abs();
            diagnostic_worst = json!({"tick":tick+1,"rust_row":row,"native_row":expected.iter().collect::<Vec<_>>(),"velocity_error_mps":velocity_error,"distance_error_m":distance_error});
        }
        let gear = row[11] as i32;
        if gear != last_gear {
            shifts.push(json!({"time_s":row[1],"from":last_gear,"to":gear}));
            last_gear = gear;
        }
    }
    writer.flush()?;
    debug.flush()?;
    let consumed = session.state.diesel.cars[0].consumed_l - settle_fuel;
    let fuel_error = consumed - native_integrated_flow;
    let mut native_shifts = vec![];
    let mut last = -1;
    for row in &native_rows {
        let gear = csv_number(row, &header, "current_gear_index") as i32;
        if gear != last {
            native_shifts
                .push(json!({"time_s":csv_number(row,&header,"time_s"),"from":last,"to":gear}));
            last = gear;
        }
    }
    let same_gear_sequence = shifts.len() == native_shifts.len()
        && shifts
            .iter()
            .zip(&native_shifts)
            .all(|(a, b)| a["from"] == b["from"] && a["to"] == b["to"]);
    let max_shift_error = if same_gear_sequence {
        shifts
            .iter()
            .zip(&native_shifts)
            .map(|(a, b)| (number(a, "time_s") - number(b, "time_s")).abs())
            .fold(0., f64::max)
    } else {
        f64::MAX
    };
    let passed = max_speed_error <= number(limits, "speed_mps_abs")
        && max_distance_error <= number(limits, "distance_m_abs")
        && fuel_error.abs() <= number(limits, "fuel_l_abs")
        && max_shift_error <= number(limits, "manual_shift_time_s_abs");
    let report = json!({
        "schema_version":1,"status":if passed {"passed"} else {"failed"},
        "scope":"LiveDriveSession loaded original author ENG/PAT/SCE and independently evolved motion, forces, engine, brakes and fuel; only authored controls prescribed.",
        "original_content_unmodified":digest(&vehicle)?==manifest["source_vehicle"]["sha256"] && digest(&native_path)?==manifest["source_path"]["sha256"],
        "reference_version":manifest["reference_version"],"reference_commit":manifest["reference_commit"],
        "reference_trace_sha256":digest(&reference.join("native-drive/trace.csv"))?,"tolerances_sha256":digest(&reference.join("tolerances.json"))?,
        "tolerances":limits,"rows":ticks+1,"max_speed_error_mps":max_speed_error,
        "rms_speed_error_mps":(square_speed_error/ticks as f64).sqrt(),"max_distance_error_m":max_distance_error,
        "rust_consumed_l":consumed,"native_integrated_flow_l":native_integrated_flow,"fuel_error_l":fuel_error,
        "rust_shifts":shifts,"native_shifts":native_shifts,"same_gear_sequence":same_gear_sequence,"max_shift_time_error_s":max_shift_error,
        "rust_final_speed_mps":session.velocity_mps(),"rust_distance_m":session.state.odometer_m-settle_odometer,
        "worst_speed_sample":diagnostic_worst,"rust_trace_sha256":digest(&output.join("trace.csv"))?,
        "diagnostic_column_scope":"Sampled resistance/gravity use final tick geometry/speed; velocity, odometer, RPM, clutch, flow and fuel are production session state. No diagnostic force is applied to the simulation."
    });
    write_json(&output.join("comparison.json"), &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(passed)
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("Closed native gearbox comparison failed: {error}");
            std::process::exit(2);
        }
    }
}

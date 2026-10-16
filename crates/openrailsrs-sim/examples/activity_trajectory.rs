//! Replay an entire native driving trace through the playable physics path.
//! Stops/events are separately reported: this is physical trajectory acceptance.
use openrailsrs_sim::LiveDriveSession;
use std::{fs::File, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: activity_trajectory scenario.toml native-trace.csv output.csv".into());
    }
    let path = Path::new(&args[1]);
    let mut scenario = openrailsrs_scenarios::load_scenario(path)?;
    // Keep the original physical path and all vehicles. Station scoring is not
    // allowed to truncate a physical replay when implementations drift.
    scenario.route.stops.clear();
    scenario.native_activity = None;
    let mut s =
        LiveDriveSession::from_scenario(path.parent().ok_or("scenario parent")?, &scenario)?;
    s.set_direction(1.)?;
    let mut input = csv::Reader::from_path(&args[2])?;
    let headers = input.headers()?.clone();
    let index = |name: &str| {
        headers
            .iter()
            .position(|s| s == name)
            .ok_or_else(|| format!("missing {name}"))
    };
    let ti = index("time_s")?;
    let th = index("throttle")?;
    let br = index("brake")?;
    let door = index("doors_open")?;
    let mut out = csv::Writer::from_writer(File::create(&args[3])?);
    out.write_record([
        "time_s",
        "velocity_mps",
        "odometer_m",
        "throttle",
        "brake",
        "doors_open",
        "brake_pipe_psi",
        "cylinder_psi",
        "motive_force_n",
        "brake_force_n",
    ])?;
    let mut previous = 0.;
    for row in input.records() {
        let row = row?;
        let number = |i: usize| -> Result<f64, Box<dyn std::error::Error>> {
            let x: f64 = row.get(i).ok_or("short native row")?.parse()?;
            if !x.is_finite() {
                return Err("nonfinite native row".into());
            }
            Ok(x)
        };
        let time = number(ti)?;
        let throttle = number(th)?;
        let brake = number(br)?;
        let doors = number(door)? > 0.5;
        if !(0. ..=1.).contains(&throttle)
            || !(0. ..=1.).contains(&brake)
            || time < previous
            || time > 3600.001
        {
            return Err("invalid native control or time".into());
        }
        if time > 0. {
            let dt = time - previous;
            if (dt - 0.05).abs() > 1e-6 {
                return Err("native replay must use declared20Hz".into());
            }
            s.driver_throttle = throttle;
            s.driver_brake = brake;
            if s.exterior.door.anim_open_target() != doors {
                s.toggle_doors();
            }
            let before = s.time_s();
            s.step_realtime(dt, |_| {});
            if s.time_s() <= before {
                return Err(format!(
                    "physical replay stopped at {time}: {:?}",
                    s.gameplay.failure
                )
                .into());
            }
        }
        let c = s
            .state
            .brake_system
            .cylinders
            .first()
            .ok_or("no brake cylinder")?;
        out.serialize((
            s.time_s(),
            s.velocity_mps(),
            s.state.odometer_m,
            throttle,
            brake,
            if doors { 1 } else { 0 },
            c.pipe_pressure_bar().unwrap_or(0.) / 0.0689475729,
            c.pressure_bar() / 0.0689475729,
            s.state.rail_force_n.iter().sum::<f64>(),
            s.state.brake_system.last_native_brake_force_n(),
        ))?;
        previous = time;
    }
    if previous < 1. {
        return Err("empty physical replay".into());
    }
    out.flush()?;
    Ok(())
}

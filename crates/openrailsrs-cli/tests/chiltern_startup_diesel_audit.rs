//! OR-P6 startup diesel audit (0–40 s): CSV traces + OR vs sim table.
//!
//! Writes `run_startup_diesel_audit.csv` with per-engine RPM, apparent throttle,
//! tractive force, and MSTS run-up fraction. OR limits ORTS lead via RPM → apparent
//! throttle (not `RunUpTimeToMaxForce`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use openrailsrs_scenarios::{apply_scenario_runtime_overlay_dir, load_scenario};
use openrailsrs_sim::{ScriptedDriver, run_scenario_headless_with_driver};
use openrailsrs_train::load_consist_with_asset_root;
use openrailsrs_validate::{OrColumnMap, parse_or_dump_csv};

fn chiltern_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern")
}

#[derive(Debug, Clone)]
struct DieselSample {
    time_s: f64,
    velocity_mps: f64,
    throttle: f64,
    brake: f64,
    rpm: Vec<f64>,
    apparent: Vec<f64>,
    f_n: Vec<f64>,
    #[allow(dead_code)]
    run_up: Vec<f64>,
}

fn parse_diesel_run_csv(path: &Path, engine_count: usize) -> Vec<DieselSample> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .expect("open run csv");
    let headers: Vec<String> = rdr
        .headers()
        .expect("headers")
        .iter()
        .map(str::to_string)
        .collect();
    let idx =
        |name: &str| -> Option<usize> { headers.iter().position(|h| h.eq_ignore_ascii_case(name)) };
    let i_t = idx("time_s").expect("time_s");
    let i_v = idx("velocity_mps").expect("velocity_mps");
    let i_th = idx("throttle").expect("throttle");
    let i_br = idx("brake").expect("brake");
    let diesel_cols: Vec<(usize, usize, usize, usize)> = (0..engine_count)
        .map(|i| {
            (
                idx(&format!("diesel_rpm_{i}")).unwrap_or_else(|| panic!("diesel_rpm_{i}")),
                idx(&format!("diesel_apparent_{i}"))
                    .unwrap_or_else(|| panic!("diesel_apparent_{i}")),
                idx(&format!("diesel_f_n_{i}")).unwrap_or_else(|| panic!("diesel_f_n_{i}")),
                idx(&format!("diesel_run_up_{i}")).unwrap_or_else(|| panic!("diesel_run_up_{i}")),
            )
        })
        .collect();

    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.expect("record");
        let parse_f = |i: usize| rec.get(i).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let mut rpm = Vec::with_capacity(engine_count);
        let mut apparent = Vec::with_capacity(engine_count);
        let mut f_n = Vec::with_capacity(engine_count);
        let mut run_up = Vec::with_capacity(engine_count);
        for (ir, ia, ifn, iu) in &diesel_cols {
            rpm.push(parse_f(*ir));
            apparent.push(parse_f(*ia));
            f_n.push(parse_f(*ifn));
            run_up.push(parse_f(*iu));
        }
        out.push(DieselSample {
            time_s: parse_f(i_t),
            velocity_mps: parse_f(i_v),
            throttle: parse_f(i_th),
            brake: parse_f(i_br),
            rpm,
            apparent,
            f_n,
            run_up,
        });
    }
    out
}

fn sample_at(samples: &[DieselSample], t: f64) -> DieselSample {
    samples
        .iter()
        .min_by(|a, b| {
            (a.time_s - t)
                .abs()
                .partial_cmp(&(b.time_s - t).abs())
                .unwrap()
        })
        .cloned()
        .unwrap_or_else(|| DieselSample {
            time_s: t,
            velocity_mps: 0.0,
            throttle: 0.0,
            brake: 0.0,
            rpm: vec![0.0; 2],
            apparent: vec![0.0; 2],
            f_n: vec![0.0; 2],
            run_up: vec![0.0; 2],
        })
}

fn or_velocity_at(or_samples: &[openrailsrs_validate::TraceSample], t: f64) -> f64 {
    or_samples
        .iter()
        .min_by(|a, b| {
            (a.time_s - t)
                .abs()
                .partial_cmp(&(b.time_s - t).abs())
                .unwrap()
        })
        .map(|s| s.velocity_mps)
        .unwrap_or(0.0)
}

#[test]
fn chiltern_startup_diesel_audit_0_40s() {
    let chiltern = chiltern_dir();
    if !chiltern.join("track.toml").exists() {
        return;
    }

    let mut scenario = load_scenario(chiltern.join("scenario.toml")).expect("scenario");
    apply_scenario_runtime_overlay_dir(&mut scenario, &chiltern).expect("overlay");
    scenario.simulation.duration = 40.0;
    scenario.output.csv = "run_startup_diesel_audit.csv".into();
    scenario.output.metadata = "run_startup_diesel_audit.json".into();

    let mut driver = ScriptedDriver::from_csv(chiltern.join("driver_or.csv")).expect("driver");
    run_scenario_headless_with_driver(&chiltern, &scenario, &mut driver).expect("sim");

    let run_path = chiltern.join("run_startup_diesel_audit.csv");
    assert!(
        run_path.exists(),
        "audit CSV missing: {}",
        run_path.display()
    );

    let consist =
        load_consist_with_asset_root(chiltern.join("consists/birmingham_pullman.con"), &chiltern)
            .expect("consist");
    let models = consist.diesel_traction_models();
    assert_eq!(models.len(), 2, "Pullman consist: lead DMBSA + trail DMBSH");

    let samples = parse_diesel_run_csv(&run_path, models.len());
    assert!(
        samples.len() >= 35,
        "expected ~40 rows, got {}",
        samples.len()
    );

    let or_path = chiltern.join("../baselines/chiltern_birmingham/or_evaluation_speed.csv");
    let or_trace = parse_or_dump_csv(&or_path, &OrColumnMap::default()).expect("OR trace");
    let target_rpm = models[0]
        .engine
        .as_ref()
        .map(|e| e.target_rpm(0.8))
        .unwrap_or(0.0);
    let idle_rpm = models[0]
        .engine
        .as_ref()
        .map(|e| e.idle_rpm)
        .unwrap_or(650.0);

    eprintln!("\n=== Chiltern startup diesel audit (0–40 s) ===");
    eprintln!("DMBSA target RPM @ 80% notch: {target_rpm:.0} (idle {idle_rpm:.0})");
    eprintln!(
        "{:>4} {:>7} {:>7} {:>7} {:>6} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "t",
        "v_or",
        "v_sim",
        "dv",
        "brake",
        "rpm0",
        "app0",
        "F0",
        "Pcap0",
        "Fsum",
        "rpm1",
        "app1",
        "F1"
    );

    let mut sq_err = 0.0;
    let mut n = 0_usize;
    let mut checks: HashMap<i32, DieselSample> = HashMap::new();

    for t in 0..=40 {
        let t = t as f64;
        let s = sample_at(&samples, t);
        checks.insert(t as i32, s.clone());
        let v_or = or_velocity_at(&or_trace.samples, t);
        let dv = s.velocity_mps - v_or;
        sq_err += dv * dv;
        n += 1;
        let v = s.velocity_mps.max(0.5);
        let f0 = s.f_n.first().copied().unwrap_or(0.0);
        let f1 = s.f_n.get(1).copied().unwrap_or(0.0);
        let f_sum = f0 + f1;
        let app0 = s.apparent.first().copied().unwrap_or(0.0);
        let p_cap0 = models[0].traction_power_cap_w(
            s.rpm.first().copied().unwrap_or(650.0),
            s.throttle,
            v,
            false,
        );
        let f_cap0 = if v > 0.5 { p_cap0 / v } else { f64::INFINITY };
        eprintln!(
            "{t:4.0} {v_or:7.3} {v_sim:7.3} {dv:+7.3} {br:6.3} {rpm0:7.0} {app0:7.3} {f0:7.0} {pcap:7.0} {fsum:7.0} {rpm1:7.0} {app1:7.3} {f1:7.0}",
            t = t,
            v_or = v_or,
            v_sim = s.velocity_mps,
            dv = dv,
            br = s.brake,
            rpm0 = s.rpm.first().copied().unwrap_or(0.0),
            app0 = app0,
            f0 = f0,
            pcap = f_cap0,
            fsum = f_sum,
            rpm1 = s.rpm.get(1).copied().unwrap_or(0.0),
            app1 = s.apparent.get(1).copied().unwrap_or(0.0),
            f1 = f1,
        );
    }

    let rms = (sq_err / n as f64).sqrt();
    eprintln!("velocity RMS (1 s samples): {rms:.3} m/s");
    eprintln!("audit CSV: {}", run_path.display());

    for s in &samples {
        for (i, app) in s.apparent.iter().enumerate() {
            assert!(
                *app <= s.throttle + 0.02,
                "t={:.0} eng[{i}] apparent {app:.3} > throttle {:.3}",
                s.time_s,
                s.throttle
            );
        }
    }

    let s5 = checks.get(&5).expect("t=5");
    assert!(
        s5.rpm[0] > idle_rpm + 50.0,
        "t=5 lead RPM should rise above idle while brakes on, got {:.0}",
        s5.rpm[0]
    );
    assert!(
        s5.f_n.iter().sum::<f64>() < 1000.0,
        "t=5 no traction while brake held (OR revs only), F_sum={:.0}",
        s5.f_n.iter().sum::<f64>()
    );
    assert!(
        s5.velocity_mps < 0.05,
        "t=5 creep with residual brake: v={:.3}",
        s5.velocity_mps
    );

    let s7 = checks.get(&7).expect("t=7");
    assert!(
        s7.brake < 0.02,
        "t=7 brake should be released, got {:.3}",
        s7.brake
    );
    assert!(
        s7.apparent[0] < 0.79,
        "t=7 lead apparent {:.3} should be < driver 0.80 (OR RPM cap)",
        s7.apparent[0]
    );

    let s40 = checks.get(&40).expect("t=40");
    assert!(
        s40.rpm[0] >= target_rpm - 80.0,
        "t=40 lead RPM {:.0} should be near target {target_rpm:.0}",
        s40.rpm[0]
    );

    assert!(
        models[1].engine.is_some(),
        "trail DMBSH inherits ORTS from lead (OR-P13); OR evaluation still differs on legacy stub path"
    );
    let s13 = checks.get(&13).expect("t=13");
    assert!(
        s13.f_n[0] > 40_000.0,
        "t=13 lead should carry traction alone, F0={:.0}",
        s13.f_n[0]
    );
}

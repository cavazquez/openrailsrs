#[test]
fn audit_chiltern_forces_at_cruise() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chiltern");
    let con = base.join("consists/birmingham_pullman.con");
    if !con.exists() {
        return;
    }
    let consist = openrailsrs_train::load_consist_with_asset_root(&con, &base).expect("consist");
    let models = consist.diesel_traction_models();
    let throttle = 0.8;
    let f_res = |v: f64| {
        consist.davis.a_n + consist.davis.b_n_per_mps * v + consist.davis.c_n_per_mps2 * v * v
    };
    eprintln!(
        "mass={:.0} davis@10.86m/s={:.0}N",
        consist.total_mass_kg(),
        f_res(10.863)
    );
    for (i, m) in models.iter().enumerate() {
        let fv = m.force_at(10.863, throttle);
        let rpm = m
            .engine
            .as_deref()
            .map(|e| e.target_rpm(throttle))
            .unwrap_or(0.0);
        let pwr = m.effective_power_w(rpm, throttle);
        eprintln!(
            "eng[{i}] scale={:.4} F={:.0} rpm={:.0} P={:.0} P/v={:.0}",
            m.effort_scale,
            fv,
            rpm,
            pwr,
            pwr / 10.863
        );
    }
    for v in [
        9.0, 10.0, 10.863, 11.0, 12.0, 12.8, 13.0, 15.0, 18.0, 20.0, 22.0, 23.0,
    ] {
        let fr = f_res(v);
        let mut f_trac = 0.0;
        for (i, m) in models.iter().enumerate() {
            let rpm = if i == 0 { 1200.0 } else { 0.0 };
            let run_factor = 1.0;
            let mut f = m.force_at_scaled(v, throttle, rpm, run_factor, 0.0, true);
            let p = m.traction_power_cap_w(rpm, throttle, v, true) * run_factor;
            if v > 0.5 && p > 0.0 && m.engine.is_some() {
                f = f.min(p / v);
            }
            eprintln!(
                "  eng[{i}] v={v:.1} F={f:.0} P/v={:.0}",
                if v > 0.5 { p / v } else { 0.0 }
            );
            f_trac += f;
        }
        eprintln!(
            "v={v:.1} F_trac={f_trac:.0} F_res={fr:.0} delta={:.0}",
            f_trac - fr
        );
    }
}

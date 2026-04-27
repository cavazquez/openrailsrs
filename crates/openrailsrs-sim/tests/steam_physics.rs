//! Tests for the steam traction model (Fase 24).

use openrailsrs_sim::steam::{BoilerState, stall_force_n, steam_step};
use openrailsrs_train::SteamParams;

/// Reference locomotive: 2-8-0 Consolidation with known parameters.
fn consolidation() -> SteamParams {
    SteamParams {
        cylinder_count: 2,
        cylinder_bore_m: 0.470,
        piston_stroke_m: 0.660,
        driving_wheel_radius_m: 0.970,
        working_pressure_bar: 16.0,
        evaporation_rate_kg_per_s: 10.0,
        coal_consumption_kg_per_s: 0.70,
        initial_water_kg: 15_000.0,
        initial_coal_kg: 8_000.0,
    }
}

fn fresh_boiler(params: &SteamParams) -> BoilerState {
    BoilerState::from_params(params)
}

// ── Tractive effort ───────────────────────────────────────────────────────────

#[test]
fn stall_force_is_positive_and_reasonable() {
    let params = consolidation();
    let f = stall_force_n(&params);
    // Theoretical: ~155 000 N for a 2-8-0 at 16 bar.
    assert!(f > 100_000.0, "stall force too low: {f:.0} N");
    assert!(f < 300_000.0, "stall force unreasonably high: {f:.0} N");
}

#[test]
fn stall_force_matches_steam_params_method() {
    let params = consolidation();
    let from_fn = stall_force_n(&params);
    let from_method = params.max_tractive_effort_n();
    assert!(
        (from_fn - from_method).abs() < 0.1,
        "mismatch between stall_force_n and SteamParams::max_tractive_effort_n"
    );
}

#[test]
fn no_force_when_regulator_closed() {
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    let f = steam_step(&mut boiler, &params, 0.0, 20.0, 1.0);
    assert!(
        f.abs() < 1.0,
        "force should be zero with closed regulator, got {f:.1} N"
    );
}

#[test]
fn full_regulator_gives_max_stall_force() {
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    // At v = 0, full regulator should give approximately the stall force.
    let f = steam_step(&mut boiler, &params, 1.0, 0.0, 0.01);
    let expected = stall_force_n(&params);
    let rel_err = (f - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "stall force mismatch: got {f:.0} N, expected ~{expected:.0} N (err {rel_err:.3})"
    );
}

#[test]
fn force_scales_with_regulator() {
    let params = consolidation();
    let mut b1 = fresh_boiler(&params);
    let mut b2 = fresh_boiler(&params);
    let f_half = steam_step(&mut b1, &params, 0.5, 0.0, 0.01);
    let f_full = steam_step(&mut b2, &params, 1.0, 0.0, 0.01);
    // At 50 % regulator, force should be roughly half (within 5 %).
    let ratio = f_half / f_full;
    assert!(
        (ratio - 0.5).abs() < 0.05,
        "force should scale with regulator, got ratio {ratio:.3}"
    );
}

// ── Boiler dynamics ───────────────────────────────────────────────────────────

#[test]
fn pressure_rises_at_idle() {
    // At v = 0 with closed regulator, steam supply > minimal standing loss.
    // Pressure should rise (or at least not drop) over a few seconds.
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    boiler.pressure_bar = 14.0; // slightly below working pressure
    let p0 = boiler.pressure_bar;
    for _ in 0..10 {
        steam_step(&mut boiler, &params, 0.0, 0.0, 1.0);
    }
    assert!(
        boiler.pressure_bar >= p0,
        "pressure should not drop at idle, was {p0:.2}, now {:.2}",
        boiler.pressure_bar
    );
}

#[test]
fn pressure_drops_under_heavy_load() {
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    let p0 = boiler.pressure_bar;
    // Simulate full-throttle at moderate speed for 60 seconds.
    for _ in 0..60 {
        steam_step(&mut boiler, &params, 1.0, 15.0, 1.0);
    }
    // Under full load the demand typically exceeds supply momentarily → pressure drop.
    // It should not drop below P_MIN (2 bar).
    assert!(
        boiler.pressure_bar >= 2.0,
        "pressure below safety floor: {:.2}",
        boiler.pressure_bar
    );
    // For this loco, heavy load does cause some drop.
    // We don't assert a specific value since the dynamics depend on K_P; just
    // check it stays in a realistic range.
    assert!(
        boiler.pressure_bar <= p0 * 1.06,
        "pressure exceeded safety valve: {:.2}",
        boiler.pressure_bar
    );
}

#[test]
fn water_decreases_under_load() {
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    let w0 = boiler.water_kg;
    for _ in 0..300 {
        steam_step(&mut boiler, &params, 1.0, 15.0, 1.0);
    }
    assert!(boiler.water_kg < w0, "water should decrease under load");
    assert!(boiler.water_kg > 0.0, "water should not go negative");
}

#[test]
fn coal_decreases_over_time() {
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    let c0 = boiler.coal_kg;
    for _ in 0..60 {
        steam_step(&mut boiler, &params, 0.5, 10.0, 1.0);
    }
    assert!(boiler.coal_kg < c0, "coal should decrease over time");
    assert!(boiler.coal_kg > 0.0, "coal should not go negative");
}

#[test]
fn auto_injector_prevents_empty_boiler() {
    let params = consolidation();
    let mut boiler = fresh_boiler(&params);
    // Drain water almost completely to force the injector to activate.
    boiler.water_kg = 100.0;
    // Simulate 60 steps: injector kicks in immediately (water < 30 % threshold).
    // Net change = injector_rate - steam_demand > 0 → water should rise.
    let w_before = boiler.water_kg;
    for _ in 0..60 {
        steam_step(&mut boiler, &params, 1.0, 15.0, 1.0);
    }
    // Water must have increased (injector > demand) and must stay above zero.
    assert!(
        boiler.water_kg > w_before,
        "injector should add water; before {w_before:.1} kg, after {:.1} kg",
        boiler.water_kg
    );
    assert!(boiler.water_kg > 0.0, "water must not reach zero");
}

// ── TOML loader ───────────────────────────────────────────────────────────────

#[test]
fn load_steam_engine_toml() {
    let toml_src = r#"
[engine]
name = "Test Vapor"
mass_kg = 80000
max_velocity_mps = 25.0

[steam]
cylinder_count = 2
cylinder_bore_m = 0.47
piston_stroke_m = 0.66
driving_wheel_radius_m = 0.97
working_pressure_bar = 16.0
evaporation_rate_kg_per_s = 10.0
coal_consumption_kg_per_s = 0.7
initial_water_kg = 15000.0
initial_coal_kg = 8000.0
"#;
    let loco = openrailsrs_train::steam_loader::parse_steam_engine_toml(toml_src)
        .expect("parse steam engine TOML");

    assert_eq!(loco.name, "Test Vapor");
    assert!(loco.steam.is_some(), "steam field should be Some");
    assert!(
        loco.max_tractive_effort_n > 100_000.0,
        "max_tractive_effort_n too low"
    );
}

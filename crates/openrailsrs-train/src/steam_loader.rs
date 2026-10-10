//! TOML-native loader for steam locomotive `.eng` files.
//!
//! MSTS `.eng` files use the S-expression format and are parsed by
//! `openrailsrs-formats`.  However, for *new* steam locomotives defined
//! natively in openrailsrs (e.g. the example under `examples/steam/`), a
//! simpler TOML layout is used:
//!
//! ```toml
//! [engine]
//! name = "Vapor 2-8-0"
//! mass_kg = 80000
//! max_velocity_mps = 27.8
//! max_brake_force_n = 120000.0   # optional
//!
//! [steam]
//! cylinder_count = 2
//! cylinder_bore_m = 0.470
//! piston_stroke_m = 0.660
//! driving_wheel_radius_m = 0.970
//! working_pressure_bar = 16.0
//! evaporation_rate_kg_per_s = 10.0
//! coal_consumption_kg_per_s = 0.7
//! initial_water_kg = 15000.0
//! initial_coal_kg = 8000.0
//! ```
//!
//! The file extension is still `.eng` to keep the consist format consistent.
//! The loader detects whether the file is TOML (starts with `[`) or
//! MSTS S-expression (starts with `(` or has a SIMISA header) and dispatches
//! accordingly.

use std::path::Path;

use serde::Deserialize;

use crate::error::TrainError;
use crate::model::{Locomotive, SteamParams};

// ── TOML schema ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct EngineToml {
    engine: EngineMeta,
    #[serde(default)]
    steam: Option<SteamToml>,
}

#[derive(Deserialize)]
struct EngineMeta {
    name: String,
    mass_kg: f64,
    #[serde(default)]
    wagon_shape: Option<String>,
    #[serde(default = "default_max_velocity")]
    max_velocity_mps: f64,
    #[serde(default = "default_max_brake")]
    max_brake_force_n: f64,
}

#[derive(Deserialize)]
struct SteamToml {
    #[serde(default)]
    operation: openrailsrs_formats::SteamOperatingParams,
    #[serde(default = "default_cylinder_count")]
    cylinder_count: u32,
    cylinder_bore_m: f64,
    piston_stroke_m: f64,
    driving_wheel_radius_m: f64,
    working_pressure_bar: f64,
    evaporation_rate_kg_per_s: f64,
    coal_consumption_kg_per_s: f64,
    initial_water_kg: f64,
    initial_coal_kg: f64,
}

fn default_max_velocity() -> f64 {
    27.8
} // ~100 km/h
fn default_max_brake() -> f64 {
    120_000.0
}
fn default_cylinder_count() -> u32 {
    2
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Load a TOML-format `.eng` file and return a `Locomotive`.
///
/// If the `[steam]` section is present, `locomotive.steam` will be `Some`.
/// `max_power_w` is computed from the steam stall force × a nominal speed
/// so that the P/v fallback (used for the traction curve display) gives a
/// reasonable value.
pub fn load_steam_engine_from_toml(path: impl AsRef<Path>) -> Result<Locomotive, TrainError> {
    let text = std::fs::read_to_string(path.as_ref())?;
    parse_steam_engine_toml(&text)
}

/// Parse a TOML string directly (useful for tests).
pub fn parse_steam_engine_toml(text: &str) -> Result<Locomotive, TrainError> {
    let parsed: EngineToml = toml::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| TrainError::Parse(format!("steam engine TOML parse error: {e}")))?;

    let steam = parsed.steam.map(|s| SteamParams {
        operation: Box::new(s.operation),
        cylinder_count: s.cylinder_count,
        cylinder_bore_m: s.cylinder_bore_m,
        piston_stroke_m: s.piston_stroke_m,
        driving_wheel_radius_m: s.driving_wheel_radius_m,
        working_pressure_bar: s.working_pressure_bar,
        evaporation_rate_kg_per_s: s.evaporation_rate_kg_per_s,
        coal_consumption_kg_per_s: s.coal_consumption_kg_per_s,
        initial_water_kg: s.initial_water_kg,
        initial_coal_kg: s.initial_coal_kg,
    });

    if steam.as_ref().is_some_and(|s| !s.valid())
        || !parsed.engine.mass_kg.is_finite()
        || parsed.engine.mass_kg <= 0.
        || !parsed.engine.max_velocity_mps.is_finite()
        || parsed.engine.max_velocity_mps <= 0.
        || !parsed.engine.max_brake_force_n.is_finite()
        || parsed.engine.max_brake_force_n < 0.
    {
        return Err(TrainError::Parse(
            "invalid steam dimensions, capacities or operating rates".into(),
        ));
    }
    let max_tractive_effort_n = steam
        .as_ref()
        .map(|s| s.max_tractive_effort_n())
        .unwrap_or(0.0);

    // Approximate nominal power from F_te × design speed / 2.
    let max_power_w = max_tractive_effort_n * parsed.engine.max_velocity_mps / 2.0;

    Ok(Locomotive {
        power_supply: Default::default(),
        diesel_operation: None,
        electric: None,
        name: parsed.engine.name,
        mass_kg: parsed.engine.mass_kg,
        max_power_w,
        max_velocity_mps: parsed.engine.max_velocity_mps,
        max_tractive_effort_n,
        max_brake_force_n: parsed.engine.max_brake_force_n,
        tractive_curve: None,
        diesel_traction: None,
        regen_factor: 0.0,
        diesel_sfc_g_per_kwh: None,
        steam,
        wagon_shape: parsed.engine.wagon_shape,
        length_m: 18.0,
        davis: crate::model::DavisCoefficients::default(),
        brake_shoe_type: openrailsrs_formats::OrtsBrakeShoeType::default(),
        brake_shoe_friction: None,
        brake_profile: Default::default(),
        flipped: false,
    })
}

// ── Helpers for consist loading ───────────────────────────────────────────────

/// Detect whether a `.eng` file is in TOML format (vs MSTS S-expression).
///
/// Ignore BOM, blank lines and TOML comments before the first section. Native
/// UTF-16 files remain on the MSTS decoding path.
pub fn is_toml_eng(path: impl AsRef<Path>) -> std::io::Result<bool> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?
        .take(8192)
        .read_to_end(&mut buf)?;
    let Ok(text) = std::str::from_utf8(&buf) else {
        return Ok(false);
    };
    Ok(text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .is_some_and(|line| line.starts_with('[')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commented_example_loads_and_preserves_operating_capacities() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/steam/vehicles/steam_2_8_0.eng");
        assert!(is_toml_eng(&path).unwrap());
        let loco = load_steam_engine_from_toml(path).unwrap();
        let steam = loco.steam.unwrap();
        assert_eq!(steam.operation.boiler_water_capacity_kg, 4000.);
        assert_eq!(steam.initial_water_kg, 15000.);
        assert!(steam.valid());
        assert!(
            loco.wagon_shape.is_none(),
            "example uses generic geometry with no external model dependencies"
        );
    }

    #[test]
    fn bom_and_comments_dispatch_to_toml_but_native_utf16_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engine.eng");
        std::fs::write(&path, "\u{feff}\n# Header longer than the former sixteen-byte probe\n[engine]\nname = 'BOM'\nmass_kg = 80000\n").unwrap();
        assert!(is_toml_eng(&path).unwrap());
        assert_eq!(load_steam_engine_from_toml(&path).unwrap().name, "BOM");
        let mut native = vec![0xff, 0xfe];
        for unit in "SIMISA@@@@@@@@@@JINX0D0t______\nEngine ( Hall )".encode_utf16() {
            native.extend(unit.to_le_bytes());
        }
        std::fs::write(&path, native).unwrap();
        assert!(!is_toml_eng(path).unwrap());
    }

    #[test]
    fn invalid_dimensions_or_resource_capacity_fail_before_starting() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/steam/vehicles/steam_2_8_0.eng");
        let original = std::fs::read_to_string(path).unwrap();
        for invalid in [
            original.replace("0.970   #", "0.0   #"),
            original.replace(
                "boiler_water_capacity_kg = 4000.0",
                "boiler_water_capacity_kg = -1.0",
            ),
        ] {
            assert!(parse_steam_engine_toml(&invalid).is_err());
        }
    }
}

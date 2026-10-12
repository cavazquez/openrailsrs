//! Apply explicit tuning to loaded values only; ENG/WAG/Include files stay intact.
use crate::{Consist, TrainError, Vehicle};
use openrailsrs_core::vehicle_profile::VehicleProfile;
use std::path::Path;
pub fn apply(
    con: &Path,
    consist: &mut Consist,
    profiles: &[VehicleProfile],
) -> Result<bool, TrainError> {
    apply_with_davis(con, consist, profiles, None)
}
pub fn apply_with_davis(
    con: &Path,
    consist: &mut Consist,
    profiles: &[VehicleProfile],
    overlay: Option<&crate::DavisCoefficients>,
) -> Result<bool, TrainError> {
    if profiles.is_empty() {
        return Ok(false);
    }
    let mut baseline = consist.per_vehicle_davis(overlay);
    if let Some(target) = overlay {
        let count = baseline.len().max(1) as f64;
        for d in &mut baseline {
            if consist.davis.a_n.abs() < 1e-9 {
                d.a_n = target.a_n / count;
            }
            if consist.davis.b_n_per_mps.abs() < 1e-9 {
                d.b_n_per_mps = target.b_n_per_mps / count;
            }
            if consist.davis.c_n_per_mps2.abs() < 1e-9 {
                d.c_n_per_mps2 = target.c_n_per_mps2 / count;
            }
        }
    }
    let parsed = openrailsrs_formats::read_msts_file_to_string(con)
        .and_then(|s| openrailsrs_formats::parse_vehicle_text(&s))
        .and_then(|a| openrailsrs_formats::ConsistFile::from_ast(&a))?;
    let mut factors = vec![1.; consist.vehicles.len()];
    let mut changed = false;
    for (index, (entry, vehicle)) in parsed.entries.iter().zip(&mut consist.vehicles).enumerate() {
        let path = crate::resolve_consist_entry_path(crate::consist_asset_root(con), entry.path());
        let identity = crate::consist_audit::source_fingerprint(&path)
            .ok()
            .or_else(|| crate::consist_audit::content_file_sha256(&path).ok())
            .unwrap_or_default();
        let Some(profile) = profiles
            .iter()
            .find(|p| p.valid() && p.source_sha256 == identity)
        else {
            continue;
        };
        let (mass, brake) = match vehicle {
            Vehicle::Loco(l) => (&mut l.mass_kg, &mut l.max_brake_force_n),
            Vehicle::Wagon(w) => (&mut w.mass_kg, &mut w.max_brake_force_n),
        };
        *mass *= profile.mass_factor;
        *brake *= profile.brake_factor;
        factors[index] = profile.resistance_factor;
        changed = true;
    }
    if changed {
        for ((vehicle, base), factor) in consist.vehicles.iter_mut().zip(baseline).zip(factors) {
            let resistance = match vehicle {
                Vehicle::Loco(l) => &mut l.davis,
                Vehicle::Wagon(w) => &mut w.davis,
            };
            *resistance = crate::DavisCoefficients {
                a_n: base.a_n * factor,
                b_n_per_mps: base.b_n_per_mps * factor,
                c_n_per_mps2: base.c_n_per_mps2 * factor,
            };
        }
        consist.davis = consist.vehicles.iter().fold(
            crate::DavisCoefficients {
                a_n: 0.,
                b_n_per_mps: 0.,
                c_n_per_mps2: 0.,
            },
            |sum, v| {
                sum.sum(match v {
                    Vehicle::Loco(l) => &l.davis,
                    Vehicle::Wagon(w) => &w.davis,
                })
            },
        );
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tuning_changes_loaded_physics_only_and_never_follows_another_author_version() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join("vehicles")).unwrap();
        std::fs::create_dir_all(root.join("consists")).unwrap();
        let source = root.join("vehicles/power.eng");
        std::fs::write(&source,"(Engine (Name test) (Type Diesel) (Mass 40000) (MaxPower 1000000) (MaxForce 100000) (MaxBrakeForce 120000) (Davis_A 400) (Davis_B 10) (Davis_C 0.4))").unwrap();
        let con = root.join("consists/test.con");
        std::fs::write(&con, "(Train (Engine \"vehicles/power.eng\"))").unwrap();
        let original = std::fs::read(&source).unwrap();
        let base = crate::load_consist_with_asset_root(&con, root).unwrap();
        let mut tuned = base.clone();
        let hash = crate::consist_audit::source_fingerprint(&source).unwrap();
        let profile = VehicleProfile {
            source_sha256: hash,
            mass_factor: 1.1,
            brake_factor: 0.8,
            resistance_factor: 1.2,
        };
        assert!(apply(&con, &mut tuned, std::slice::from_ref(&profile)).unwrap());
        assert!((tuned.total_mass_kg() - base.total_mass_kg() * 1.1).abs() < 1e-8);
        assert!((tuned.total_max_brake_n() - base.total_max_brake_n() * 0.8).abs() < 1e-8);
        assert!((tuned.davis.a_n - base.davis.a_n * 1.2).abs() < 1e-8);
        assert_eq!(std::fs::read(&source).unwrap(), original);
        let overlay = crate::DavisCoefficients {
            a_n: 123.,
            b_n_per_mps: 4.,
            c_n_per_mps2: 0.2,
        };
        let mut overlaid = base.clone();
        apply_with_davis(
            &con,
            &mut overlaid,
            std::slice::from_ref(&profile),
            Some(&overlay),
        )
        .unwrap();
        assert!((overlaid.davis.a_n - 123. * 1.2).abs() < 1e-8);
        let brake_only = VehicleProfile {
            source_sha256: profile.source_sha256.clone(),
            brake_factor: 0.9,
            ..VehicleProfile::default()
        };
        let mut brakes = base.clone();
        apply_with_davis(&con, &mut brakes, &[brake_only], Some(&overlay)).unwrap();
        assert!((brakes.davis.a_n - overlay.a_n).abs() < 1e-10);
        assert!((brakes.davis.b_n_per_mps - overlay.b_n_per_mps).abs() < 1e-10);
        assert!((brakes.davis.c_n_per_mps2 - overlay.c_n_per_mps2).abs() < 1e-10);
        std::fs::write(
            &source,
            String::from_utf8(original)
                .unwrap()
                .replace("40000", "41000"),
        )
        .unwrap();
        let mut next = crate::load_consist_with_asset_root(&con, root).unwrap();
        assert!(!apply(&con, &mut next, &[profile]).unwrap());
        assert_eq!(next.total_mass_kg(), 41000.);
    }
}

use openrailsrs_sim::brake::{BrakeSystem, vehicle_specs_from_consist};
use openrailsrs_train::{Consist, Vehicle, load_wagon_from_path};

fn formation(file: &str) -> Consist {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/brake_supply_native");
    let ast = openrailsrs_formats::read_vehicle_ast(root.join(file)).unwrap();
    let entries = openrailsrs_formats::ConsistFile::from_ast(&ast).unwrap();
    // These subsystem fixtures omit steam thermodynamics. Read the common
    // wagon/brake fields for engines too, preserving their authored profiles.
    Consist {
        davis: Default::default(),
        vehicles: entries
            .entries
            .iter()
            .map(|entry| Vehicle::Wagon(load_wagon_from_path(root.join(entry.path())).unwrap()))
            .collect(),
    }
}

fn brakes(consist: &Consist) -> BrakeSystem {
    BrakeSystem::from_vehicle_specs(
        &vehicle_specs_from_consist(consist, false, false),
        200.,
        false,
        3.,
    )
}

fn advance(system: &mut BrakeSystem, command: f64, seconds: f64) {
    for _ in 0..(seconds / 0.05).round() as usize {
        system.step(command, 0.05);
    }
}

#[test]
fn native_formations_brake_release_and_resume_without_losing_car_state() {
    for (file, count, ep_only) in [
        ("bristol-pullman-con.con", 8, true),
        ("121single-con.con", 1, false),
        ("1960centralwr8car-con.con", 8, true),
        ("r-stock-6-car-con.con", 6, true),
        ("downton-hall-le-con.con", 2, false),
        ("kingle-con.con", 2, false),
        ("mt-mt-class-47-6-mk2-pp-con.con", 7, false),
    ] {
        let consist = formation(file);
        let mut system = brakes(&consist);
        assert_eq!(system.cylinders.len(), count, "{file}");
        system.precharge(0.);
        advance(&mut system, 0., 10.);
        assert!(system.total_force_n(0.) < 1., "{file}: initial release");
        advance(&mut system, 0.6, 20.);
        let applied = system.total_force_n(0.);
        assert!(applied > 1_000., "{file}: service brake absent");
        if ep_only {
            for (cylinder, spec) in system
                .cylinders
                .iter()
                .zip(vehicle_specs_from_consist(&consist, false, false))
            {
                if spec.profile.electro_pneumatic() == Some(false) {
                    assert!(cylinder.current_force_n < 1., "{file}: charged air trailer");
                }
            }
        }
        let mut resumed: BrakeSystem =
            serde_json::from_str(&serde_json::to_string(&system).unwrap()).unwrap();
        advance(&mut system, 0., 50.);
        advance(&mut resumed, 0., 50.);
        for (original, restored) in system.cylinders.iter().zip(&resumed.cylinders) {
            assert!((original.pressure_bar() - restored.pressure_bar()).abs() < 1e-10);
            assert!((original.current_force_n - restored.current_force_n).abs() < 1e-7);
        }
        assert!(system.total_force_n(0.) < applied * 0.02, "{file}: release");
    }
}

#[test]
fn native_formations_vacuum_cock_isolation_holds_pressure_and_open_hose_applies_brakes() {
    for file in [
        "121single-con.con",
        "downton-hall-le-con.con",
        "kingle-con.con",
    ] {
        let mut system = brakes(&formation(file));
        system.precharge(0.);
        advance(&mut system, 0.6, 20.);
        let held_pressure = system.cylinders[0].pressure_bar();
        let held_force = system.cylinders[0].current_force_n;
        system.cylinders[0].air_isolated = true;
        advance(&mut system, 0., 30.);
        assert_eq!(system.cylinders[0].pressure_bar(), held_pressure, "{file}");
        assert_eq!(system.cylinders[0].current_force_n, held_force, "{file}");
        system.cylinders[0].air_isolated = false;
        advance(&mut system, 0., 50.);
        assert!(
            system.cylinders[0].current_force_n < held_force * 0.02,
            "{file}"
        );
        system.cylinders[0].air_vented = true;
        advance(&mut system, 0., 15.);
        assert!(
            system.cylinders[0].current_force_n > held_force,
            "{file}: open hose"
        );
    }
}

//! The native loader must preserve STF Engine/Wagon roots after resolving files.
use openrailsrs_train::load_consist_native_parameters;

#[test]
fn resolved_consist_driver_radius_keeps_its_engine_scope_and_includes() {
    let temp = tempfile::tempdir().unwrap();
    let stock = temp.path().join("TRAINS/TRAINSET/Scoped");
    let consists = temp.path().join("TRAINS/CONSISTS");
    std::fs::create_dir_all(&stock).unwrap();
    std::fs::create_dir_all(&consists).unwrap();
    std::fs::write(stock.join("Power.eng"),
        "Wagon ( power Mass(25t) WheelRadius(54.6in) ) Engine ( power Include(\"driver.inc\") NumWheels(1) )").unwrap();
    std::fs::write(stock.join("driver.inc"), "WheelRadius(27.3in)").unwrap();
    std::fs::write(
        stock.join("Coach.wag"),
        "Wagon ( coach Mass(20t) WheelRadius(21in) )",
    )
    .unwrap();
    let con = consists.join("scoped.con");
    std::fs::write(
        &con,
        "(Train (Engine \"TRAINSET/scoped/power.eng\") (Wagon \"TRAINSET/scoped/coach.wag\"))",
    )
    .unwrap();
    let parameters =
        load_consist_native_parameters(&con, &temp.path().join("TRAINS"), &[25000., 20000.])
            .unwrap();
    assert_eq!(parameters.len(), 2);
    assert!(
        (parameters[0].wheel_radius_m - 0.69342).abs() < 1e-10,
        "driver radius {}",
        parameters[0].wheel_radius_m
    );
    assert!(
        (parameters[1].wheel_radius_m - 0.5334).abs() < 1e-10,
        "wagon radius {}",
        parameters[1].wheel_radius_m
    );
    assert_eq!(parameters[0].drive_axles, 1.);
}

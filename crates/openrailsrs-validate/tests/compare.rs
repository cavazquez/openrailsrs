use std::io::Write;

use openrailsrs_validate::compare_csv_files;

#[test]
fn identical_runs_zero_diff() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.csv");
    let mut f = std::fs::File::create(&p).unwrap();
    writeln!(
        f,
        "time_s,edge_id,pos_on_edge_m,velocity_mps,odometer_m,cumulative_energy_kwh,throttle,brake"
    )
    .unwrap();
    writeln!(f, "0,e1,0,0,0,0,0,0").unwrap();
    writeln!(f, "0.1,e1,0,1,0.1,0,1,0").unwrap();
    f.flush().unwrap();
    let rep = compare_csv_files(&p, &p).expect("compare");
    assert_eq!(rep.velocity.max_abs_diff, 0.0);
    assert_eq!(rep.position.max_abs_diff, 0.0);
}

#[test]
fn different_runs_have_non_zero_diff() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.csv");
    let b = dir.path().join("b.csv");
    let header = "time_s,edge_id,pos_on_edge_m,velocity_mps,odometer_m,cumulative_energy_kwh,throttle,brake\n";
    std::fs::write(
        &a,
        format!("{header}0,e1,0,10,0,1,1,0\n1,e1,0,12,11,2,1,0\n"),
    )
    .unwrap();
    std::fs::write(
        &b,
        format!("{header}0,e1,0,8,0,0.5,1,0\n1,e1,0,9,9,1,1,0\n"),
    )
    .unwrap();

    let rep = compare_csv_files(&a, &b).expect("compare");
    assert!(rep.velocity.max_abs_diff > 0.0);
    assert!(rep.position.max_abs_diff > 0.0);
    assert!(rep.energy.max_abs_diff > 0.0);
}

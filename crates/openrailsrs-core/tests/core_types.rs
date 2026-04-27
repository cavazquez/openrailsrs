use openrailsrs_core::SimTime;

#[test]
fn sim_time_addition_works() {
    let t = SimTime(1.5) + 0.5;
    assert_eq!(t.seconds(), 2.0);
}

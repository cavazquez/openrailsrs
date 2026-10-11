//! OR 1.6.1 CVF current meters use locomotive force until electrical motor
//! current is modelled. Keep motor effort separate from wheel/rail effort.
#[derive(Clone, Copy, Debug)]
pub enum CurrentMeter {
    Ammeter,
    AbsoluteAmmeter,
    LoadMeter,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct CurrentMeterInput {
    pub motor_force_n: f64,
    pub rail_force_n: f64,
    pub max_force_n: f64,
    pub max_current_a: Option<f64>,
    pub native_axles: bool,
}
pub fn current_meter_a(kind: CurrentMeter, input: CurrentMeterInput, scale_max: f64) -> f64 {
    if input.max_force_n <= 0. || !input.max_force_n.is_finite() {
        return 0.;
    }
    let current = input
        .max_current_a
        .filter(|v| v.is_finite() && *v != 0.)
        .unwrap_or(scale_max);
    let force = match kind {
        CurrentMeter::LoadMeter => {
            if input.motor_force_n > 0. {
                input.rail_force_n.abs()
            } else {
                0.
            }
        }
        _ if input.native_axles => input.motor_force_n.max(0.),
        _ => input.rail_force_n,
    };
    let result = force / input.max_force_n * current;
    match kind {
        CurrentMeter::AbsoluteAmmeter => result.abs(),
        _ => result,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_cab_current_capture() {
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("../../../oracles/openrails-cab.json")).unwrap();
        for row in reference["meters"].as_array().unwrap() {
            let n = |key: &str| row[key].as_f64().unwrap();
            let kind = match row["kind"].as_str().unwrap() {
                "AMMETER" => CurrentMeter::Ammeter,
                "AMMETER_ABS" => CurrentMeter::AbsoluteAmmeter,
                _ => CurrentMeter::LoadMeter,
            };
            let input = CurrentMeterInput {
                motor_force_n: n("motor_force_n"),
                rail_force_n: n("rail_force_n"),
                max_force_n: n("max_force_n"),
                max_current_a: Some(n("current_a")),
                native_axles: n("axles") > 0.,
            };
            assert!(
                (current_meter_a(kind, input, n("scale_max")) - n("result")).abs() < 0.001,
                "{row}"
            );
        }
    }
}

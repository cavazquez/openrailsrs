//! Simple braking-curve helpers for the Rust TCS (#163).

/// Braking distance \(v^2 / 2a\) (m).
pub fn braking_distance_m(speed_mps: f64, deceleration_mps2: f64) -> f64 {
    let v = speed_mps.max(0.0);
    let a = deceleration_mps2.max(0.05);
    (v * v) / (2.0 * a)
}

/// Time to cover `distance_m` at constant `speed_mps` (s).
pub fn time_to_distance_s(distance_m: f64, speed_mps: f64) -> f64 {
    if speed_mps < 0.1 || distance_m <= 0.0 {
        return f64::INFINITY;
    }
    distance_m / speed_mps
}

/// Comfortable service deceleration used for indication / TSM (m/s²).
pub const SERVICE_DECEL_MPS2: f64 = 0.7;

/// Emergency-ish deceleration for intervention margin (m/s²).
pub const EMERGENCY_DECEL_MPS2: f64 = 1.1;

/// Distance at which the driver should start braking for a lower target (CSM→TSM).
pub fn indication_distance_m(speed_mps: f64, target_mps: f64) -> f64 {
    let v0 = speed_mps.max(0.0);
    let v1 = target_mps.max(0.0).min(v0);
    if v0 <= v1 + 0.1 {
        return 0.0;
    }
    (v0 * v0 - v1 * v1) / (2.0 * SERVICE_DECEL_MPS2)
}

/// Allowed speed along a linear braking envelope from `limit` to `target` over `brake_dist`.
pub fn allowed_on_curve(
    distance_to_target_m: f64,
    brake_dist_m: f64,
    limit_mps: f64,
    target_mps: f64,
) -> f64 {
    if brake_dist_m <= 1.0 || distance_to_target_m >= brake_dist_m {
        return limit_mps;
    }
    if distance_to_target_m <= 0.0 {
        return target_mps;
    }
    // v² = v_t² + 2 a s  (remaining distance to target)
    let a = SERVICE_DECEL_MPS2;
    let v_sq = target_mps * target_mps + 2.0 * a * distance_to_target_m;
    v_sq.sqrt().min(limit_mps).max(target_mps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braking_distance_grows_with_speed() {
        let d40 = braking_distance_m(40.0 / 3.6, SERVICE_DECEL_MPS2);
        let d80 = braking_distance_m(80.0 / 3.6, SERVICE_DECEL_MPS2);
        assert!(d80 > d40 * 3.0);
    }

    #[test]
    fn allowed_on_curve_at_target_is_target() {
        let a = allowed_on_curve(0.0, 500.0, 25.0, 10.0);
        assert!((a - 10.0).abs() < 0.01);
    }
}

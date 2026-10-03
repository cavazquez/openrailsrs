//! MSTS interrupted Goode projection, adapted from Open Rails 1.6.1
//! `Orts.Common/WorldLatLon.cs` (Open Rails contributors, GPL-3.0-or-later).

/// Geographic coordinates in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeographicPosition {
    pub latitude: f64,
    pub longitude: f64,
}

/// Convert a native MSTS tile/local position to latitude and longitude.
/// Returns `None` in a projection interruption or outside the map.
#[allow(clippy::approx_constant)] // Preserve the pinned native projection coefficients.
pub fn geographic_position(
    tile_x: i32,
    tile_z: i32,
    local_x: f64,
    local_z: f64,
) -> Option<GeographicPosition> {
    if !local_x.is_finite() || !local_z.is_finite() {
        return None;
    }
    // Native WorldLatLon truncates the local coordinates to whole metres.
    let x = -20_013_965.0 + (f64::from(tile_x) + 16_384.0) * 2048.0 + local_x.trunc();
    let y = 8_674_008.0 - (16_384.0 - f64::from(tile_z)) * 2048.0 + local_z.trunc();
    let r = 6_370_997.0;
    let region = if y >= r * 0.710987989993 {
        if x <= r * -0.698131700798 { 0 } else { 2 }
    } else if y >= 0.0 {
        if x <= r * -0.698131700798 { 1 } else { 3 }
    } else {
        let band = if y >= r * -0.710987989993 { 0 } else { 1 };
        if x <= r * -1.74532925199 {
            4 + 2 * band
        } else if x <= r * -0.349065850399 {
            // Retain native region 5 in both southern bands for compatibility.
            5
        } else if x <= r * 1.3962634016 {
            8 + 2 * band
        } else {
            9 + 2 * band
        }
    };
    let centers = [
        -1.74532925199,
        -1.74532925199,
        0.523598775598,
        0.523598775598,
        -2.79252680319,
        -1.0471975512,
        -2.79252680319,
        -1.0471975512,
        0.349065850399,
        2.44346095279,
        0.349065850399,
        2.44346095279,
    ];
    let center = centers[region];
    let x = x - r * center;
    // MathHelper's constants are single precision in the pinned native engine.
    let pi = f64::from(std::f32::consts::PI);
    let half_pi = f64::from(std::f32::consts::FRAC_PI_2);
    let (latitude, longitude) = if matches!(region, 1 | 3 | 4 | 5 | 8 | 9) {
        let latitude = y / r;
        if latitude.abs() > half_pi {
            return None;
        }
        let raw = if (latitude.abs() - half_pi).abs() > 1e-10 {
            center + x / (r * latitude.cos())
        } else {
            center
        };
        let longitude = if raw.abs() > pi {
            raw - raw.signum() * pi * 2.0
        } else {
            raw
        };
        (latitude, longitude)
    } else {
        let arg =
            (y + 0.0528035274542 * r * if y < 0.0 { -1.0 } else { 1.0 }) / (1.4142135623731 * r);
        if arg.abs() > 1.0 {
            return None;
        }
        let theta = arg.asin();
        let longitude = center + x / (0.900316316158 * r * theta.cos());
        let arg = (2.0 * theta + (2.0 * theta).sin()) / pi;
        if longitude < -pi || arg.abs() > 1.0 {
            return None;
        }
        (arg.asin(), longitude)
    };
    let (min, max) = match region {
        0 | 1 => (-pi, -0.698131700798),
        2 | 3 => (-0.698131700798, pi),
        4 | 6 => (-pi, -1.74532925199),
        5 | 7 => (-1.74532925199, -0.349065850399),
        8 | 10 => (-0.349065850399, 1.3962634016),
        _ => (1.3962634016, pi),
    };
    (longitude >= min && longitude <= max).then_some(GeographicPosition {
        latitude,
        longitude,
    })
}

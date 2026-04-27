//! Minimal geographic helpers — no external geo crate needed.

const EARTH_R_M: f64 = 6_371_000.0;

/// Great-circle distance between two WGS-84 points (Haversine formula), in metres.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    EARTH_R_M * c
}

/// Project (lat, lon) onto a flat plane relative to a reference origin.
///
/// Uses an equirectangular (plate carrée) approximation — accurate to within
/// ~0.2 % for areas up to ~500 km.  Returns `(x_m, y_m)`.
pub fn equirectangular_m(lat: f64, lon: f64, ref_lat: f64, ref_lon: f64) -> (f64, f64) {
    let x_m = (lon - ref_lon).to_radians() * EARTH_R_M * ref_lat.to_radians().cos();
    let y_m = (lat - ref_lat).to_radians() * EARTH_R_M;
    (x_m, y_m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_known_distance() {
        // Vienna (48.2082, 16.3738) to Bratislava (48.1482, 17.1067) ≈ 55 km
        let d = haversine_m(48.2082, 16.3738, 48.1482, 17.1067);
        assert!(
            (d - 55_500.0).abs() < 1000.0,
            "expected ~55.5 km, got {:.1} km",
            d / 1000.0
        );
    }

    #[test]
    fn equirectangular_origin_is_zero() {
        let (x, y) = equirectangular_m(47.0, 15.0, 47.0, 15.0);
        assert!(x.abs() < 1e-6);
        assert!(y.abs() < 1e-6);
    }

    #[test]
    fn equirectangular_north_is_positive_y() {
        let (_, y) = equirectangular_m(48.0, 15.0, 47.0, 15.0);
        assert!(y > 0.0, "north should be +y");
    }

    #[test]
    fn equirectangular_east_is_positive_x() {
        let (x, _) = equirectangular_m(47.0, 16.0, 47.0, 15.0);
        assert!(x > 0.0, "east should be +x");
    }
}

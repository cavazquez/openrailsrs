//! Literal OR1.6.1 TrainCar.UpdateTunnelForce, with static imported tunnel groups.
//! Dimensions are fixed in current vehicle runtime, so the original per-car
//! max-history of width*height equals this authored area whenever active.
//! CarTunnelData was published after the preceding native tick. Recomputing
//! it from the current pre-step pose needs no serializable cache or warmup.
use crate::path_data::PathData;

#[derive(Clone, Copy, Debug)]
pub struct TunnelForceInput {
    pub train_length_m: f64,
    pub train_mass_kg: f64,
    pub car_mass_kg: f64,
    pub car_cross_section_m2: f64,
    pub speed_mps: f64,
}

/// `tunnel_length_m` is the native Ahead+Behind, NOT the full group length:
/// Train.ProcessTunnels subtracts this car's length before their sum.
pub fn force_for_geometry(
    car: &TunnelForceInput,
    tunnel_length_m: f64,
    tunnel_area_m2: f64,
    tunnel_perimeter_m: f64,
) -> f64 {
    let area = tunnel_area_m2 as f32;
    let perimeter = tunnel_perimeter_m as f32;
    let cross = car.car_cross_section_m2 as f32;
    let length = car.train_length_m as f32;
    let total_mass = car.train_mass_kg as f32;
    // Invalid external content must not put NaN/Inf into motion. Valid native
    // inputs retain the literal calculation; unsupported geometry yields no
    // aerodynamic contribution, never a force fitted to a reference sample.
    if [
        car.train_length_m,
        car.train_mass_kg,
        car.car_mass_kg,
        car.car_cross_section_m2,
        car.speed_mps,
        tunnel_length_m,
        tunnel_area_m2,
        tunnel_perimeter_m,
    ]
    .iter()
    .any(|v| !v.is_finite())
        || length <= 0.
        || total_mass <= 0.
        || area <= 0.
        || perimeter < 0.
        || car.car_mass_kg <= 0.
        || cross < 0.
        || cross == area
    {
        return 0.;
    }
    let a = (0.00003318_f32 * 1.2 * area) / ((1. - cross / area) * (1. - cross / area));
    let b = 174.419_f32 * (1. - cross / area) * (1. - cross / area);
    let c = (2.907_f32 * (1. - cross / area) * (1. - cross / area)) / (4. * (area / perimeter));
    let term = b + c * (tunnel_length_m as f32 - length) / length;
    if !term.is_finite() || term < 0. {
        return 0.;
    }
    let temp1 = f64::from(term).sqrt() as f32;
    let temp2 = (1. - 1. / (1. + temp1)) * (1. - 1. / (1. + temp1));
    let drag = ((a * length) / (total_mass / 1000.)) * temp2;
    let speed = car.speed_mps.abs() as f32;
    let force = drag * (car.car_mass_kg as f32 / 1000.) * speed * speed;
    if force.is_finite() && force >= 0. {
        f64::from(force)
    } else {
        0.
    }
}

/// Membership is evaluated at the body's rear in the formation's direction,
/// matching the native full-entry / partial-exit rule on an unsplit interval.
/// The profile is directed; opposite edges carry separately mirrored groups.
pub fn force_at(
    path: &PathData,
    center_m: f64,
    car_length_m: f64,
    path_reversed: bool,
    car: &TunnelForceInput,
) -> f64 {
    let rear = center_m
        + if path_reversed {
            car_length_m / 2.
        } else {
            -car_length_m / 2.
        };
    let mut before = 0.;
    for edge in &path.edges {
        let position = rear - before;
        if position >= 0. && position <= edge.length_m {
            for tunnel in &edge.profile.native_tunnels {
                if (position > tunnel.start_m || tunnel.start_m == 0. && position == 0.)
                    && position <= tunnel.end_m
                {
                    return force_for_geometry(
                        car,
                        tunnel.length_m - car_length_m,
                        tunnel.area_m2,
                        tunnel.perimeter_m,
                    );
                }
            }
        }
        before += edge.length_m;
    }
    0.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_data::PathEdgeData;
    use openrailsrs_track::{EdgePhysicsProfile, tunnel::NativeTunnel};
    #[test]
    fn native_entry_exit_reverse_and_json_recomputation_preserve_metadata() {
        let tunnel = NativeTunnel {
            start_m: 100.,
            end_m: 500.,
            length_m: 400.,
            num_paths: 1,
            area_m2: 70.,
            perimeter_m: 32.,
        };
        let car = TunnelForceInput {
            train_length_m: 80.,
            train_mass_kg: 200000.,
            car_mass_kg: 40000.,
            car_cross_section_m2: 12.,
            speed_mps: 20.,
        };
        let make = |t| PathData {
            edge_ids: vec!["authored".into()],
            edges: vec![PathEdgeData {
                length_m: 1000.,
                profile: EdgePhysicsProfile {
                    native_tunnels: vec![t],
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        let path = make(tunnel.clone());
        assert_eq!(
            force_at(&path, 105., 20., false, &car),
            0.,
            "front alone has entered"
        );
        assert_eq!(
            force_at(&path, 110., 20., false, &car),
            0.,
            "strict native entry boundary"
        );
        let inside = force_at(&path, 111., 20., false, &car);
        assert!(inside > 0.);
        assert_eq!(
            force_at(&path, 509., 20., false, &car),
            inside,
            "front has exited but rear remains in tunnel"
        );
        assert_eq!(force_at(&path, 511., 20., false, &car), 0.);
        let reversed = make(tunnel.mirrored(1000.));
        assert_eq!(force_at(&reversed, 889., 20., true, &car), inside);
        assert_eq!(force_at(&reversed, 489., 20., true, &car), 0.);
        let profile: EdgePhysicsProfile =
            serde_json::from_slice(&serde_json::to_vec(&path.edges[0].profile).unwrap()).unwrap();
        let restored = PathData {
            edge_ids: path.edge_ids.clone(),
            edges: vec![PathEdgeData {
                profile,
                ..path.edges[0].clone()
            }],
        };
        assert_eq!(force_at(&restored, 111., 20., false, &car), inside);
        assert_eq!(force_at(&restored, 511., 20., false, &car), 0.);
    }
    #[test]
    fn unsupported_tunnel_geometry_never_injects_nonfinite_motion() {
        let car = TunnelForceInput {
            train_length_m: 80.,
            train_mass_kg: 200000.,
            car_mass_kg: 40000.,
            car_cross_section_m2: 12.,
            speed_mps: 20.,
        };
        for (length, area, perimeter) in [
            (400., 12., 32.),
            (-100000., 70., 32.),
            (400., f64::INFINITY, 32.),
            (400., 70., f64::NAN),
        ] {
            assert_eq!(force_for_geometry(&car, length, area, perimeter), 0.);
        }
        assert_eq!(
            force_for_geometry(
                &TunnelForceInput {
                    speed_mps: f64::MAX,
                    ..car
                },
                400.,
                70.,
                32.
            ),
            0.
        );
    }
}

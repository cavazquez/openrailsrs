//! Render a native consist sound demonstration without an output device.
use openrailsrs_audio::native::{
    ConsistSoundSpec, SoundFrame, SoundState, TrainSoundFrame, render_oracle,
};
use std::path::PathBuf;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("Usage: native_oracle CONSIST.con ROUTE OUTPUT.wav [cab|exterior]");
        std::process::exit(2);
    }
    let spec = ConsistSoundSpec {
        id: 0,
        consist: PathBuf::from(&args[0]),
        route: PathBuf::from(&args[1]),
    };
    let consist = openrailsrs_train::load_consist_with_asset_root(
        &spec.consist,
        openrailsrs_train::consist_asset_root(&spec.consist),
    )
    .ok();
    let mut total_length = 0.0_f32;
    let electric = consist.as_ref().is_some_and(|c| c.vehicles.iter().any(|v| {
        matches!(v, openrailsrs_train::Vehicle::Loco(l) if l.diesel_sfc_g_per_kwh.is_none() && l.steam.is_none())
    }));
    let offsets: Vec<_> = consist
        .as_ref()
        .map(|c| {
            c.vehicles
                .iter()
                .map(|v| {
                    let length = match v {
                        openrailsrs_train::Vehicle::Loco(l) => l.length_m,
                        openrailsrs_train::Vehicle::Wagon(w) => w.length_m,
                    } as f32;
                    let offset = total_length;
                    total_length += length;
                    offset
                })
                .collect()
        })
        .unwrap_or_default();
    let steam_radius = openrailsrs_train::load_consist_with_asset_root(
        &spec.consist,
        openrailsrs_train::consist_asset_root(&spec.consist),
    )
    .ok()
    .and_then(|c| {
        c.vehicles.into_iter().find_map(|v| match v {
            openrailsrs_train::Vehicle::Loco(l) => l.steam.map(|s| s.driving_wheel_radius_m),
            _ => None,
        })
    });
    let cab = args.get(3).is_some_and(|s| s == "cab");
    let frames: Vec<_> = (0..=1200)
        .map(|i| {
            let seconds = i as f64 / 100.0;
            let throttle = if seconds < 2.0 {
                0.0
            } else if seconds < 7.0 {
                ((seconds - 2.0) / 5.0) as f32
            } else {
                0.0
            };
            SoundFrame {
                time_s: seconds,
                cab,
                volume: 0.4,
                trains: vec![TrainSoundFrame {
                    id: 0,
                    distance_m: if cab { 0.0 } else { 25.0 },
                    vehicle_distances_m: offsets.iter().map(|x| x.hypot(25.0)).collect(),
                    state: SoundState {
                        speed: ((seconds - 2.0).max(0.0) * 2.0) as f32,
                        distance: (seconds * seconds) as f32,
                        variable1: steam_radius.map_or(
                            if electric { throttle * 100.0 } else { throttle },
                            |radius| {
                                if throttle > 0.0 {
                                    ((seconds - 2.0).max(0.0) * 2.0 / radius / std::f64::consts::PI
                                        * 5.0) as f32
                                } else {
                                    0.0
                                }
                            },
                        ),
                        steam_phase: steam_radius
                            .map(|r| seconds * seconds / (std::f64::consts::TAU * r) * 8.0),
                        variable2: if electric || steam_radius.is_some() {
                            throttle * 100.0
                        } else {
                            throttle
                        },
                        throttle,
                        brake: if seconds >= 9.0 { 0.5 } else { 0.0 },
                        brake_pipe: if seconds >= 9.0 { 4.0 } else { 5.0 },
                        brake_cylinder: if seconds >= 9.0 { 30.0 } else { 0.0 },
                        horn: (7.5..8.5).contains(&seconds),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            }
        })
        .collect();
    match render_oracle(&[spec], &frames, &PathBuf::from(&args[2])) {
        Ok(report) => println!("{}", serde_json::to_string_pretty(&report).unwrap()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

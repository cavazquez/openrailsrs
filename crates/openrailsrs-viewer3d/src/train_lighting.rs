//! Authored lamp positions with Bevy spotlights. Some MSTS engines only declare
//! glow sprites; white front lamps receive a physical beam as a Bevy enhancement.
use crate::{
    cab_view::CabLeadVehicle,
    live::{LiveDrive, LiveTrainMarker},
    rolling_stock::TrainConsistScene,
    shapes::RouteAssets,
};
use bevy::light::{NotShadowCaster, NotShadowReceiver, VolumetricLight};
use bevy::prelude::*;
use openrailsrs_formats::{
    ConsistEntry, ConsistFile, VehicleLight, parse_vehicle_lights, parse_vehicle_text,
    read_msts_file_to_string, resolve_path_case_insensitive,
};
use std::path::PathBuf;

#[derive(Component)]
pub struct Headlamp {
    pub position: Vec3,
    pub condition: u8,
    pub beam: bool,
    pub forward: f32,
    pub service_index: usize,
}

fn beam_intensity(level: u8) -> f32 {
    // Bevy 0.19 divides spot intensity by 4π, just like a point light;
    // it does not concentrate lumens into the authored cone's solid angle.
    // Express the railway beam in candela, then convert to Bevy's input.
    let candela = match level {
        2 => 250_000.0,
        1 => 50_000.0,
        _ => 0.0,
    };
    candela * 4.0 * std::f32::consts::PI
}

fn original_engine(con: &std::path::Path, route: &RouteAssets) -> Option<PathBuf> {
    let file =
        ConsistFile::from_ast(&parse_vehicle_text(&read_msts_file_to_string(con).ok()?).ok()?)
            .ok()?;
    let rel = file.entries.into_iter().find_map(|e| match e {
        ConsistEntry::Engine { path, .. } => Some(path),
        _ => None,
    })?;
    let path = openrailsrs_train::resolve_consist_entry_path(
        openrailsrs_train::consist_asset_root(con),
        &rel,
    );
    let folder = path.parent()?.file_name()?.to_str()?;
    for root in crate::shapes::or_content_trainset_roots(&route.route_dir, folder) {
        if let Some(p) = resolve_path_case_insensitive(&root.join(path.file_name()?)) {
            return Some(p);
        }
    }
    Some(path)
}

pub fn lamps_for_level(condition: u8, level: u8) -> bool {
    match condition {
        0 => true,
        1 => level == 0,
        2 => level == 1,
        3 => level == 2,
        4 => level >= 1,
        5 => level != 1,
        6 => level <= 1,
        _ => false,
    }
}

pub fn spawn_train_lights(
    mut commands: Commands,
    consist: Res<TrainConsistScene>,
    live: Res<LiveDrive>,
    route: Res<RouteAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let directory = live
        .scenario_path()
        .parent()
        .unwrap_or(std::path::Path::new("."));
    let mut services = Vec::new();
    if let Some(relative) = &consist.primary_consist_rel {
        services.push((0, "primary".to_string(), directory.join(relative)));
    }
    if let Ok(scenario) = openrailsrs_scenarios::load_scenario(live.scenario_path()) {
        services.extend(
            scenario
                .extra_trains
                .into_iter()
                .enumerate()
                .map(|(i, entry)| (i + 1, entry.id, directory.join(entry.consist))),
        );
    }
    for (service_index, label, con) in services {
        let lights = original_engine(&con, &route)
            .and_then(|p| read_msts_file_to_string(&p).ok())
            .and_then(|t| parse_vehicle_text(&t).ok())
            .map(|a| parse_vehicle_lights(&a))
            .unwrap_or_default();
        let front = consist
            .vehicles_for(&label)
            .first()
            .map_or(10.0, |v| v.length_m * 0.5);
        let lights = if lights.is_empty() {
            vec![
                VehicleLight {
                    cone: false,
                    headlight: 4,
                    unit: 2,
                    position_m: [0.7, 1.8, front],
                    color_rgba: [1.0, 0.95, 0.8, 1.0],
                    radius_m: 0.22,
                    angle_deg: 15.0,
                },
                VehicleLight {
                    cone: false,
                    headlight: 4,
                    unit: 2,
                    position_m: [-0.7, 1.8, front],
                    color_rgba: [1.0, 0.95, 0.8, 1.0],
                    radius_m: 0.22,
                    angle_deg: 15.0,
                },
            ]
        } else {
            lights
        };
        let has_cone = lights.iter().any(|l| l.cone);
        for lamp in lights {
            // Lead unit only. Lamps authored for trailing/intermediate units belong
            // to those vehicles rather than being duplicated on the player's nose.
            if !matches!(lamp.unit, 0 | 2) {
                continue;
            }
            // The car frame already rotates converted shape coordinates into
            // track space. Reflect native Z once, just like the shape vertices.
            let local = Vec3::new(lamp.position_m[0], lamp.position_m[1], -lamp.position_m[2]);
            let color = Color::srgba(
                lamp.color_rgba[0],
                lamp.color_rgba[1],
                lamp.color_rgba[2],
                lamp.color_rgba[3],
            );
            let forward = if local.z >= 0.0 { 1.0 } else { -1.0 };
            if !lamp.cone {
                let material = materials.add(StandardMaterial {
                    base_color: color,
                    unlit: true,
                    fog_enabled: true,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                });
                commands.spawn((
                    Headlamp {
                        position: local,
                        condition: lamp.headlight,
                        beam: false,
                        forward,
                        service_index,
                    },
                    Name::new("authored-lamp-glow"),
                    Mesh3d(meshes.add(Circle::new((lamp.radius_m * 0.12).clamp(0.035, 0.12)))),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            }
            let white =
                lamp.color_rgba[0] > 0.5 && lamp.color_rgba[1] > 0.5 && lamp.color_rgba[2] > 0.4;
            if lamp.cone || (!has_cone && white && local.z < 0.0) {
                let angle = lamp.angle_deg.to_radians().clamp(0.12, 0.6);
                commands.spawn((
                    Headlamp {
                        position: local,
                        condition: lamp.headlight,
                        beam: true,
                        forward,
                        service_index,
                    },
                    Name::new("train-headlight-beam"),
                    SpotLight {
                        intensity: 0.0,
                        color,
                        range: 220.0,
                        inner_angle: angle * 0.55,
                        outer_angle: angle,
                        radius: 0.06,
                        shadow_maps_enabled: true,
                        shadow_depth_bias: 0.015,
                        shadow_normal_bias: 0.04,
                        ..default()
                    },
                    VolumetricLight,
                    Transform::IDENTITY,
                    Visibility::Visible,
                ));
            }
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn update_train_lights(
    live: Res<LiveDrive>,
    settings: Res<crate::player_settings::PlayerSettings>,
    lead: Query<
        (
            &GlobalTransform,
            Option<&crate::rolling_stock_anim::TrainCarTrackOffset>,
        ),
        (With<CabLeadVehicle>, Without<Headlamp>, Without<Camera3d>),
    >,
    traffic_cars: Query<
        (
            &crate::rolling_stock_anim::TrainCarTrackOffset,
            &GlobalTransform,
        ),
        Without<Headlamp>,
    >,
    head: Query<&GlobalTransform, (With<LiveTrainMarker>, Without<Headlamp>, Without<Camera3d>)>,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<Headlamp>)>,
    mut lamps: Query<(
        &Headlamp,
        &mut Transform,
        &mut Visibility,
        Option<&mut SpotLight>,
    )>,
    mut previous_diagnostic: Local<Option<std::time::Instant>>,
) {
    let now = std::time::Instant::now();
    let diagnose = std::env::var_os("OPENRAILSRS_SHADER_DIAGNOSTICS").is_some()
        && previous_diagnostic.is_none_or(|last| now.duration_since(last).as_secs_f32() >= 5.0);
    if diagnose {
        *previous_diagnostic = Some(now);
    }
    let player_base = lead
        .iter()
        .next()
        .map(|(base, offset)| (base, offset.is_some()))
        .or_else(|| head.iter().next().map(|base| (base, false)));
    for (lamp, mut transform, mut visibility, spot) in &mut lamps {
        let (base, authored, session, active) = if lamp.service_index == 0 {
            let Some((base, authored)) = player_base else {
                continue;
            };
            (base, authored, &live.session, true)
        } else {
            let Some(service) = live.traffic.services.get(lamp.service_index - 1) else {
                continue;
            };
            let Some((_, base)) = traffic_cars.iter().find(|(offset, _)| {
                offset.track_index == lamp.service_index && offset.offset_m.abs() < 0.01
            }) else {
                continue;
            };
            (base, true, &service.session, service.departed)
        };
        let (_, rotation, translation) = base.to_scale_rotation_translation();
        let rotation = if authored {
            rotation
        } else {
            rotation * crate::shapes::msts_shape_to_train_rotation()
        };
        let point = translation + rotation * lamp.position;
        let enabled = active
            && lamps_for_level(lamp.condition, session.headlights)
            && session.formation.cars.first().is_none_or(|c| c.battery_on);
        if let Some(mut spot) = spot {
            let direction = (rotation * Vec3::new(0.0, -0.035, lamp.forward)).normalize();
            transform.set_if_neq(Transform::from_translation(point).looking_to(direction, Vec3::Y));
            let intensity = if enabled {
                beam_intensity(session.headlights)
            } else {
                0.0
            };
            let shadows = settings.shadows && enabled;
            if spot.intensity != intensity || spot.shadow_maps_enabled != shadows {
                spot.intensity = intensity;
                spot.shadow_maps_enabled = shadows;
            }
            if diagnose {
                crate::viewer_log!(
                    "headlamp service={} position={point:?} direction={direction:?} lumens={intensity} shadows={shadows}",
                    lamp.service_index
                );
            }
        } else {
            *visibility = if enabled {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            if let Some(camera) = camera.iter().next() {
                *transform =
                    Transform::from_translation(point).looking_at(camera.translation(), Vec3::Y);
            }
        }
    }
}

pub fn update_cab_lighting(
    live: Res<LiveDrive>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    mut materials: ResMut<Assets<crate::or_cab_material::OrCabMaterial>>,
    mut previous: Local<Option<(u32, usize)>>,
) {
    let daylight = sun
        .as_ref()
        .map_or(1.0, |s| (s.direction.y * 2.0).clamp(0.0, 1.0));
    let brightness = (0.08 + daylight * 0.92).max(if live.session.cab_light { 0.75 } else { 0.0 });
    let key = (brightness.to_bits(), materials.len());
    if *previous == Some(key) {
        return;
    }
    *previous = Some(key);
    for (_, material) in materials.iter_mut() {
        if material.params.shader_kind < 4.0 {
            material.params.tint_r = brightness;
            material.params.tint_g = brightness;
            material.params.tint_b = brightness;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn railway_beam_has_useful_illuminance_at_fifty_metres() {
        // A perpendicular target at 50 m receives 100 lux from the high
        // beam before cone/shadow attenuation. Raw 40,000 Bevy lumens gave
        // only 1.27 lux here, leaving the rails indistinguishable from Off.
        let illuminance =
            |level| beam_intensity(level) / (4.0 * std::f32::consts::PI * 50.0 * 50.0);
        assert!((50.0..=150.0).contains(&illuminance(2)));
        assert!((10.0..=30.0).contains(&illuminance(1)));
        assert_eq!(illuminance(0), 0.0);
    }

    #[test]
    fn authored_lamp_frame_points_along_track_and_respects_flip() {
        for (flipped, sign) in [(false, 1.0), (true, -1.0)] {
            let car = crate::shapes::vehicle_authored_frame_transform(0.0, flipped);
            let lamp = car.rotation * Vec3::new(0.7, 1.8, -10.0);
            let beam = car.rotation * Vec3::NEG_Z;
            assert!((lamp.x - sign * 10.0).abs() < 1e-5);
            assert!(beam.dot(Vec3::X * sign) > 0.99999);
        }
    }
    #[test]
    fn bright_only_lamp_does_not_emit_in_dim_or_off() {
        assert!(!lamps_for_level(3, 0));
        assert!(!lamps_for_level(3, 1));
        assert!(lamps_for_level(3, 2));
        assert!(lamps_for_level(2, 1));
        assert!(!lamps_for_level(2, 2));
        assert!(lamps_for_level(1, 0));
        assert!(!lamps_for_level(1, 1));
        assert!(lamps_for_level(4, 2));
        assert!(!lamps_for_level(4, 0));
    }
}

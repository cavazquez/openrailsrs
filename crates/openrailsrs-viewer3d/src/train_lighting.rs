//! Authored lamp positions with Bevy spotlights. Some MSTS engines only declare
//! glow sprites; white front lamps receive a physical beam as a Bevy enhancement.
use crate::{live::LiveDrive, rolling_stock::TrainConsistScene};
use bevy::light::{NotShadowCaster, NotShadowReceiver, VolumetricLight};
use bevy::prelude::*;
use openrailsrs_formats::{VehicleLight, parse_vehicle_lights};

#[derive(Component)]
pub struct Headlamp {
    pub position: Vec3,
    /// Active off/low/high bits; physical fallback beams may fill a missing level.
    pub level_mask: u8,
    pub beam: bool,
    pub forward: f32,
    pub service_index: usize,
    pub vehicle_index: usize,
    pub authored: VehicleLight,
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

fn lamp_mask(condition: u8) -> u8 {
    (0..=2).fold(0, |mask, level| {
        mask | (u8::from(lamps_for_level(condition, level)) << level)
    })
}

fn white_front(lamp: &VehicleLight) -> bool {
    lamp.position_m[2] > 0.0
        && lamp.color_rgba[0] > 0.5
        && lamp.color_rgba[1] > 0.5
        && lamp.color_rgba[2] > 0.4
}

fn beam_mask(lamp: &VehicleLight, lights: &[VehicleLight]) -> u8 {
    if lamp.cone {
        return authored_mask(lamp);
    }
    if !white_front(lamp) || authored_mask(lamp) & 0b110 == 0 {
        return 0;
    }
    // A high-only white sprite (e.g. Pullman) still provides a usable dipped
    // beam. An authored front cone suppresses fallback only at its own levels.
    let authored = lights
        .iter()
        .filter(|l| l.cone && matches!(l.unit, 0 | 2) && l.position_m[2] > 0.0)
        .fold(0, |mask, l| mask | authored_mask(l));
    0b110 & !authored
}

pub fn spawn_train_lights(
    mut commands: Commands,
    consist: Res<TrainConsistScene>,
    live: Res<LiveDrive>,
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
        let _ = con;
        for (vehicle_index, visual) in consist.vehicles_for(&label).iter().enumerate() {
            let lights = visual
                .stock_file
                .as_ref()
                .and_then(|p| openrailsrs_formats::read_vehicle_ast(p).ok())
                .map(|a| parse_vehicle_lights(&a))
                .unwrap_or_default();
            let front = visual.length_m * 0.5;
            let engine = visual
                .stock_file
                .as_ref()
                .is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("eng")));
            let lights = if lights.is_empty() && engine {
                vec![
                    VehicleLight {
                        cone: false,
                        headlight: 4,
                        unit: 2,
                        position_m: [0.7, 1.8, front],
                        color_rgba: [1.0, 0.95, 0.8, 1.0],
                        radius_m: 0.22,
                        angle_deg: 15.0,
                        states: vec![],
                        cycle: false,
                        conditions: vec![],
                        condition_sets: vec![],
                    },
                    VehicleLight {
                        cone: false,
                        headlight: 4,
                        unit: 2,
                        position_m: [-0.7, 1.8, front],
                        color_rgba: [1.0, 0.95, 0.8, 1.0],
                        radius_m: 0.22,
                        angle_deg: 15.0,
                        states: vec![],
                        cycle: false,
                        conditions: vec![],
                        condition_sets: vec![],
                    },
                ]
            } else {
                lights
            };
            let mut fallback_positions: Vec<(Vec3, u8)> = Vec::new();
            for lamp in &lights {
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
                let mut physical_mask = beam_mask(lamp, &lights);
                if !lamp.cone {
                    for (position, mask) in &fallback_positions {
                        if position.distance(local) < 0.05 {
                            physical_mask &= !mask;
                        }
                    }
                    if physical_mask != 0 {
                        fallback_positions.push((local, physical_mask));
                    }
                }
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
                            level_mask: authored_mask(lamp) | physical_mask,
                            beam: false,
                            forward,
                            service_index,
                            vehicle_index,
                            authored: lamp.clone(),
                        },
                        Name::new("authored-lamp-glow"),
                        Mesh3d(meshes.add(Circle::new(1.0))),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        Visibility::Hidden,
                        NotShadowCaster,
                        NotShadowReceiver,
                    ));
                }
                if physical_mask != 0 {
                    let angle = lamp.angle_deg.to_radians().clamp(0.12, 0.6);
                    commands.spawn((
                        Headlamp {
                            position: local,
                            level_mask: physical_mask,
                            beam: true,
                            forward,
                            service_index,
                            vehicle_index,
                            authored: lamp.clone(),
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
}

/// OR's first/last and flipped-car tests, evaluated for the current coupled group.
pub fn unit_enabled(unit: u8, index: usize, count: usize, flipped: bool, reversed: bool) -> bool {
    if count == 0 || index >= count {
        return false;
    }
    let first = index == if reversed { count - 1 } else { 0 };
    let last = index == if reversed { 0 } else { count - 1 };
    let reversed = flipped ^ reversed;
    match unit {
        0 => true,
        1 => !first && !last,
        2 => first && !reversed,
        3 => last && !reversed,
        4 => last && reversed,
        5 => first && reversed,
        _ => false,
    }
}
struct LampContext<'a> {
    session: &'a openrailsrs_sim::LiveDriveSession,
    index: usize,
    count: usize,
    flipped: bool,
    reversed: bool,
    player: bool,
    powered: bool,
    front_coupled: bool,
    rear_coupled: bool,
    day: bool,
    rain: bool,
    snow: bool,
}
impl LampContext<'_> {
    fn condition(&self, name: &str, value: u8, level: u8, fallback: bool) -> bool {
        if value == 0 {
            return true;
        }
        let s = self.session;
        let doors = matches!(
            s.exterior.door,
            openrailsrs_sim::DoorState::Opening | openrailsrs_sim::DoorState::Open
        );
        let direction = if s.driver_direction >= 0.75 {
            1
        } else if s.driver_direction <= 0.25 {
            -1
        } else {
            0
        };
        let direction = if self.flipped { -direction } else { direction };
        let cab = s.cab_vehicle();
        match (name, value) {
            ("headlight", _) => {
                lamps_for_level(value, level) || (fallback && level == 1 && value == 3)
            }
            ("unit", _) => unit_enabled(value, self.index, self.count, self.flipped, self.reversed),
            ("coupling", 1) => self.front_coupled && !self.rear_coupled,
            ("coupling", 2) => !self.front_coupled && self.rear_coupled,
            ("coupling", 3) => self.front_coupled && self.rear_coupled,
            ("battery", 1) => self.powered,
            ("battery", 2) => !self.powered,
            ("control", 1) => !self.player,
            ("control", 2) => self.player,
            ("service", 1) => self.index >= self.count,
            ("service", 2) => self.index < self.count,
            ("penalty", 1) => !s.state.brake_system.emergency,
            ("penalty", 2) => s.state.brake_system.emergency,
            ("timeofday", 1) => self.day,
            ("timeofday", 2) => !self.day,
            ("weather", 1) => !self.rain && !self.snow,
            ("weather", 2) => self.rain,
            ("weather", 3) => self.snow,
            ("brake", 1) => s
                .state
                .brake_system
                .cylinders
                .get(self.index)
                .is_none_or(|c| c.pressure_bar() < 0.1),
            ("brake", 2) => s
                .state
                .brake_system
                .cylinders
                .get(self.index)
                .is_some_and(|c| c.pressure_bar() >= 0.1),
            ("reverser", 1) => direction > 0,
            ("reverser", 2) => direction < 0,
            ("reverser", 3) => direction == 0,
            ("reverser", 4) => direction != 0,
            ("reverser", 5) => direction >= 0,
            ("reverser", 6) => direction <= 0,
            ("doors", 1) => !doors,
            ("doors", 2..=5) => doors,
            ("horn", 1) => !s.horn_active(),
            ("horn", 2) => s.horn_active(),
            ("bell", 1) => true,
            ("bell", 2) => false,
            ("mu", 1) => self.index == cab,
            ("mu", 2) => {
                self.index == cab
                    || s.formation
                        .cars
                        .get(self.index)
                        .is_some_and(|c| !c.mu_connected)
            }
            ("mu", 3) => {
                self.index != cab
                    && s.formation
                        .cars
                        .get(self.index)
                        .is_some_and(|c| c.mu_connected)
            }
            _ => false,
        }
    }
    fn matches(&self, lamp: &VehicleLight, level: u8, fallback: bool) -> bool {
        if lamp.condition_sets.is_empty() {
            return self.condition("headlight", lamp.headlight, level, fallback)
                && self.condition("unit", lamp.unit, level, fallback)
                && lamp
                    .conditions
                    .iter()
                    .all(|(n, v)| self.condition(n, *v, level, fallback));
        }
        lamp.condition_sets.iter().any(|set| {
            set.iter()
                .all(|(n, v)| self.condition(n, *v, level, fallback))
        })
    }
}
fn authored_mask(lamp: &VehicleLight) -> u8 {
    if lamp.condition_sets.is_empty() {
        return lamp_mask(lamp.headlight);
    }
    lamp.condition_sets.iter().fold(0, |mask, set| {
        mask | lamp_mask(
            set.iter()
                .find(|(n, _)| n == "headlight")
                .map_or(0, |(_, v)| *v),
        )
    })
}

#[derive(Debug, PartialEq)]
struct LampState {
    position: [f32; 3],
    color: [f32; 4],
    angle: f32,
    azimuth: f32,
    elevation: f32,
    radius: f32,
}
fn lamp_state(lamp: &VehicleLight, time: f64) -> LampState {
    let baseline = || LampState {
        position: lamp.position_m,
        color: lamp.color_rgba,
        angle: lamp.angle_deg,
        azimuth: 0.,
        elevation: 0.,
        radius: lamp.radius_m,
    };
    if lamp.states.is_empty() {
        return baseline();
    }
    let n = lamp.states.len();
    let sequence_len = n + if lamp.cycle && n > 2 { n - 2 } else { 0 };
    let at = |i: usize| if i < n { i } else { 2 * n - 2 - i };
    let duration: f64 = (0..sequence_len)
        .map(|i| f64::from(lamp.states[at(i)].duration_s))
        .sum();
    let mut remaining = time.max(0.).rem_euclid(duration.max(0.001));
    for i in 0..sequence_len {
        let state = &lamp.states[at(i)];
        if remaining < f64::from(state.duration_s) {
            let next = &lamp.states[at((i + 1) % sequence_len)];
            let t = if state.transition {
                (remaining / f64::from(state.duration_s)) as f32
            } else {
                0.
            };
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            return LampState {
                position: std::array::from_fn(|i| lerp(state.position_m[i], next.position_m[i])),
                color: std::array::from_fn(|i| lerp(state.color_rgba[i], next.color_rgba[i])),
                angle: lerp(state.angle_deg, next.angle_deg),
                azimuth: lerp(state.azimuth_deg, next.azimuth_deg),
                elevation: lerp(state.elevation_deg, next.elevation_deg),
                radius: lerp(state.radius_m, next.radius_m),
            };
        }
        remaining -= f64::from(state.duration_s);
    }
    baseline()
}

#[allow(clippy::type_complexity)]
pub fn update_train_lights(
    live: Res<LiveDrive>,
    settings: Res<crate::player_settings::PlayerSettings>,
    _cab: Res<crate::camera::LiveDriverCab>,
    quality: Option<Res<crate::adaptive_graphics::AdaptiveGraphics>>,
    atmosphere: (
        Option<Res<crate::route_lighting::RouteSunState>>,
        Option<Res<crate::weather_state::WeatherState>>,
    ),
    cars: Query<
        (
            &crate::rolling_stock_anim::TrainCarTrackOffset,
            &crate::rolling_stock::ConsistCarIndex,
            &GlobalTransform,
        ),
        Without<Headlamp>,
    >,
    camera: Query<&GlobalTransform, (With<Camera3d>, Without<Headlamp>)>,
    mut lamps: Query<(
        Entity,
        &Headlamp,
        &mut Transform,
        &mut Visibility,
        Option<&mut SpotLight>,
        Option<&MeshMaterial3d<StandardMaterial>>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let observer = camera.iter().next().map_or(Vec3::ZERO, |c| c.translation());
    let limit = if quality.as_ref().is_some_and(|q| q.level >= 2) {
        2
    } else if quality.as_ref().is_some_and(|q| q.level == 1) {
        4
    } else {
        8
    };
    let mut ranked = lamps
        .iter()
        .filter_map(|(entity, lamp, _, _, spot, _)| {
            spot?;
            let (_, _, pose) = cars.iter().find(|(o, i, _)| {
                o.track_index == lamp.service_index && i.0 == lamp.vehicle_index
            })?;
            let distance = pose.translation().distance_squared(observer);
            (lamp.service_index == 0 || distance < 300f32.powi(2)).then_some((
                entity,
                lamp.service_index == 0,
                distance,
            ))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.2.total_cmp(&b.2))
            .then(a.0.to_bits().cmp(&b.0.to_bits()))
    });
    ranked.truncate(limit);
    for (entity, lamp, mut transform, mut visibility, spot, material) in &mut lamps {
        let session = if lamp.service_index == 0 {
            Some(&live.session)
        } else {
            live.traffic
                .services
                .get(lamp.service_index - 1)
                .filter(|s| s.departed)
                .map(|s| &s.session)
        };
        let Some(session) = session else {
            *visibility = Visibility::Hidden;
            if let Some(mut s) = spot {
                s.intensity = 0.;
            }
            continue;
        };
        let Some((offset, _, base)) = cars
            .iter()
            .find(|(o, i, _)| o.track_index == lamp.service_index && i.0 == lamp.vehicle_index)
        else {
            *visibility = Visibility::Hidden;
            if let Some(mut s) = spot {
                s.intensity = 0.;
            }
            continue;
        };
        let index = lamp.vehicle_index;
        let count = session.formation.coupled_count;
        let reversed = session.driver_direction <= 0.25;
        let level = session.effective_headlights().min(2);
        let powered = session
            .state
            .power_supply
            .cars
            .iter()
            .find(|c| c.vehicle == index)
            .map_or_else(
                || {
                    session
                        .formation
                        .cars
                        .get(index)
                        .is_some_and(|c| c.battery_on)
                },
                |c| c.supply.low_voltage,
            );
        let mut enabled = powered && index < count && lamp.level_mask & (1 << level) != 0;
        // A Bevy fallback beam only illuminates an exposed end, never a coupled nose.
        let front_coupled = if offset.flipped {
            index + 1 < count
        } else {
            index > 0
        };
        let rear_coupled = if offset.flipped {
            index > 0
        } else {
            index + 1 < count
        };
        if lamp.beam {
            enabled &= if lamp.position.z < 0. {
                !front_coupled
            } else {
                !rear_coupled
            };
        }
        let context = LampContext {
            session,
            index,
            count,
            flipped: offset.flipped,
            reversed,
            player: lamp.service_index == 0,
            powered,
            front_coupled,
            rear_coupled,
            day: atmosphere.0.as_ref().is_none_or(|s| s.direction.y > 0.),
            rain: atmosphere
                .1
                .as_ref()
                .is_some_and(|w| w.atmosphere.rain > 0.01),
            snow: atmosphere
                .1
                .as_ref()
                .is_some_and(|w| w.atmosphere.snow > 0.01),
        };
        enabled &= context.matches(
            &lamp.authored,
            level,
            !lamp.authored.cone && white_front(&lamp.authored),
        );
        let LampState {
            position,
            color: rgba,
            angle,
            azimuth,
            elevation,
            radius,
        } = lamp_state(&lamp.authored, session.time_s());
        let color = Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let rotation = base.rotation();
        let point = base.transform_point(Vec3::new(position[0], position[1], -position[2]));
        if let Some(mut spot) = spot {
            enabled &= ranked.iter().any(|(e, _, _)| *e == entity);
            *visibility = Visibility::Visible;
            let low = level == 1;
            let aim = Quat::from_rotation_y(-azimuth.to_radians())
                * Quat::from_rotation_x(elevation.to_radians());
            let direction =
                (rotation * aim * Vec3::new(0., if low { -0.055 } else { -0.035 }, lamp.forward))
                    .normalize();
            *transform = Transform::from_translation(point).looking_to(direction, Vec3::Y);
            spot.intensity = if enabled {
                beam_intensity(level) * rgba[3]
            } else {
                0.
            };
            spot.color = color;
            spot.range = if low { 120. } else { 220. };
            spot.outer_angle = angle.to_radians().clamp(0.12, 1.2);
            spot.inner_angle = spot.outer_angle * 0.55;
            spot.shadow_maps_enabled = settings.shadows
                && enabled
                && rgba[3] > 0.001
                && ranked.iter().take(4).any(|(e, _, _)| *e == entity);
        } else {
            *visibility = if enabled && rgba[3] > 0.001 {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
            if let Some(camera) = camera.iter().next() {
                *transform =
                    Transform::from_translation(point).looking_at(camera.translation(), Vec3::Y);
                transform.rotate_y(std::f32::consts::PI);
                transform.scale = Vec3::splat((radius * 0.12).clamp(0.035, 0.12));
            }
            if let Some(mut m) = material.and_then(|h| materials.get_mut(&h.0))
                && m.base_color != color
            {
                m.base_color = color;
            }
        }
    }
}

pub fn update_cab_lighting(
    live: Res<LiveDrive>,
    sun: Option<Res<crate::route_lighting::RouteSunState>>,
    cab: Res<crate::cab_view::CabInteriorState>,
    cvf: Res<crate::cab_cvf::CabCvfState>,
    settings: Res<crate::player_settings::PlayerSettings>,
    mut materials: ResMut<Assets<crate::or_cab_material::OrCabMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut previous: Local<Option<(u32, usize, usize, usize, [u32; 3])>>,
) {
    let daylight = sun
        .as_ref()
        .map_or(1.0, |s| (s.direction.y * 2.0).clamp(0.0, 1.0));
    let cab_light = live.session.effective_cab_light();
    let brightness = (0.08 + daylight * 0.92).max(if cab_light { 0.75 } else { 0.0 });
    let profile = cvf
        .cvf_path
        .as_ref()
        .map(|p| settings.cab_profile(p))
        .unwrap_or_default();
    let levels = [
        profile.instrument_light,
        profile.console_light,
        profile.interior_light,
    ];
    let zones = levels
        .map(|level| (0.08 + daylight * 0.92).max(if cab_light { 0.75 * level } else { 0.0 }));
    let variation = if crate::shapes::cab_night_textures_enabled() {
        if cab_light { 2 } else { 1 }
    } else {
        0
    };
    let key = (
        brightness.to_bits(),
        materials.len(),
        cab.texture_lighting.len(),
        variation,
        levels.map(f32::to_bits),
    );
    if *previous == Some(key) {
        return;
    }
    *previous = Some(key);
    let bound_materials: std::collections::HashSet<_> = cab
        .texture_lighting
        .iter()
        .filter_map(|binding| binding.or_material.as_ref().map(Handle::id))
        .collect();
    // Assets::iter_mut marks every visited asset as modified. Inspect first so
    // unchanged FullBright artwork is not re-extracted and uploaded each frame.
    let changed_materials: Vec<_> = materials
        .iter()
        .filter(|(id, material)| {
            !bound_materials.contains(id)
                && material.params.shader_kind < 4.0
                && [
                    material.params.tint_r,
                    material.params.tint_g,
                    material.params.tint_b,
                ] != [brightness; 3]
        })
        .map(|(id, _)| id)
        .collect();
    for id in changed_materials {
        if let Some(mut material) = materials.get_mut(id) {
            material.params.tint_r = brightness;
            material.params.tint_g = brightness;
            material.params.tint_b = brightness;
        }
    }
    apply_cab_texture_lighting(
        &cab.texture_lighting,
        variation,
        zones,
        &mut materials,
        &mut standard,
    );
}

fn apply_cab_texture_lighting(
    bindings: &[crate::cab_view::CabTextureBinding],
    variation: usize,
    brightness: [f32; 3],
    materials: &mut Assets<crate::or_cab_material::OrCabMaterial>,
    standard: &mut Assets<StandardMaterial>,
) {
    for binding in bindings {
        let brightness = brightness[binding.zone.min(2)];
        let (texture, tint) = &binding.variants[variation];
        if let Some(mut material) = binding
            .or_material
            .as_ref()
            .and_then(|h| materials.get_mut(h))
        {
            if material.base_texture != *texture {
                material.base_texture = texture.clone();
            }
            // Preserve authored FullBright day/night artwork; its switchable
            // CABLIGHT illumination follows the selected zone intensity.
            let brightness = if material.params.shader_kind < 4.0 || variation == 2 {
                brightness
            } else {
                1.0
            };
            let rgb = [
                tint.red * brightness,
                tint.green * brightness,
                tint.blue * brightness,
            ];
            if [
                material.params.tint_r,
                material.params.tint_g,
                material.params.tint_b,
            ] != rgb
            {
                [
                    material.params.tint_r,
                    material.params.tint_g,
                    material.params.tint_b,
                ] = rgb;
            }
        } else if let Some(mut material) = standard.get_mut(&binding.standard_material) {
            if material.base_color_texture.as_ref() != Some(texture) {
                material.base_color_texture = Some(texture.clone());
            }
            let colour = Color::linear_rgba(
                tint.red * brightness,
                tint.green * brightness,
                tint.blue * brightness,
                tint.alpha,
            );
            if material.base_color != colour {
                material.base_color = colour;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_lamp(cone: bool, condition: u8, front: bool, white: bool) -> VehicleLight {
        VehicleLight {
            cone,
            headlight: condition,
            unit: 2,
            position_m: [0.7, 1.8, if front { 10.0 } else { -10.0 }],
            color_rgba: if white {
                [1.0; 4]
            } else {
                [1.0, 0.0, 0.0, 1.0]
            },
            radius_m: 0.22,
            angle_deg: 15.0,
            states: vec![],
            cycle: false,
            conditions: vec![],
            condition_sets: vec![],
        }
    }
    #[test]
    fn pullman_high_only_white_lamps_also_offer_a_dipped_beam() {
        let white = test_lamp(false, 3, true, true);
        let red = test_lamp(false, 2, true, false);
        let lights = [white.clone(), red.clone()];
        assert_eq!(beam_mask(&white, &lights), 0b110);
        assert_eq!(beam_mask(&red, &lights), 0);
        assert!(beam_intensity(1) > 0.0 && beam_intensity(1) < beam_intensity(2));
    }
    #[test]
    fn authored_cones_replace_fallback_only_for_the_levels_and_end_they_cover() {
        let glow = test_lamp(false, 4, true, true);
        let high = test_lamp(true, 3, true, true);
        assert_eq!(beam_mask(&glow, &[glow.clone(), high.clone()]), 0b010);
        assert_eq!(beam_mask(&high, std::slice::from_ref(&high)), 0b100);
        let rear = test_lamp(true, 4, false, true);
        assert_eq!(beam_mask(&glow, &[glow.clone(), rear]), 0b110);
    }
    #[test]
    fn cab_light_switches_cached_native_textures_without_changing_other_materials() {
        use crate::cab_view::CabTextureBinding;
        use crate::or_cab_material::OrCabMaterial;
        let mut materials = Assets::<OrCabMaterial>::default();
        let mut standard = Assets::<StandardMaterial>::default();
        let mut images = Assets::<Image>::default();
        let [day, night, light] = std::array::from_fn(|_| images.add(Image::default()));
        let fullbright = crate::or_cab_material::create_or_cab_material(
            &mut materials,
            day.clone(),
            Color::WHITE,
            AlphaMode::Opaque,
            Some("Tex"),
            None,
        );
        let unrelated = standard.add(StandardMaterial::default());
        let bound = standard.add(StandardMaterial::default());
        let tint = LinearRgba::new(0.7, 0.6, 0.5, 0.4);
        let variants = [
            (day.clone(), tint),
            (night.clone(), tint),
            (light.clone(), tint),
        ];
        let bindings = [
            CabTextureBinding {
                zone: 2,
                or_material: Some(fullbright.clone()),
                standard_material: unrelated.clone(),
                variants: variants.clone(),
            },
            CabTextureBinding {
                zone: 2,
                or_material: None,
                standard_material: bound.clone(),
                variants,
            },
        ];
        for (index, image, brightness) in [
            (1, &night, 0.08),
            (2, &light, 0.75),
            (0, &day, 1.0),
            (1, &night, 0.08),
        ] {
            apply_cab_texture_lighting(
                &bindings,
                index,
                [brightness; 3],
                &mut materials,
                &mut standard,
            );
            let cab = materials.get(&fullbright).unwrap();
            assert_eq!(&cab.base_texture, image);
            assert!(
                (cab.params.tint_r - 0.7 * if index == 2 { brightness } else { 1.0 }).abs() < 1e-6,
                "native FullBright was dimmed"
            );
            let fallback = standard.get(&bound).unwrap();
            assert_eq!(fallback.base_color_texture.as_ref(), Some(image));
            assert!((fallback.base_color.to_linear().alpha - 0.4).abs() < 1e-6);
            assert!((fallback.base_color.to_linear().red - brightness * 0.7).abs() < 1e-6);
            assert_eq!(standard.get(&unrelated).unwrap().base_color, Color::WHITE);
        }
        assert_eq!(images.len(), 3, "a light toggle allocated a new texture");
    }
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

    #[test]
    fn plan30_tail_conditions_follow_flips_reversal_and_current_coupled_end() {
        assert!(unit_enabled(3, 7, 8, false, false));
        assert!(!unit_enabled(3, 2, 8, false, false));
        assert!(unit_enabled(3, 2, 3, false, false));
        assert!(!unit_enabled(3, 7, 3, false, false));
        assert!(unit_enabled(4, 7, 8, true, false));
        assert!(unit_enabled(3, 0, 8, true, true));
        assert!(unit_enabled(5, 7, 8, false, true));
    }
    #[test]
    fn plan30_original_alternative_conditions_and_state_transitions_are_preserved() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/scenario.toml");
        let drive = LiveDrive::from_scenario_path(&path).unwrap();
        let ast=openrailsrs_formats::parse_named_stf("Wagon ( Lights ( 1 Light ( Conditions ( Unit ( 2 ) Headlight ( 3 ) ) Conditions ( Unit ( 2 ) Headlight ( 2 ) Weather ( 2 ) ) States ( 2 State ( Duration ( 2 ) Position ( 1 2 10 ) LightColour ( ffffffff ) Transition ( 1 ) ) State ( Duration ( 2 ) Position ( 3 2 10 ) LightColour ( 00ffffff ) ) ) ) ) )").unwrap();
        let lamps = parse_vehicle_lights(&ast);
        let lamp = &lamps[0];
        assert_eq!(lamp.condition_sets.len(), 2);
        assert_eq!(authored_mask(lamp), 0b110);
        let mut context = LampContext {
            session: &drive.session,
            index: 0,
            count: 2,
            flipped: false,
            reversed: false,
            player: true,
            powered: true,
            front_coupled: false,
            rear_coupled: true,
            day: true,
            rain: false,
            snow: false,
        };
        assert!(context.matches(lamp, 2, false));
        assert!(!context.matches(lamp, 1, false));
        context.rain = true;
        assert!(context.matches(lamp, 1, false));
        let state = lamp_state(lamp, 1.);
        assert_eq!(state.position, [2., 2., 10.]);
        assert!((state.color[3] - 0.5).abs() < 1e-6);
        assert_eq!(lamp_state(lamp, 1.), lamp_state(lamp, 5.));
    }
    #[test]
    fn plan30_beams_respond_to_actual_power_without_moving_authored_origin() {
        use bevy::ecs::system::RunSystemOnce;
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/smoke/scenario.toml");
        let mut drive = LiveDrive::from_scenario_path(&path).unwrap();
        drive.session.headlights = 1;
        drive.session.driver_direction = 1.;
        let mut app = App::new();
        app.insert_resource(drive)
            .init_resource::<crate::player_settings::PlayerSettings>()
            .init_resource::<crate::camera::LiveDriverCab>()
            .init_resource::<Assets<StandardMaterial>>();
        app.world_mut().spawn((
            crate::rolling_stock_anim::TrainCarTrackOffset {
                offset_m: 0.,
                track_index: 0,
                flipped: false,
            },
            crate::rolling_stock::ConsistCarIndex(0),
            GlobalTransform::from_translation(Vec3::new(100., 0., 200.)),
        ));
        let lamp = test_lamp(false, 4, true, true);
        let beam = app
            .world_mut()
            .spawn((
                Headlamp {
                    position: Vec3::new(0.7, 1.8, -10.),
                    level_mask: 0b110,
                    beam: true,
                    forward: -1.,
                    service_index: 0,
                    vehicle_index: 0,
                    authored: lamp,
                },
                Transform::IDENTITY,
                Visibility::Visible,
                SpotLight::default(),
            ))
            .id();
        app.world_mut()
            .run_system_once(update_train_lights)
            .unwrap();
        assert!(app.world().get::<SpotLight>(beam).unwrap().intensity > 0.);
        let point = app.world().get::<Transform>(beam).unwrap().translation;
        assert!(point.abs_diff_eq(Vec3::new(100.7, 1.8, 190.), 1e-5));
        {
            let mut live = app.world_mut().resource_mut::<LiveDrive>();
            live.session.formation.cars[0].battery_on = false;
            for c in &mut live.session.state.power_supply.cars {
                c.supply.low_voltage = false;
            }
        }
        app.world_mut()
            .run_system_once(update_train_lights)
            .unwrap();
        assert_eq!(app.world().get::<SpotLight>(beam).unwrap().intensity, 0.);
        assert_eq!(
            app.world().get::<Transform>(beam).unwrap().translation,
            point
        );
    }
}

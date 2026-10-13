//! Declared vehicle equipment; absent blocks preserve native legacy behaviour.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VehicleSystemsDef {
    pub vehicle: usize,
    pub doors: Option<DoorsDef>,
    pub tilt: Option<TiltDef>,
    pub crosswind: Option<CrosswindDef>,
    pub electric_brake: Option<ElectricBrakeDef>,
    pub brake_heat: Option<BrakeHeatDef>,
    pub wheel_slide: Option<WheelSlideDef>,
    pub traction_control: Option<TractionControlDef>,
    pub battery: Option<TractionBatteryDef>,
    pub motors: Vec<MotorDef>,
    pub diesel_circuits: Option<DieselCircuitsDef>,
    pub auxiliaries: Option<AuxiliaryDef>,
    pub multiple_unit: Option<MultipleUnitDef>,
    /// Train supply cable through this vehicle. False means an intentional break.
    pub train_bus: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorsDef {
    pub left: bool,
    pub right: bool,
    pub open_s: f64,
    pub close_s: f64,
    #[serde(default = "yes")]
    pub traction_interlock: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DoorSide {
    Left,
    Right,
    #[default]
    Both,
}
impl DoorSide {
    pub fn flipped(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Both => Self::Both,
        }
    }
    pub fn includes(self, side: usize) -> bool {
        self == Self::Both
            || (side == 0 && self == Self::Left)
            || (side == 1 && self == Self::Right)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformSideDef {
    pub stop: usize,
    pub side: DoorSide,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TiltDef {
    #[serde(default)]
    pub native_response: bool,
    pub maximum_rad: f64,
    pub time_constant_s: f64,
    pub max_rate_rad_s: f64,
    pub compensation: f64,
    pub minimum_speed_mps: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrosswindDef {
    pub area_m2: f64,
    pub lateral_coefficient: f64,
    pub center_height_m: f64,
    pub roll_stiffness_nm_rad: f64,
    pub roll_damping_nm_s: f64,
    pub roll_inertia_kg_m2: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElectricBrakeDef {
    #[serde(default)]
    pub native_response: bool,
    #[serde(default)]
    pub force_ramp_n_s: [f64; 3],
    #[serde(default)]
    pub power_ramp_w_s: [f64; 3],
    #[serde(default)]
    pub maximum_current_a: Option<f64>,
    /// Speed m/s → force N at full handle; sorted, including low-speed fade.
    pub force_curve: Vec<[f64; 2]>,
    #[serde(default)]
    pub notches: Vec<(f64, Vec<[f64; 2]>)>,
    pub rheostat_max_w: f64,
    pub regenerative_max_w: f64,
    pub receptivity_w: f64,
    pub efficiency: f64,
    pub apply_delay_s: f64,
    #[serde(default)]
    pub blend_with_service: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrakeHeatDef {
    pub heat_capacity_j_k: f64,
    pub cooling_w_k: f64,
    pub fade_start_c: f64,
    pub fade_full_c: f64,
    pub minimum_factor: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WheelSlideDef {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub slip_threshold: f64,
    pub release_rate_s: f64,
    pub reapply_rate_s: f64,
    pub wheel_inertia_kg_m2: f64,
    pub radius_m: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TractionControlDef {
    pub slip_threshold: f64,
    pub cut_rate_s: f64,
    pub recover_rate_s: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TractionSource {
    #[default]
    Auto,
    Line,
    Diesel,
    Battery,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TractionBatteryDef {
    pub capacity_kwh: f64,
    pub initial_fraction: f64,
    pub maximum_discharge_w: f64,
    pub maximum_charge_w: f64,
    pub efficiency: f64,
    pub minimum_fraction: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotorDef {
    #[serde(default)]
    pub native_governor: Option<NativeMotorGovernor>,
    pub id: String,
    pub idle_rpm: f64,
    pub maximum_rpm: f64,
    pub rise_rpm_s: f64,
    pub fall_rpm_s: f64,
    pub rated_power_w: f64,
    pub fuel_l: f64,
    pub idle_lph: f64,
    pub full_lph: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeMotorGovernor {
    pub power_curve: Vec<[f64; 2]>,
    pub throttle_rpm_curve: Vec<[f64; 2]>,
    pub rate_up_rpm_s2: f64,
    pub rate_down_rpm_s2: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DieselCircuitsDef {
    pub oil_capacity_l: f64,
    pub coolant_capacity_l: f64,
    pub oil_heat_capacity_j_k: f64,
    pub coolant_heat_capacity_j_k: f64,
    pub heat_fraction: f64,
    pub transfer_w_k: f64,
    pub radiator_w_k: f64,
    pub fan_start_c: f64,
    pub fan_stop_c: f64,
    pub minimum_oil_fraction: f64,
    pub minimum_coolant_fraction: f64,
    pub shutdown_oil_c: f64,
    pub shutdown_coolant_c: f64,
    #[serde(default)]
    pub failures_enabled: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuxiliaryDef {
    /// OR 1.6.1 flow tables for each explicitly fitted injector, in millimetres.
    #[serde(default)]
    pub native_injector_mm: Option<[u8; 2]>,
    #[serde(default)]
    pub leak_bar_s: f64,
    pub compressor_efficiency: f64,
    pub compressor_idle_w: f64,
    /// Battery for low voltage auxiliaries; separate from a traction battery.
    pub battery_kwh: f64,
    pub steam_compressor_kg_j: f64,
    pub ejector_steam_kg_s: f64,
    pub injector_steam_kg_per_water_kg: f64,
    pub injector_max_pressure_bar: f64,
    pub feedwater_c: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MultipleUnitDef {
    pub protocol: String,
    pub compatible_protocols: Vec<String>,
    pub delay_s: f64,
    #[serde(default = "yes")]
    pub throttle: bool,
    #[serde(default = "yes")]
    pub dynamic_brake: bool,
    #[serde(default = "yes")]
    pub reverser: bool,
}

fn yes() -> bool {
    true
}
fn finite_range(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}
fn name(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
impl VehicleSystemsDef {
    pub fn valid(&self) -> bool {
        self.vehicle < 256
            && self.doors.as_ref().is_none_or(|p| {
                (p.left || p.right)
                    && finite_range(p.open_s, 0., 60.)
                    && finite_range(p.close_s, 0., 60.)
            })
            && self.tilt.as_ref().is_none_or(|p| {
                finite_range(p.maximum_rad, 0.001, 0.2)
                    && finite_range(p.time_constant_s, 0.05, 20.)
                    && finite_range(p.max_rate_rad_s, 0.001, 0.2)
                    && finite_range(p.compensation, 0., 1.)
                    && finite_range(p.minimum_speed_mps, 0., 100.)
            })
            && self.crosswind.as_ref().is_none_or(|p| {
                finite_range(p.area_m2, 1., 200.)
                    && finite_range(p.lateral_coefficient, 0., 3.)
                    && finite_range(p.center_height_m, 0.1, 6.)
                    && finite_range(p.roll_stiffness_nm_rad, 1e5, 1e9)
                    && finite_range(p.roll_damping_nm_s, 1e3, 1e8)
                    && finite_range(p.roll_inertia_kg_m2, 1e3, 1e7)
            })
            && self.electric_brake.as_ref().is_none_or(|p| {
                p.force_ramp_n_s.iter().all(|v| finite_range(*v, 0., 1e8))
                    && p.power_ramp_w_s.iter().all(|v| finite_range(*v, 0., 1e10))
                    && p.maximum_current_a.is_none_or(|v| finite_range(v, 0., 1e5))
                    && p.notches.len() <= 64
                    && p.notches.iter().all(|(h, t)| {
                        finite_range(*h, 0., 1.)
                            && t.len() >= 2
                            && t.len() <= 256
                            && t.iter().all(|q| {
                                finite_range(q[0], 0., 400.) && finite_range(q[1], 0., 1e6)
                            })
                            && t.windows(2).all(|q| q[0][0] < q[1][0])
                    })
                    && p.notches.windows(2).all(|q| q[0].0 < q[1].0)
                    && p.force_curve.len() >= 2
                    && p.force_curve.len() <= 256
                    && p.force_curve
                        .iter()
                        .all(|q| finite_range(q[0], 0., 400.) && finite_range(q[1], 0., 1e6))
                    && p.force_curve.windows(2).all(|q| q[0][0] < q[1][0])
                    && finite_range(p.rheostat_max_w, 0., 1e12)
                    && finite_range(p.regenerative_max_w, 0., 2e7)
                    && finite_range(p.receptivity_w, 0., 2e7)
                    && finite_range(p.efficiency, 0.1, 1.)
                    && finite_range(p.apply_delay_s, 0., 60.)
            })
            && self.brake_heat.as_ref().is_none_or(|p| {
                finite_range(p.heat_capacity_j_k, 1e4, 1e8)
                    && finite_range(p.cooling_w_k, 1., 1e5)
                    && finite_range(p.fade_start_c, 50., 700.)
                    && finite_range(p.fade_full_c, p.fade_start_c + 1., 1200.)
                    && finite_range(p.minimum_factor, 0.05, 1.)
            })
            && self.wheel_slide.as_ref().is_none_or(|p| {
                finite_range(p.slip_threshold, 0.01, 0.5)
                    && finite_range(p.release_rate_s, 0.1, 50.)
                    && finite_range(p.reapply_rate_s, 0.01, 20.)
                    && finite_range(p.wheel_inertia_kg_m2, 1., 1e5)
                    && finite_range(p.radius_m, 0.1, 2.)
            })
            && self.traction_control.as_ref().is_none_or(|p| {
                finite_range(p.slip_threshold, 0.01, 0.5)
                    && finite_range(p.cut_rate_s, 0.1, 50.)
                    && finite_range(p.recover_rate_s, 0.01, 5.)
            })
            && self.battery.as_ref().is_none_or(|p| {
                finite_range(p.capacity_kwh, 0.01, 10000.)
                    && finite_range(p.initial_fraction, 0., 1.)
                    && finite_range(p.maximum_discharge_w, 1., 2e7)
                    && finite_range(p.maximum_charge_w, 0., 2e7)
                    && finite_range(p.efficiency, 0.1, 1.)
                    && finite_range(p.minimum_fraction, 0., p.initial_fraction)
            })
            && self.motors.len() <= 8
            && self.motors.iter().enumerate().all(|(i, p)| {
                p.native_governor.as_ref().is_none_or(|g| {
                    g.power_curve.len() <= 256
                        && g.throttle_rpm_curve.len() <= 256
                        && g.power_curve
                            .iter()
                            .all(|v| finite_range(v[0], 0., 20000.) && finite_range(v[1], 0., 2e7))
                        && g.throttle_rpm_curve
                            .iter()
                            .all(|v| finite_range(v[0], 0., 1.) && finite_range(v[1], 0., 10000.))
                        && g.power_curve.windows(2).all(|v| v[0][0] < v[1][0])
                        && g.throttle_rpm_curve.windows(2).all(|v| v[0][0] < v[1][0])
                        && finite_range(g.rate_up_rpm_s2, 1., 10000.)
                        && finite_range(g.rate_down_rpm_s2, 1., 10000.)
                }) && name(&p.id)
                    && self.motors[..i].iter().all(|o| o.id != p.id)
                    && finite_range(p.idle_rpm, 1., 3000.)
                    && finite_range(p.maximum_rpm, p.idle_rpm + 1., 10000.)
                    && finite_range(p.rise_rpm_s, 1., 10000.)
                    && finite_range(p.fall_rpm_s, 1., 10000.)
                    && finite_range(p.rated_power_w, 1., 2e7)
                    && finite_range(p.fuel_l, 0., 200000.)
                    && finite_range(p.idle_lph, 0., 10000.)
                    && finite_range(p.full_lph, p.idle_lph, 20000.)
            })
            && self.diesel_circuits.as_ref().is_none_or(|p| {
                finite_range(p.oil_capacity_l, 1., 5000.)
                    && finite_range(p.coolant_capacity_l, 1., 10000.)
                    && finite_range(p.oil_heat_capacity_j_k, 1e3, 1e8)
                    && finite_range(p.coolant_heat_capacity_j_k, 1e3, 1e8)
                    && finite_range(p.heat_fraction, 0., 2.)
                    && finite_range(p.transfer_w_k, 1., 1e5)
                    && finite_range(p.radiator_w_k, 1., 1e5)
                    && finite_range(p.fan_start_c, 30., 150.)
                    && finite_range(p.fan_stop_c, 0., p.fan_start_c - 1.)
                    && finite_range(p.minimum_oil_fraction, 0., 1.)
                    && finite_range(p.minimum_coolant_fraction, 0., 1.)
                    && finite_range(p.shutdown_oil_c, 80., 300.)
                    && finite_range(p.shutdown_coolant_c, 80., 300.)
            })
            && self.auxiliaries.as_ref().is_none_or(|p| {
                finite_range(p.leak_bar_s, 0., 1.)
                    && finite_range(p.compressor_efficiency, 0.1, 1.)
                    && p.native_injector_mm.is_none_or(|sizes| {
                        sizes
                            .into_iter()
                            .all(|s| [9, 10, 11, 13, 14, 15].contains(&s))
                    })
                    && finite_range(p.compressor_idle_w, 0., 1e5)
                    && finite_range(p.battery_kwh, 0., 100.)
                    && finite_range(p.steam_compressor_kg_j, 0., 0.01)
                    && finite_range(p.ejector_steam_kg_s, 0., 5.)
                    && finite_range(p.injector_steam_kg_per_water_kg, 0., 0.5)
                    && finite_range(p.injector_max_pressure_bar, 1., 50.)
                    && finite_range(p.feedwater_c, 0., 150.)
            })
            && self.multiple_unit.as_ref().is_none_or(|p| {
                name(&p.protocol)
                    && p.compatible_protocols.len() <= 16
                    && p.compatible_protocols.iter().all(|s| name(s))
                    && finite_range(p.delay_s, 0., 30.)
            })
    }
}

pub fn valid_definitions(defs: &[VehicleSystemsDef], platforms: &[PlatformSideDef]) -> bool {
    defs.len() <= 256
        && defs
            .iter()
            .enumerate()
            .all(|(i, p)| p.valid() && defs[..i].iter().all(|o| o.vehicle != p.vehicle))
        && platforms.len() <= 4096
        && platforms
            .iter()
            .enumerate()
            .all(|(i, p)| p.stop < 4096 && platforms[..i].iter().all(|o| o.stop != p.stop))
}

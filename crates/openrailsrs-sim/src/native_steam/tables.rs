// COPYRIGHT 2010, 2011 by the Open Rails project.
// Rust translation of OR 1.6.1 SteamTable.cs and Interpolator.cs.
// GPL-3.0-or-later; source d16e670da333d26d2edfc97d5631a19dadf49ce5.
// Generated implementation tables. These are not independent reference data.
#![allow(non_upper_case_globals, non_snake_case, dead_code)]
#[derive(Clone, Copy)]
pub struct Table {
    pub x: &'static [f32],
    pub y: &'static [f32],
}
impl Table {
    pub fn at(self, x: f32) -> f32 {
        interpolate(self.x, self.y, x)
    }
}
pub fn interpolate(xs: &[f32], ys: &[f32], x: f32) -> f32 {
    let mut i = 0;
    if x > xs[xs.len() - 2] {
        i = xs.len() - 2;
    } else if x >= xs[1] {
        let mut j = xs.len() - 1;
        while j - i > 1 {
            let k = (i + j) / 2;
            if xs[k] > x {
                j = k;
            } else {
                i = k;
            }
        }
    }
    let d = xs[i + 1] - xs[i];
    let a = (xs[i + 1] - x) / d;
    let b = (x - xs[i]) / d;
    a * ys[i] + b * ys[i + 1]
}
pub fn cutoff_initial_pressure(upper: bool, rpm: f32, cutoff: f32) -> f32 {
    let rows: [&[f32]; 8] = if upper {
        [
            &InitialPressureUpper0RpM,
            &InitialPressureUpper50RpM,
            &InitialPressureUpper100RpM,
            &InitialPressureUpper150RpM,
            &InitialPressureUpper200RpM,
            &InitialPressureUpper250RpM,
            &InitialPressureUpper300RpM,
            &InitialPressureUpper350RpM,
        ]
    } else {
        [
            &InitialPressureLower0RpM,
            &InitialPressureLower50RpM,
            &InitialPressureLower100RpM,
            &InitialPressureLower150RpM,
            &InitialPressureLower200RpM,
            &InitialPressureLower250RpM,
            &InitialPressureLower300RpM,
            &InitialPressureLower350RpM,
        ]
    };
    let mut i = 0;
    if rpm > WheelRevolutionsRpM[6] {
        i = 6;
    } else if rpm >= WheelRevolutionsRpM[1] {
        let mut j = 7;
        while j - i > 1 {
            let k = (i + j) / 2;
            if WheelRevolutionsRpM[k] > rpm {
                j = k;
            } else {
                i = k;
            }
        }
    }
    let d = WheelRevolutionsRpM[i + 1] - WheelRevolutionsRpM[i];
    let a = (WheelRevolutionsRpM[i + 1] - rpm) / d;
    let b = (rpm - WheelRevolutionsRpM[i]) / d;
    let mut z = 0.;
    if a != 0. {
        z += a * interpolate(&CutOff, rows[i], cutoff);
    }
    if b != 0. {
        z += b * interpolate(&CutOff, rows[i + 1], cutoff);
    }
    z
}
// SteamTable.cs SHA256: 9e6d57d726a2c0dd827c4fb2a0a4a1d21325ebda0252027a47c2a8c41843ce0b
pub const PressureTableGaugePSI: [f32; 31] = [
    0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 140.0,
    150.0, 160.0, 170.0, 180.0, 190.0, 200.0, 210.0, 220.0, 230.0, 240.0, 250.0, 260.0, 270.0,
    280.0, 290.0, 300.0,
];
pub const TemperatureTableF: [f32; 31] = [
    212.0, 239.4, 258.7, 274.0, 286.7, 297.7, 307.3, 316.0, 323.9, 331.2, 337.9, 344.2, 350.1,
    355.6, 360.9, 365.9, 370.6, 375.6, 379.6, 383.8, 387.8, 391.7, 395.5, 399.1, 402.6, 406.0,
    409.4, 412.6, 415.7, 418.8, 421.8,
];
pub const WaterHeatTableBTUpLB: [f32; 31] = [
    180.15, 207.83, 227.51, 243.08, 256.09, 267.34, 277.31, 286.30, 294.49, 302.05, 309.08, 315.66,
    321.85, 327.70, 333.26, 338.56, 343.62, 348.47, 353.13, 357.62, 361.95, 366.14, 370.19, 374.12,
    377.94, 381.65, 385.26, 388.78, 392.21, 395.56, 398.84,
];
pub const WaterDensityTableLBpFT3: [f32; 31] = [
    59.83, 59.11, 58.57, 58.12, 57.73, 57.39, 57.07, 56.79, 56.52, 56.27, 56.03, 55.81, 55.59,
    55.39, 55.19, 55.00, 54.82, 54.65, 54.48, 54.31, 54.15, 53.99, 53.84, 53.69, 53.54, 53.40,
    53.26, 53.12, 52.99, 52.86, 52.73,
];
pub const SteamHeatTableBTUpLB: [f32; 31] = [
    1150.28, 1160.31, 1167.01, 1172.03, 1176.00, 1179.27, 1182.04, 1184.42, 1186.50, 1188.33,
    1189.95, 1191.41, 1192.72, 1193.91, 1194.99, 1195.97, 1196.86, 1197.68, 1198.43, 1199.12,
    1199.75, 1200.33, 1200.86, 1201.35, 1201.79, 1202.20, 1202.58, 1202.92, 1203.23, 1203.51,
    1203.77,
];
pub const SteamDensityTableLBpFT3: [f32; 31] = [
    0.0373, 0.0606, 0.0834, 0.1057, 0.1277, 0.1496, 0.1713, 0.1928, 0.2143, 0.2356, 0.2569, 0.2782,
    0.2994, 0.3205, 0.3416, 0.3627, 0.3838, 0.4048, 0.4259, 0.4469, 0.4680, 0.4890, 0.5101, 0.5312,
    0.5522, 0.5733, 0.5944, 0.6155, 0.6367, 0.6578, 0.6790,
];
pub const Injector09FlowTableUKGpM: [f32; 31] = [
    7.30, 9.95, 12.61, 15.26, 17.91, 20.56, 23.22, 25.87, 28.52, 31.18, 33.83, 36.48, 39.14, 39.90,
    40.66, 41.43, 42.19, 42.95, 43.72, 43.97, 44.23, 44.49, 44.74, 45.00, 45.26, 45.51, 45.77,
    46.03, 46.28, 46.28, 46.80,
];
pub const Injector10FlowTableUKGpM: [f32; 31] = [
    7.3, 10.68, 14.06, 17.44, 20.83, 24.21, 27.59, 30.97, 34.35, 37.73, 41.12, 44.50, 47.88, 48.92,
    49.96, 51.00, 52.04, 53.08, 54.12, 54.48, 54.85, 55.21, 55.57, 55.93, 56.29, 56.65, 57.01,
    57.37, 57.73, 58.09, 58.45,
];
pub const Injector11FlowTableUKGpM: [f32; 31] = [
    9.3, 13.30, 17.31, 21.31, 25.31, 29.32, 33.32, 37.33, 41.33, 45.33, 49.34, 53.34, 57.34, 58.59,
    59.84, 61.09, 62.34, 63.59, 64.84, 65.21, 65.57, 65.94, 66.31, 66.68, 67.04, 67.41, 67.78,
    68.15, 68.52, 68.88, 69.25,
];
pub const Injector13FlowTableUKGpM: [f32; 31] = [
    9.3, 15.35, 21.40, 27.44, 33.49, 39.54, 45.59, 51.64, 57.69, 63.73, 69.78, 75.83, 81.88, 83.75,
    85.63, 87.50, 89.37, 91.25, 93.12, 93.75, 94.37, 94.99, 95.62, 96.24, 96.87, 97.49, 98.12,
    98.74, 99.37, 99.99, 100.61,
];
pub const Injector14FlowTableUKGpM: [f32; 31] = [
    9.30, 16.79, 24.28, 31.77, 39.26, 46.75, 54.24, 61.73, 69.21, 76.70, 84.19, 91.68, 99.17,
    100.81, 102.45, 104.09, 105.72, 107.36, 109.00, 111.65, 114.30, 115.94, 117.58, 119.21, 120.85,
    122.49, 124.13, 125.77, 127.40, 129.04, 130.68,
];
pub const Injector15FlowTableUKGpM: [f32; 31] = [
    9.30, 17.82, 26.33, 34.85, 43.36, 51.88, 60.40, 68.91, 77.43, 85.94, 94.46, 102.98, 111.49,
    113.33, 115.16, 117.0, 118.83, 120.67, 122.50, 125.50, 128.50, 130.14, 131.78, 133.41, 135.05,
    136.69, 138.33, 139.97, 141.60, 143.24, 144.88,
];
pub const SpecificHeatTableKJpKGpK: [f32; 31] = [
    4.2170, 4.2049, 4.2165, 4.2223, 4.2287, 4.2355, 4.2427, 4.2505, 4.2587, 4.2675, 4.2769, 4.2926,
    4.3035, 4.3151, 4.3274, 4.3405, 4.3543, 4.3690, 4.3846, 4.4012, 4.4187, 4.4374, 4.4573, 4.4784,
    4.5009, 4.5248, 4.5503, 4.5774, 4.6064, 4.6373, 4.6703,
];
pub const WaterTemperatureTableK: [f32; 31] = [
    274.00, 281.40, 288.80, 296.20, 303.60, 311.00, 318.40, 325.80, 333.20, 340.60, 348.00, 355.40,
    362.80, 370.20, 377.60, 385.00, 392.40, 399.80, 407.20, 414.60, 422.00, 429.40, 436.80, 444.20,
    451.60, 459.00, 466.40, 473.80, 481.20, 488.60, 496.00,
];
pub const SaturationPressureTablePSI: [f32; 31] = [
    0.00, 10.00, 20.00, 30.00, 40.00, 50.00, 60.00, 70.00, 80.00, 90.00, 100.00, 110.00, 120.00,
    130.00, 140.00, 150.00, 160.00, 170.00, 180.00, 190.00, 200.00, 210.00, 220.00, 230.00, 240.00,
    250.00, 260.00, 270.00, 280.00, 290.00, 300.00,
];
pub const TemperatureTableK: [f32; 31] = [
    372.76, 388.11, 398.94, 407.45, 414.53, 420.62, 426.01, 430.84, 435.23, 439.27, 443.02, 446.51,
    449.79, 452.88, 455.80, 458.58, 461.23, 463.77, 466.20, 468.53, 470.78, 472.94, 475.03, 477.06,
    479.02, 480.92, 482.76, 484.56, 486.30, 488.00, 489.66,
];
pub const CoalGrateAreaTableLbspFt2: [f32; 12] = [
    0.0, 20.0, 40.0, 60.0, 80.0, 100.0, 120.0, 140.0, 160.0, 180.0, 200.0, 220.0,
];
pub const SatBoilerEfficiencyTableX: [f32; 12] = [
    0.80, 0.749, 0.69, 0.63, 0.571, 0.512, 0.452, 0.393, 0.334, 0.274, 0.215, 0.156,
];
pub const SuperBoilerEfficiencyTableX: [f32; 12] = [
    0.903, 0.8484, 0.7936, 0.7390, 0.6843, 0.6296, 0.5749, 0.5202, 0.4655, 0.4108, 0.3561, 0.3014,
];
pub const InjectorUsePressureTablePSI: [f32; 5] = [75.0, 150.00, 175.00, 200.00, 225.00];
pub const WaterTemPDeliveryMinTableF: [f32; 5] = [217.0, 255.00, 257.00, 260.00, 269.00];
pub const WaterTemPDeliveryMaxTableF: [f32; 5] = [128.0, 149.00, 156.00, 164.00, 174.00];
pub const WaterDelFedSteamTableLbs: [f32; 5] = [17.20, 12.80, 11.80, 10.70, 9.70];
pub const InjMinCapFactorTableX: [f32; 5] = [0.366, 0.395, 0.419, 0.454, 0.509];
pub const CutOffFractionEventTableX: [f32; 16] = [
    0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55, 0.6, 0.65, 0.7, 0.75, 0.8,
];
pub const CylinderExhaustTableX: [f32; 16] = [
    0.5306, 0.6122, 0.6646, 0.7042, 0.7358, 0.7628, 0.7866, 0.8076, 0.8270, 0.8451, 0.8626, 0.8792,
    0.8955, 0.9112, 0.9269, 0.9422,
];
pub const CylinderCompressionTableX: [f32; 16] = [
    0.4580, 0.3864, 0.3418, 0.3082, 0.2811, 0.2575, 0.2363, 0.2170, 0.1988, 0.1814, 0.1641, 0.1471,
    0.1299, 0.1127, 0.0950, 0.0771,
];
pub const CylinderAdmissionTableX: [f32; 16] = [
    0.0241, 0.0121, 0.0080, 0.0058, 0.0046, 0.0037, 0.0030, 0.0026, 0.0022, 0.0019, 0.0015, 0.0013,
    0.0011, 0.0009, 0.0008, 0.0006,
];
pub const CutOffFractionTableX: [f32; 11] =
    [0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.55];
pub const CylinderCondensationFractionTableX: [f32; 11] = [
    0.526, 0.42, 0.345, 0.29, 0.245, 0.213, 0.181, 0.159, 0.142, 0.125, 0.11,
];
pub const SuperheatCondenstationLimitTableDegF: [f32; 11] = [
    265.0, 245.0, 223.0, 190.0, 166.0, 145.0, 128.0, 110.0, 100.0, 75.0, 60.0,
];
pub const CylinderSteamTableLbpH: [f32; 19] = [
    0.0, 2000.0, 4000.0, 6000.0, 8000.0, 10000.0, 12000.0, 14000.0, 16000.0, 18000.0, 20000.0,
    22000.0, 24000.0, 26000.0, 28000.0, 30000.0, 32000.0, 34000.0, 36000.0,
];
pub const SuperheatTempTableDegF: [f32; 19] = [
    0.0, 40.0, 70.0, 100.0, 140.0, 164.0, 195.0, 220.0, 242.0, 260.0, 278.0, 290.0, 304.0, 320.0,
    335.0, 348.0, 360.0, 375.0, 384.0,
];
pub const WheelRotationRpM: [f32; 16] = [
    0.0, 50.0, 100.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0, 450.0, 500.0, 550.0, 600.0, 650.0,
    700.0, 750.0,
];
pub const SatInitialPressureDropRatio: [f32; 16] = [
    0.98, 0.965, 0.95, 0.935, 0.92, 0.905, 0.89, 0.875, 0.87, 0.8650, 0.8625, 0.86, 0.8575, 0.855,
    0.8525, 0.85,
];
pub const SuperInitialPressureDropRatio: [f32; 16] = [
    0.99, 0.98, 0.97, 0.96, 0.95, 0.94, 0.93, 0.92, 0.915, 0.910, 0.905, 0.90, 0.8975, 0.8950,
    0.8925, 0.8900,
];
pub const PistonSpeedFtpMin: [f32; 22] = [
    0.0, 100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 900.0, 1000.0, 1100.0, 1200.0,
    1300.0, 1400.0, 1500.0, 1600.0, 1700.0, 1800.0, 1900.0, 2000.0, 2100.0,
];
pub const SpeedFactorSat: [f32; 22] = [
    1.0, 1.0, 1.0, 0.954, 0.863, 0.772, 0.680, 0.590, 0.517, 0.460, 0.412, 0.372, 0.337, 0.307,
    0.283, 0.261, 0.241, 0.225, 0.213, 0.202, 0.190, 0.185,
];
pub const SpeedFactorSuper: [f32; 22] = [
    1.0, 1.0, 1.0, 0.988, 0.965, 0.912, 0.859, 0.800, 0.753, 0.706, 0.659, 0.612, 0.571, 0.535,
    0.500, 0.471, 0.447, 0.433, 0.424, 0.420, 0.410, 0.410,
];
pub const IndicatedHorsepowerIHP: [f32; 16] = [
    0.0, 200.0, 400.0, 600.0, 800.0, 1000.0, 1200.0, 1400.0, 1600.0, 1800.0, 2000.0, 2200.0,
    2400.0, 2600.0, 2800.0, 3000.0,
];
pub const BackPressureSatPSI: [f32; 16] = [
    0.0, 1.0, 2.0, 2.33, 3.0, 5.3, 5.6, 8.0, 11.2, 14.25, 16.0, 20.0, 24.0, 26.0, 28.0, 30.0,
];
pub const BackPressureSuperPSI: [f32; 16] = [
    0.0, 0.25, 0.5, 0.75, 1.25, 1.75, 2.5, 3.5, 4.8, 7.2, 11.25, 16.0, 20.0, 22.0, 24.0, 26.0,
];
pub const CondensationWheelRotationRpM: [f32; 8] =
    [0.0, 50.0, 100.0, 150.0, 200.0, 250.0, 300.0, 350.0];
pub const WheelRevolutionsRpM: [f32; 8] = [0.0, 50.0, 100.0, 150.0, 200.0, 250.0, 300.0, 350.0];
pub const CutOff: [f32; 11] = [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
pub const InitialPressureUpper0RpM: [f32; 11] =
    [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
pub const InitialPressureUpper50RpM: [f32; 11] = [
    0.79, 0.82, 0.83, 0.86, 0.875, 0.9, 0.92, 0.94, 0.96, 0.98, 1.0,
];
pub const InitialPressureUpper100RpM: [f32; 11] = [
    0.72, 0.74, 0.77, 0.795, 0.825, 0.85, 0.875, 0.9, 0.925, 0.955, 0.98,
];
pub const InitialPressureUpper150RpM: [f32; 11] = [
    0.68, 0.71, 0.735, 0.760, 0.7875, 0.8125, 0.840, 0.870, 0.90, 0.930, 0.9575,
];
pub const InitialPressureUpper200RpM: [f32; 11] = [
    0.66, 0.68, 0.705, 0.727, 0.757, 0.78, 0.81, 0.84, 0.875, 0.908, 0.938,
];
pub const InitialPressureUpper250RpM: [f32; 11] = [
    0.645, 0.665, 0.6875, 0.71, 0.73, 0.76, 0.788, 0.81, 0.85, 0.88, 0.913,
];
pub const InitialPressureUpper300RpM: [f32; 11] = [
    0.63, 0.6475, 0.665, 0.68, 0.71, 0.73, 0.7625, 0.79, 0.81, 0.85, 0.877,
];
pub const InitialPressureUpper350RpM: [f32; 11] = [
    0.6125, 0.63, 0.65, 0.663, 0.6875, 0.7125, 0.7425, 0.775, 0.809, 0.8375, 0.865,
];
pub const InitialPressureLower0RpM: [f32; 11] =
    [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
pub const InitialPressureLower50RpM: [f32; 11] = [
    0.745, 0.76, 0.77, 0.7875, 0.8125, 0.8375, 0.865, 0.9, 0.93, 0.97, 1.0,
];
pub const InitialPressureLower100RpM: [f32; 11] = [
    0.6875, 0.6875, 0.69, 0.708, 0.725, 0.755, 0.79, 0.83, 0.88, 0.9325, 0.9875,
];
pub const InitialPressureLower150RpM: [f32; 11] = [
    0.64, 0.64, 0.645, 0.6525, 0.67, 0.6975, 0.731, 0.775, 0.825, 0.875, 0.93,
];
pub const InitialPressureLower200RpM: [f32; 11] = [
    0.61, 0.61, 0.61, 0.6125, 0.625, 0.65, 0.68, 0.72, 0.775, 0.83, 0.88,
];
pub const InitialPressureLower250RpM: [f32; 11] = [
    0.5775, 0.5775, 0.5775, 0.585, 0.595, 0.62, 0.653, 0.69, 0.7375, 0.7825, 0.825,
];
pub const InitialPressureLower300RpM: [f32; 11] = [
    0.55, 0.55, 0.55, 0.5575, 0.57, 0.59, 0.62, 0.6575, 0.6925, 0.73, 0.7675,
];
pub const InitialPressureLower350RpM: [f32; 11] = [
    0.52, 0.52, 0.52, 0.528, 0.545, 0.57, 0.60, 0.635, 0.67, 0.7075, 0.74,
];
pub const CylinderEventExhausttoCutoff: Table = Table {
    x: &CutOffFractionEventTableX,
    y: &CylinderExhaustTableX,
};
pub const CylinderEventCompressiontoCutoff: Table = Table {
    x: &CutOffFractionEventTableX,
    y: &CylinderCompressionTableX,
};
pub const CylinderEventAdmissiontoCutoff: Table = Table {
    x: &CutOffFractionEventTableX,
    y: &CylinderAdmissionTableX,
};
pub const CylinderCondensationFractionInterpolatorX: Table = Table {
    x: &CutOffFractionTableX,
    y: &CylinderCondensationFractionTableX,
};
pub const SuperheatTempLimitInterpolatorXtoDegF: Table = Table {
    x: &CutOffFractionTableX,
    y: &SuperheatCondenstationLimitTableDegF,
};
pub const BackpressureSatIHPtoPSI: Table = Table {
    x: &IndicatedHorsepowerIHP,
    y: &BackPressureSatPSI,
};
pub const BackpressureSuperIHPtoPSI: Table = Table {
    x: &IndicatedHorsepowerIHP,
    y: &BackPressureSuperPSI,
};
pub const SaturatedSpeedFactorSpeedDropFtpMintoX: Table = Table {
    x: &PistonSpeedFtpMin,
    y: &SpeedFactorSat,
};
pub const SuperheatedSpeedFactorSpeedDropFtpMintoX: Table = Table {
    x: &PistonSpeedFtpMin,
    y: &SpeedFactorSuper,
};
pub const SuperInitialPressureDropRatioInterpolatorRpMtoX: Table = Table {
    x: &WheelRotationRpM,
    y: &SuperInitialPressureDropRatio,
};
pub const SatInitialPressureDropRatioInterpolatorRpMtoX: Table = Table {
    x: &WheelRotationRpM,
    y: &SatInitialPressureDropRatio,
};
pub const SuperheatTempInterpolatorLbpHtoDegF: Table = Table {
    x: &CylinderSteamTableLbpH,
    y: &SuperheatTempTableDegF,
};
pub const InjCapMinFactorInterpolatorX: Table = Table {
    x: &InjectorUsePressureTablePSI,
    y: &InjMinCapFactorTableX,
};
pub const InjDelWaterTempMaxPressureInterpolatorFtoPSI: Table = Table {
    x: &InjectorUsePressureTablePSI,
    y: &WaterTemPDeliveryMaxTableF,
};
pub const InjDelWaterTempMinPressureInterpolatorFtoPSI: Table = Table {
    x: &InjectorUsePressureTablePSI,
    y: &WaterTemPDeliveryMinTableF,
};
pub const InjWaterFedSteamPressureInterpolatorFtoPSI: Table = Table {
    x: &InjectorUsePressureTablePSI,
    y: &WaterDelFedSteamTableLbs,
};
pub const SatBoilerEfficiencyGrateAreaInterpolatorLbstoX: Table = Table {
    x: &CoalGrateAreaTableLbspFt2,
    y: &SatBoilerEfficiencyTableX,
};
pub const SuperBoilerEfficiencyGrateAreaInterpolatorLbstoX: Table = Table {
    x: &CoalGrateAreaTableLbspFt2,
    y: &SuperBoilerEfficiencyTableX,
};
pub const SaturationPressureInterpolatorKtoPSI: Table = Table {
    x: &TemperatureTableK,
    y: &SaturationPressureTablePSI,
};
pub const SpecificHeatInterpolatorKtoKJpKGpK: Table = Table {
    x: &WaterTemperatureTableK,
    y: &SpecificHeatTableKJpKGpK,
};
pub const Injector09FlowrateInterpolatorPSItoUKGpM: Table = Table {
    x: &PressureTableGaugePSI,
    y: &Injector09FlowTableUKGpM,
};
pub const Injector10FlowrateInterpolatorPSItoUKGpM: Table = Table {
    x: &PressureTableGaugePSI,
    y: &Injector10FlowTableUKGpM,
};
pub const Injector11FlowrateInterpolatorPSItoUKGpM: Table = Table {
    x: &PressureTableGaugePSI,
    y: &Injector11FlowTableUKGpM,
};
pub const Injector13FlowrateInterpolatorPSItoUKGpM: Table = Table {
    x: &PressureTableGaugePSI,
    y: &Injector13FlowTableUKGpM,
};
pub const Injector14FlowrateInterpolatorPSItoUKGpM: Table = Table {
    x: &PressureTableGaugePSI,
    y: &Injector14FlowTableUKGpM,
};
pub const Injector15FlowrateInterpolatorPSItoUKGpM: Table = Table {
    x: &PressureTableGaugePSI,
    y: &Injector15FlowTableUKGpM,
};
pub const WaterHeatInterpolatorPSItoBTUpLB: Table = Table {
    x: &PressureTableGaugePSI,
    y: &WaterHeatTableBTUpLB,
};
pub const WaterDensityInterpolatorPSItoLBpFT3: Table = Table {
    x: &PressureTableGaugePSI,
    y: &WaterDensityTableLBpFT3,
};
pub const SteamHeatInterpolatorPSItoBTUpLB: Table = Table {
    x: &PressureTableGaugePSI,
    y: &SteamHeatTableBTUpLB,
};
pub const SteamDensityInterpolatorPSItoLBpFT3: Table = Table {
    x: &PressureTableGaugePSI,
    y: &SteamDensityTableLBpFT3,
};
pub const WaterHeatToPressureInterpolatorBTUpLBtoPSI: Table = Table {
    x: &WaterHeatTableBTUpLB,
    y: &PressureTableGaugePSI,
};
pub const TemperatureToPressureInterpolatorFtoPSI: Table = Table {
    x: &TemperatureTableF,
    y: &PressureTableGaugePSI,
};
pub const PressureToTemperatureInterpolatorPSItoF: Table = Table {
    x: &PressureTableGaugePSI,
    y: &TemperatureTableF,
};
pub const SteamHeatPressureToTemperatureInterpolatorPSItoF: Table = Table {
    x: &PressureTableGaugePSI,
    y: &TemperatureTableF,
};
pub const CutOffInitialPressureUpper0RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper0RpM,
};
pub const CutOffInitialPressureUpper50RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper50RpM,
};
pub const CutOffInitialPressureUpper100RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper100RpM,
};
pub const CutOffInitialPressureUpper150RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper150RpM,
};
pub const CutOffInitialPressureUpper200RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper200RpM,
};
pub const CutOffInitialPressureUpper250RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper250RpM,
};
pub const CutOffInitialPressureUpper300RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper300RpM,
};
pub const CutOffInitialPressureUpper350RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureUpper350RpM,
};
pub const CutOffInitialPressureLower0RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower0RpM,
};
pub const CutOffInitialPressureLower50RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower50RpM,
};
pub const CutOffInitialPressureLower100RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower100RpM,
};
pub const CutOffInitialPressureLower150RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower150RpM,
};
pub const CutOffInitialPressureLower200RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower200RpM,
};
pub const CutOffInitialPressureLower250RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower250RpM,
};
pub const CutOffInitialPressureLower300RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower300RpM,
};
pub const CutOffInitialPressureLower350RpM: Table = Table {
    x: &CutOff,
    y: &InitialPressureLower350RpM,
};
pub fn by_factory(name: &str) -> Option<Table> {
    Some(match name {
        "CylinderEventExhausttoCutoff" => CylinderEventExhausttoCutoff,
        "CylinderEventCompressiontoCutoff" => CylinderEventCompressiontoCutoff,
        "CylinderEventAdmissiontoCutoff" => CylinderEventAdmissiontoCutoff,
        "CylinderCondensationFractionInterpolatorX" => CylinderCondensationFractionInterpolatorX,
        "SuperheatTempLimitInterpolatorXtoDegF" => SuperheatTempLimitInterpolatorXtoDegF,
        "BackpressureSatIHPtoPSI" => BackpressureSatIHPtoPSI,
        "BackpressureSuperIHPtoPSI" => BackpressureSuperIHPtoPSI,
        "SaturatedSpeedFactorSpeedDropFtpMintoX" => SaturatedSpeedFactorSpeedDropFtpMintoX,
        "SuperheatedSpeedFactorSpeedDropFtpMintoX" => SuperheatedSpeedFactorSpeedDropFtpMintoX,
        "SuperInitialPressureDropRatioInterpolatorRpMtoX" => {
            SuperInitialPressureDropRatioInterpolatorRpMtoX
        }
        "SatInitialPressureDropRatioInterpolatorRpMtoX" => {
            SatInitialPressureDropRatioInterpolatorRpMtoX
        }
        "SuperheatTempInterpolatorLbpHtoDegF" => SuperheatTempInterpolatorLbpHtoDegF,
        "InjCapMinFactorInterpolatorX" => InjCapMinFactorInterpolatorX,
        "InjDelWaterTempMaxPressureInterpolatorFtoPSI" => {
            InjDelWaterTempMaxPressureInterpolatorFtoPSI
        }
        "InjDelWaterTempMinPressureInterpolatorFtoPSI" => {
            InjDelWaterTempMinPressureInterpolatorFtoPSI
        }
        "InjWaterFedSteamPressureInterpolatorFtoPSI" => InjWaterFedSteamPressureInterpolatorFtoPSI,
        "SatBoilerEfficiencyGrateAreaInterpolatorLbstoX" => {
            SatBoilerEfficiencyGrateAreaInterpolatorLbstoX
        }
        "SuperBoilerEfficiencyGrateAreaInterpolatorLbstoX" => {
            SuperBoilerEfficiencyGrateAreaInterpolatorLbstoX
        }
        "SaturationPressureInterpolatorKtoPSI" => SaturationPressureInterpolatorKtoPSI,
        "SpecificHeatInterpolatorKtoKJpKGpK" => SpecificHeatInterpolatorKtoKJpKGpK,
        "Injector09FlowrateInterpolatorPSItoUKGpM" => Injector09FlowrateInterpolatorPSItoUKGpM,
        "Injector10FlowrateInterpolatorPSItoUKGpM" => Injector10FlowrateInterpolatorPSItoUKGpM,
        "Injector11FlowrateInterpolatorPSItoUKGpM" => Injector11FlowrateInterpolatorPSItoUKGpM,
        "Injector13FlowrateInterpolatorPSItoUKGpM" => Injector13FlowrateInterpolatorPSItoUKGpM,
        "Injector14FlowrateInterpolatorPSItoUKGpM" => Injector14FlowrateInterpolatorPSItoUKGpM,
        "Injector15FlowrateInterpolatorPSItoUKGpM" => Injector15FlowrateInterpolatorPSItoUKGpM,
        "WaterHeatInterpolatorPSItoBTUpLB" => WaterHeatInterpolatorPSItoBTUpLB,
        "WaterDensityInterpolatorPSItoLBpFT3" => WaterDensityInterpolatorPSItoLBpFT3,
        "SteamHeatInterpolatorPSItoBTUpLB" => SteamHeatInterpolatorPSItoBTUpLB,
        "SteamDensityInterpolatorPSItoLBpFT3" => SteamDensityInterpolatorPSItoLBpFT3,
        "WaterHeatToPressureInterpolatorBTUpLBtoPSI" => WaterHeatToPressureInterpolatorBTUpLBtoPSI,
        "TemperatureToPressureInterpolatorFtoPSI" => TemperatureToPressureInterpolatorFtoPSI,
        "PressureToTemperatureInterpolatorPSItoF" => PressureToTemperatureInterpolatorPSItoF,
        "SteamHeatPressureToTemperatureInterpolatorPSItoF" => {
            SteamHeatPressureToTemperatureInterpolatorPSItoF
        }
        "CutOffInitialPressureUpper0RpM" => CutOffInitialPressureUpper0RpM,
        "CutOffInitialPressureUpper50RpM" => CutOffInitialPressureUpper50RpM,
        "CutOffInitialPressureUpper100RpM" => CutOffInitialPressureUpper100RpM,
        "CutOffInitialPressureUpper150RpM" => CutOffInitialPressureUpper150RpM,
        "CutOffInitialPressureUpper200RpM" => CutOffInitialPressureUpper200RpM,
        "CutOffInitialPressureUpper250RpM" => CutOffInitialPressureUpper250RpM,
        "CutOffInitialPressureUpper300RpM" => CutOffInitialPressureUpper300RpM,
        "CutOffInitialPressureUpper350RpM" => CutOffInitialPressureUpper350RpM,
        "CutOffInitialPressureLower0RpM" => CutOffInitialPressureLower0RpM,
        "CutOffInitialPressureLower50RpM" => CutOffInitialPressureLower50RpM,
        "CutOffInitialPressureLower100RpM" => CutOffInitialPressureLower100RpM,
        "CutOffInitialPressureLower150RpM" => CutOffInitialPressureLower150RpM,
        "CutOffInitialPressureLower200RpM" => CutOffInitialPressureLower200RpM,
        "CutOffInitialPressureLower250RpM" => CutOffInitialPressureLower250RpM,
        "CutOffInitialPressureLower300RpM" => CutOffInitialPressureLower300RpM,
        "CutOffInitialPressureLower350RpM" => CutOffInitialPressureLower350RpM,
        _ => return None,
    })
}

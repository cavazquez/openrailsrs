//! European Train Control System — Rust TCS subset for the DMI (#163).
//!
//! No C# script host: [`BasicEtcsTcs`] derives supervision / TTI / menus from
//! [`crate::LiveDriveSession`] physics (speed limit + distance to stop).

mod braking;
mod menu;
mod status;
mod tcs;

pub use braking::{
    EMERGENCY_DECEL_MPS2, SERVICE_DECEL_MPS2, allowed_on_curve, braking_distance_m,
    indication_distance_m,
};
pub use menu::{
    MenuAction, MenuButtonDef, MenuWindowDef, SoftKeyAction, SoftKeyDef, default_soft_keys,
    main_menu_def, settings_menu_def,
};
pub use status::{
    EtcsLevel, EtcsMode, EtcsMonitor, EtcsSupervision, EtcsTcsStatus, GradientSegment,
    PlanningSymbol, SpeedTarget, TextMessage, TrackCondition, TrackConditionKind, pick_dial_scale,
};
pub use tcs::{BasicEtcsTcs, EtcsTcs};

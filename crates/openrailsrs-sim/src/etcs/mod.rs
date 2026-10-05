//! European Train Control System — Rust TCS subset for the DMI (#163).
//!
//! [`BasicEtcsTcs`] is the default. An explicitly selected C# script can use
//! [`ScriptTcsHost`], a bounded, optional process with a documented OR API subset.

mod braking;
mod menu;
mod script_host;
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
pub use script_host::{
    ScriptContext, ScriptHostConfig, ScriptSignal, ScriptSnapshot, ScriptSpeedPost, ScriptTcsHost,
    TcsInput,
};
pub use status::{
    EtcsLevel, EtcsMode, EtcsMonitor, EtcsSupervision, EtcsTcsStatus, GradientSegment,
    PlanningSymbol, SpeedTarget, TextMessage, TrackCondition, TrackConditionKind, pick_dial_scale,
};
pub use tcs::{BasicEtcsTcs, EtcsTcs};

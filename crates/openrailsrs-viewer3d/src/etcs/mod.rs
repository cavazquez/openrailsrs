//! ETCS Driver Machine Interface (OR `DriverMachineInterface` subset, #159–#162).
//!
//! CPU raster into the cab `ScreenDisplay` texture.

mod colors;
mod gauge;
pub mod input;
mod mode;
mod paint;
mod planning;
mod status;
mod subwindow;
mod symbols;

pub use input::{DmiHit, EtcsUiState, hit_test_dmi, uv_to_dmi};
pub use mode::DmiMode;
pub use paint::{paint_dmi, paint_dmi_full};
pub use status::{
    EtcsLevel, EtcsMode, EtcsMonitor, EtcsStatus, EtcsSupervision, PlanningSymbol,
    etcs_status_from_live,
};
pub use subwindow::DmiOverlay;
pub use symbols::{EtcsSymbols, resolve_etcs_content_dir};

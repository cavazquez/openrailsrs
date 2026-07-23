//! ETCS Driver Machine Interface (OR `DriverMachineInterface` subset, #159–#161).
//!
//! CPU raster into the cab `ScreenDisplay` texture (640×480 Full layout).

mod colors;
mod gauge;
pub mod input;
mod paint;
mod planning;
mod status;
mod symbols;

pub use input::{DmiHit, EtcsUiState, hit_test_dmi, uv_to_dmi};
pub use paint::paint_dmi_full;
pub use status::{
    EtcsMonitor, EtcsStatus, EtcsSupervision, PlanningSymbol, etcs_status_from_live,
};
pub use symbols::{EtcsSymbols, resolve_etcs_content_dir};
